//! Ops spec now contains both UI tree topology and operator addresses.
//!
//! The JSON format (see examples/ops.json) provides several buckets
//! (input / strata[*].enter|stages|runtime|leave / inspect). We flatten all
//! node entries into a single map keyed by numeric id, then derive parents
//! and roots from the declared children.

use crate::spec::Addr;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Deserialize)]
pub struct OpsSpec {
    #[serde(default)]
    pub input: Vec<RawNode>,

    #[serde(default)]
    pub strata: Vec<Stratum>,

    #[serde(default)]
    pub inspect: Vec<RawNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stratum {
    #[allow(dead_code)]
    pub label: String,

    #[serde(default)]
    pub enter: Vec<RawNode>,

    #[serde(default)]
    pub rules: Vec<Rule>,

    #[serde(default)]
    pub runtime: Vec<RawNode>,

    #[serde(default)]
    pub leave: Vec<RawNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[allow(dead_code)]
    pub rule: String,
    #[serde(default)]
    pub stages: Vec<RawNode>,
}

/// Raw node shape as it appears in ops.json buckets.
#[derive(Debug, Clone, Deserialize)]
pub struct RawNode {
    pub id: u32,
    pub label: String,

    #[serde(default)]
    pub operators: Vec<OperatorRefSpec>,

    #[serde(default)]
    pub children: Vec<u32>,
}

/// Flattened, validated node ready for aggregation.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub id: u32,
    pub label: String,
    pub children: Vec<u32>,
    pub operators: BTreeSet<Addr>,
}

/// Operator references in ops.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OperatorRefSpec {
    Explicit { addr: Vec<u32> },
}

impl OpsSpec {
    /// Flatten all buckets, ensure unique ids, and compute roots.
    pub fn validate_and_build(&self) -> anyhow::Result<ValidatedOps> {
        use anyhow::bail;

        // Gather every RawNode from all buckets.
        let mut raw_nodes: Vec<RawNode> = Vec::new();
        raw_nodes.extend(self.input.clone());
        for s in &self.strata {
            raw_nodes.extend(s.enter.clone());
            for r in &s.rules {
                raw_nodes.extend(r.stages.clone());
            }
            raw_nodes.extend(s.runtime.clone());
            raw_nodes.extend(s.leave.clone());
        }
        raw_nodes.extend(self.inspect.clone());

        // Build map keyed by id, check duplicates.
        let mut nodes: BTreeMap<u32, NodeSpec> = BTreeMap::new();
        for raw in raw_nodes {
            if nodes.contains_key(&raw.id) {
                bail!("duplicate node id in ops.json: {}", raw.id);
            }

            let mut ops = BTreeSet::new();
            for op in raw.operators {
                match op {
                    OperatorRefSpec::Explicit { addr } => {
                        ops.insert(Addr::new(addr));
                    }
                }
            }

            nodes.insert(
                raw.id,
                NodeSpec {
                    id: raw.id,
                    label: raw.label,
                    children: raw.children,
                    operators: ops,
                },
            );
        }

        if nodes.is_empty() {
            bail!("ops.json contained no nodes");
        }

        // Compute parents map and roots from children edges.
        let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, node) in &nodes {
            for &cid in &node.children {
                parents.entry(cid).or_default().push(*pid);
            }
        }

        let mut roots: Vec<u32> = Vec::new();
        for id in nodes.keys() {
            if !parents.contains_key(id) {
                roots.push(*id);
            }
        }
        roots.sort();
        roots.dedup();

        // Basic sanity: every child id must exist.
        for node in nodes.values() {
            for cid in &node.children {
                if !nodes.contains_key(cid) {
                    bail!("node {} references missing child id {}", node.id, cid);
                }
            }
        }

        Ok(ValidatedOps { nodes, roots })
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedOps {
    pub nodes: BTreeMap<u32, NodeSpec>,
    pub roots: Vec<u32>,
}
