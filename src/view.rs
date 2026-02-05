//! Aggregation model: combine UI tree (from ops.json) + log rows.

use crate::addr::Addr;
use crate::diagnostics;
use crate::log::{LogIndex, LogRow};
use crate::ops::{NodeSpec, RuleSpec};
use crate::Result;

use anyhow::bail;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize)]
pub struct OperatorView {
    pub addr: Vec<u32>,
    pub op_name: String,
    pub activations: u64,
    pub total_active_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NameNodeView {
    pub name: String,
    pub label: String,
    pub block: String,
    pub fingerprint: Option<String>,
    pub tags: Vec<String>,

    /// Primary tree children (spanning tree derived from DAG).
    pub children: Vec<String>,

    /// All DAG parents (may include multi-parent edges).
    pub dag_parents: Vec<String>,

    /// Additional parents beyond the chosen primary parent (for DAG info).
    pub extra_parents: Vec<String>,

    /// Aggregated over operators owned by this name.
    pub self_activations: u64,
    pub self_total_active_ms: f64,

    /// Operators owned by this name (sorted by total_active_ms desc).
    pub operators: Vec<OperatorView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RulePlanNodeView {
    pub fingerprint: String,
    pub node: Option<String>,
    pub label: Option<String>,
    pub children: Vec<String>,
    pub parents: Vec<String>,
    pub shared: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleView {
    pub text: String,
    pub root: String,
    pub nodes: BTreeMap<String, RulePlanNodeView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportData {
    pub roots: Vec<String>,
    pub nodes: BTreeMap<String, NameNodeView>,
    pub rules: Vec<RuleView>,
    pub totals: TotalsView,
}

#[derive(Debug, Clone, Serialize)]
pub struct TotalsView {
    pub names: usize,
    pub operators_in_log: usize,
    pub operators_mapped: usize,
    pub total_mapped_ms: f64,
    pub total_mapped_activations: u64,
}

/// Build report data. Performs:
/// - detect operator addr assigned to multiple names (error)
/// - warn (stderr) about mapped addrs missing from log
pub fn build_report_data(
    nodes_spec: &BTreeMap<String, NodeSpec>,
    roots: &[String],
    rules_spec: &[RuleSpec],
    fingerprint_to_node: &BTreeMap<String, String>,
    log: &LogIndex,
) -> Result<ReportData> {
    // Phase 1: enforce each operator addr belongs to at most one name (strict).
    let mut owner: BTreeMap<&Addr, &str> = BTreeMap::new();
    for (name, spec) in nodes_spec {
        for addr in &spec.operators {
            if let Some(prev) = owner.insert(addr, name.as_str()) {
                bail!(
                    "{}",
                    diagnostics::error_message(format!(
                        "operator addr {:?} is assigned to multiple names: {} and {}",
                        addr.0,
                        prev,
                        name
                    ))
                );
            }
        }
    }

    // Phase 2: normalize parent lists once and derive tree/DAG metadata.
    // We choose a primary parent for each node: lexicographically smallest parent.
    let mut normalized_parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, spec) in nodes_spec {
        let parents = normalize_parents(spec.parents.iter().map(|p| p.to_string()).collect());
        normalized_parents.insert(name.clone(), parents);
    }

    let mut primary_parent: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut extra_parents: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (name, parents) in &normalized_parents {
        if parents.is_empty() {
            primary_parent.insert(name.clone(), None);
            extra_parents.insert(name.clone(), vec![]);
            continue;
        }

        let primary = parents[0].clone();
        let extras = parents[1..].to_vec();
        primary_parent.insert(name.clone(), Some(primary));
        extra_parents.insert(name.clone(), extras);
    }

    let mut tree_children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (child, pp) in &primary_parent {
        if let Some(p) = pp {
            tree_children
                .entry(p.clone())
                .or_default()
                .push(child.clone());
        }
    }
    for kids in tree_children.values_mut() {
        kids.sort();
    }

    // Build DAG parents list for direct consumption in the renderer.
    let dag_parents: BTreeMap<String, Vec<String>> = normalized_parents.clone();

    // Roots for the tree view: use provided roots, plus any node that has no primary parent.
    let mut roots_set: BTreeSet<String> = roots.iter().cloned().collect();
    for (name, pp) in &primary_parent {
        if pp.is_none() {
            roots_set.insert(name.clone());
        }
    }
    let roots: Vec<String> = roots_set.into_iter().collect();

    // Phase 3: build per-name operator lists + aggregates.
    let mut nodes_view: BTreeMap<String, NameNodeView> = BTreeMap::new();

    let mut total_mapped_ms = 0.0f64;
    let mut total_mapped_activations = 0u64;
    let mut operators_mapped = 0usize;

    for (name, spec) in nodes_spec {
        let mut operators: Vec<OperatorView> = Vec::new();
        let mut self_ms = 0.0f64;
        let mut self_act = 0u64;

        for addr in &spec.operators {
            match log.get(addr) {
                Some(LogRow {
                    addr,
                    activations,
                    total_active_ms,
                    op_name,
                }) => {
                    operators.push(OperatorView {
                        addr: addr.0.clone(),
                        op_name: op_name.clone(),
                        activations: *activations,
                        total_active_ms: *total_active_ms,
                    });
                    self_ms += *total_active_ms;
                    self_act += *activations;

                    total_mapped_ms += *total_active_ms;
                    total_mapped_activations += *activations;
                    operators_mapped += 1;
                }
                None => diagnostics::warn(format!(
                    "ops.json maps name '{}' to addr {:?}, but addr not found in log",
                    name, addr.0
                )),
            }
        }

        // Sort operators by addr only (stable, deterministic).
        operators.sort_by(|a, b| a.addr.cmp(&b.addr));

        nodes_view.insert(
            name.clone(),
            NameNodeView {
                name: name.clone(),
                label: spec.label.clone(),
                block: spec.block.clone(),
                fingerprint: spec.fingerprint.clone(),
                tags: spec.tags.clone(),
                children: tree_children.get(name).cloned().unwrap_or_default(),
                dag_parents: dag_parents.get(name).cloned().unwrap_or_default(),
                extra_parents: extra_parents.get(name).cloned().unwrap_or_default(),
                self_activations: self_act,
                self_total_active_ms: self_ms,
                operators,
            },
        );
    }

    Ok(ReportData {
        roots,
        totals: TotalsView {
            names: nodes_spec.len(),
            operators_in_log: log.len(),
            operators_mapped,
            total_mapped_ms,
            total_mapped_activations,
        },
        nodes: nodes_view,
        rules: build_rule_views(rules_spec, nodes_spec, fingerprint_to_node),
    })
}

fn build_rule_views(
    rules_spec: &[RuleSpec],
    nodes_spec: &BTreeMap<String, NodeSpec>,
    fingerprint_to_node: &BTreeMap<String, String>,
) -> Vec<RuleView> {
    let mut views = Vec::new();

    for rule in rules_spec {
        // Parent list for shared-node detection.
        let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (fp, node) in &rule.nodes {
            for child in &node.children {
                parents.entry(child.clone()).or_default().push(fp.clone());
            }
        }

        let mut nodes_view: BTreeMap<String, RulePlanNodeView> = BTreeMap::new();
        for (fp, node) in &rule.nodes {
            let node_name = fingerprint_to_node.get(fp).cloned();
            let label = node_name
                .as_ref()
                .and_then(|n| nodes_spec.get(n))
                .map(|s| s.label.clone());

            let parent_list = parents.get(fp).cloned().unwrap_or_default();
            let shared = parent_list.len() > 1;

            nodes_view.insert(
                fp.clone(),
                RulePlanNodeView {
                    fingerprint: fp.clone(),
                    node: node_name,
                    label,
                    children: node.children.clone(),
                    parents: parent_list,
                    shared,
                },
            );
        }

        views.push(RuleView {
            text: rule.text.clone(),
            root: rule.root.clone(),
            nodes: nodes_view,
        });
    }

    views
}

fn normalize_parents<T: Ord>(mut parents: Vec<T>) -> Vec<T> {
    parents.sort();
    parents.dedup();
    parents
}
