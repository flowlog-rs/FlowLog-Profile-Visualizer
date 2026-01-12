use crate::model::ReportData;

/// Render a self-contained HTML report (data embedded as JSON).
///
/// Important: we avoid `format!()` because the HTML contains many `{}` from JS
/// template literals (e.g., `${x}`), which would conflict with Rust formatting.
pub fn render_html_report(data: &ReportData) -> anyhow::Result<String> {
    let json = serde_json::to_string(data)?; // embedded as JS object literal

    const TEMPLATE: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FlowLog Profiler</title>
<style>
  body { font-family: system-ui, -apple-system, Segoe UI, Roboto, Arial, sans-serif; margin: 0; }
  header { padding: 12px 16px; border-bottom: 1px solid #ddd; }
  .container { display: flex; height: calc(100vh - 58px); }
  .sidebar { width: 360px; border-right: 1px solid #ddd; padding: 12px; overflow: auto; }
  .main { flex: 1; padding: 12px; overflow: hidden; display: flex; flex-direction: column; gap: 8px; }

  .summary { display: flex; gap: 16px; flex-wrap: wrap; font-size: 14px; color: #333; }
  .pill { padding: 4px 8px; border: 1px solid #ddd; border-radius: 999px; background: #fafafa; }

  .tree-node { cursor: pointer; user-select: none; padding: 2px 4px; border-radius: 4px; }
  .tree-node:hover { background: #f3f3f3; }
  .tree-node.selected { background: #e9f2ff; border: 1px solid #cfe3ff; }
  .indent { display: inline-block; width: 16px; }
  .toggle { display: inline-block; width: 16px; text-align: center; color: #666; }
  .muted { color: #777; font-size: 12px; }

  .tabs { display: flex; gap: 8px; margin-bottom: 8px; }
  .tab { padding: 6px 10px; border: 1px solid #ddd; background: #f8f8f8; border-radius: 6px; cursor: pointer; }
  .tab.active { background: #e9f2ff; border-color: #cfe3ff; }

  #graphPane { flex: 1; display: flex; flex-direction: column; }
  #graphView {
    flex: 1;
    width: 100%;
    height: 100%;
    min-height: 420px;
    border: 1px solid #eee;
    border-radius: 8px;
    overflow: auto;
  }

  svg { width: 100%; height: 100%; }
  #graphView svg { cursor: grab; }
  #graphView svg:active { cursor: grabbing; }

  .g-edge { stroke: #999; stroke-width: 1.4; fill: none; pointer-events: none; }
  .g-node rect { rx: 6; ry: 6; stroke: #5570d4; stroke-width: 1; }
  .g-node text { font-size: 12px; fill: #111; pointer-events: none; }
  .g-node.selected rect { stroke: #111; stroke-width: 2; }

  table { border-collapse: collapse; width: 100%; margin-top: 8px; }
  th, td { border-bottom: 1px solid #eee; padding: 6px 8px; text-align: left; font-size: 14px; }
  th { position: sticky; top: 0; background: white; border-bottom: 1px solid #ddd; }
  .num { text-align: right; font-variant-numeric: tabular-nums; }
  code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 13px; }
</style>
</head>
<body>
<header>
  <div class="summary" id="summary"></div>
</header>

<div class="container">
  <div class="sidebar">
    <div style="display:flex; gap: 8px; margin-bottom: 8px;">
      <input id="search" placeholder="Search name..." style="flex:1; padding: 6px 8px; border: 1px solid #ddd; border-radius: 6px;">
      <button id="expandAll" style="padding: 6px 10px;">Expand</button>
      <button id="collapseAll" style="padding: 6px 10px;">Collapse</button>
    </div>
    <div id="tree"></div>
  </div>

  <div class="main">
    <div class="tabs">
      <button class="tab active" id="tabTree">Tree</button>
      <button class="tab" id="tabGraph">Graph</button>
    </div>

    <div id="detailPane">
      <h2 id="title">Select a node</h2>
      <div id="meta" class="muted"></div>

      <table id="opsTable" style="display:none;">
        <thead>
          <tr>
            <th>addr</th>
            <th>operator</th>
            <th class="num">activations</th>
            <th class="num">total_active_ms</th>
          </tr>
        </thead>
        <tbody id="opsBody"></tbody>
      </table>
    </div>

    <div id="graphPane" style="display:none;">
      <div id="graphView"></div>
    </div>
  </div>
</div>

<script>
// Embedded report data (JSON object literal)
const DATA = __DATA__;

const state = {
  expanded: new Set(),
  selected: null,
  search: "",
  view: "tree",
  graph: { tx: 0, ty: 0, scale: 1 }, // pan/zoom
};

const NODE = {
  padX: 12,
  padY: 8,
  lineH: 16,
  minW: 140,
  maxW: 360,
  minH: 36,
  font: "12px system-ui, -apple-system, Segoe UI, Roboto, Arial, sans-serif",
};

const _measureCanvas = document.createElement("canvas");
const _measureCtx = _measureCanvas.getContext("2d");

function fmtMs(x) {
  return (Math.round(x * 1000) / 1000).toFixed(3);
}

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function measureTextPx(s) {
  _measureCtx.font = NODE.font;
  return _measureCtx.measureText(String(s)).width;
}

function wrapLines(text, maxContentW) {
  const words = String(text).split(/\s+/).filter(Boolean);
  if (!words.length) return [""];

  const lines = [];
  let cur = words[0];

  for (let i = 1; i < words.length; i++) {
    const next = `${cur} ${words[i]}`;
    if (measureTextPx(next) <= maxContentW) {
      cur = next;
    } else {
      lines.push(cur);
      cur = words[i];
    }
  }
  lines.push(cur);
  return lines;
}

function nodeBox(label) {
  const maxContentW = NODE.maxW - 2 * NODE.padX;
  const lines = wrapLines(label, maxContentW);

  const contentW = Math.min(
    maxContentW,
    Math.max(...lines.map(measureTextPx), 0)
  );

  const w = Math.max(NODE.minW, Math.min(NODE.maxW, contentW + 2 * NODE.padX));
  const h = Math.max(NODE.minH, lines.length * NODE.lineH + 2 * NODE.padY);
  return { w, h, lines };
}

function renderSummary() {
  const t = DATA.totals;
  const el = document.getElementById("summary");
  el.innerHTML = `
    <span class="pill">names: <b>${t.names}</b></span>
    <span class="pill">operators in log: <b>${t.operators_in_log}</b></span>
    <span class="pill">operators mapped: <b>${t.operators_mapped}</b></span>
    <span class="pill">mapped ms: <b>${fmtMs(t.total_mapped_ms)}</b></span>
    <span class="pill">mapped activations: <b>${t.total_mapped_activations}</b></span>
  `;
}

function nodeMatches(name, node) {
  if (!state.search) return true;
  const s = state.search.toLowerCase();
  return name.toLowerCase().includes(s) || (node.label || "").toLowerCase().includes(s);
}

function renderTree() {
  const root = document.getElementById("tree");
  root.innerHTML = "";

  // If search is active, show matches + ancestors in the spanning tree.
  const mustShow = new Set();
  if (state.search) {
    const parent = new Map();
    for (const [name, node] of Object.entries(DATA.nodes)) {
      for (const c of node.children) parent.set(c, name);
    }
    for (const [name, node] of Object.entries(DATA.nodes)) {
      if (nodeMatches(name, node)) {
        let cur = name;
        while (cur) {
          mustShow.add(cur);
          cur = parent.get(cur);
        }
      }
    }
  }

  function renderSubtree(name, depth) {
    const node = DATA.nodes[name];
    if (!node) return;

    if (state.search && !mustShow.has(name)) return;

    const isExpanded = state.expanded.has(name);
    const hasKids = node.children && node.children.length > 0;

    const row = document.createElement("div");
    row.className = "tree-node" + (state.selected === name ? " selected" : "");
    row.onclick = () => selectNode(name);

    const indent = document.createElement("span");
    indent.className = "indent";
    indent.style.width = `${depth * 16}px`;
    row.appendChild(indent);

    const toggle = document.createElement("span");
    toggle.className = "toggle";
    toggle.textContent = hasKids ? (isExpanded ? "▾" : "▸") : " ";
    toggle.onclick = (e) => {
      e.stopPropagation();
      if (!hasKids) return;
      if (isExpanded) state.expanded.delete(name);
      else state.expanded.add(name);
      renderTree();
    };
    row.appendChild(toggle);

    const label = document.createElement("span");
    label.innerHTML = `${escapeHtml(node.label)} <span class="muted">(${fmtMs(node.self_total_active_ms)} ms, ${node.self_activations} act)</span>`;
    row.appendChild(label);

    root.appendChild(row);

    if (hasKids && isExpanded) {
      for (const c of node.children) renderSubtree(c, depth + 1);
    }
  }

  for (const r of DATA.roots) renderSubtree(r, 0);
}

function renderGraph() {
  const container = document.getElementById("graphView");
  const nodes = DATA.nodes;
  const allNames = Object.keys(nodes);

  // Prefer explicit DAG edges if present, else fall back to tree children.
  const childrenOf = (name) => nodes[name]?.dag_children || nodes[name]?.children || [];

  // --- Build DAG edges (parent -> child) ---
  const parents = new Map();  // child -> [parents...]
  const children = new Map(); // parent -> [children...]
  const indeg = new Map();

  for (const n of allNames) {
    parents.set(n, []);
    children.set(n, []);
    indeg.set(n, 0);
  }

  for (const u of allNames) {
    for (const v of childrenOf(u)) {
      if (!nodes[v]) continue;
      parents.get(v).push(u);
      children.get(u).push(v);
      indeg.set(v, (indeg.get(v) ?? 0) + 1);
    }
  }

  // --- Topological order (Kahn). If cycle exists, still render with fallback. ---
  const q = [];
  for (const n of allNames) {
    if ((indeg.get(n) ?? 0) === 0) q.push(n);
  }

  const topo = [];
  while (q.length) {
    const u = q.shift();
    topo.push(u);
    for (const v of children.get(u) || []) {
      indeg.set(v, indeg.get(v) - 1);
      if (indeg.get(v) === 0) q.push(v);
    }
  }

  if (topo.length !== allNames.length) {
    const seen = new Set(topo);
    const rest = allNames.filter((n) => !seen.has(n)).sort();
    topo.push(...rest);
  }

  // --- Depth = max(parent depth) + 1 (ensures below *all* parents) ---
  const depth = new Map();
  for (const n of allNames) depth.set(n, 0);
  for (const r of DATA.roots || []) depth.set(r, 0);

  for (const v of topo) {
    let d = depth.get(v) ?? 0;
    for (const p of parents.get(v) || []) {
      const pd = depth.get(p) ?? 0;
      d = Math.max(d, pd + 1);
    }
    depth.set(v, d);
  }

  // --- Group into layers by depth ---
  const layers = new Map(); // depth -> [names]
  for (const n of allNames) {
    const d = depth.get(n) ?? 0;
    if (!layers.has(d)) layers.set(d, []);
    layers.get(d).push(n);
  }

  for (const list of layers.values()) list.sort();
  const layerKeys = [...layers.keys()].sort((a, b) => a - b);

  // --- Order within layers: barycenter heuristic to reduce crossings ---
  function barycenterSortDown() {
    for (let i = 1; i < layerKeys.length; i++) {
      const d = layerKeys[i];
      const prev = layers.get(layerKeys[i - 1]) || [];
      const idxPrev = new Map(prev.map((n, j) => [n, j]));

      const cur = layers.get(d) || [];
      cur.sort((a, b) => {
        const pa = parents.get(a) || [];
        const pb = parents.get(b) || [];
        const ba =
          pa.length === 0
            ? Number.POSITIVE_INFINITY
            : pa.reduce((s, p) => s + (idxPrev.get(p) ?? 0), 0) / pa.length;
        const bb =
          pb.length === 0
            ? Number.POSITIVE_INFINITY
            : pb.reduce((s, p) => s + (idxPrev.get(p) ?? 0), 0) / pb.length;

        if (ba !== bb) return ba - bb;
        return a.localeCompare(b);
      });
    }
  }

  function barycenterSortUp() {
    for (let i = layerKeys.length - 2; i >= 0; i--) {
      const d = layerKeys[i];
      const next = layers.get(layerKeys[i + 1]) || [];
      const idxNext = new Map(next.map((n, j) => [n, j]));

      const cur = layers.get(d) || [];
      cur.sort((a, b) => {
        const ca = children.get(a) || [];
        const cb = children.get(b) || [];
        const ba =
          ca.length === 0
            ? Number.POSITIVE_INFINITY
            : ca.reduce((s, c) => s + (idxNext.get(c) ?? 0), 0) / ca.length;
        const bb =
          cb.length === 0
            ? Number.POSITIVE_INFINITY
            : cb.reduce((s, c) => s + (idxNext.get(c) ?? 0), 0) / cb.length;

        if (ba !== bb) return ba - bb;
        return a.localeCompare(b);
      });
    }
  }

  barycenterSortDown();
  barycenterSortUp();
  barycenterSortDown();

  // --- Compute positions (initial centers) ---
  const layerGap = 120;
  const maxLayerCount = Math.max(...layerKeys.map((d) => (layers.get(d) || []).length), 1);
  const width = Math.max(960, maxLayerCount * 220);
  const height = layerKeys.length * layerGap + 80;

  const pos = new Map();
  for (let li = 0; li < layerKeys.length; li++) {
    const d = layerKeys[li];
    const list = layers.get(d) || [];
    const step = width / (list.length + 1);
    list.forEach((name, idx) => {
      pos.set(name, { x: (idx + 1) * step, y: 40 + li * layerGap });
    });
  }

  // --- Measure + place nodes (auto-sized) ---
  const boxByName = new Map(); // name -> {x0,y0,w,h,cx,cy,lines}
  for (const [name, node] of Object.entries(nodes)) {
    const p = pos.get(name);
    if (!p) continue;
    const { w, h, lines } = nodeBox(node.label || name);
    boxByName.set(name, {
      x0: p.x - w / 2,
      y0: p.y - h / 2,
      w,
      h,
      cx: p.x,
      cy: p.y,
      lines,
    });
  }

  // Node color scale based on self time.
  const maxMs = Math.max(
    ...Object.values(nodes).map((n) => n.self_total_active_ms || 0),
    0.0001
  );

  function color(ms) {
    const t = Math.min(1, (ms || 0) / maxMs);
    const c1 = [233, 242, 255];
    const c2 = [91, 141, 239];
    const mix = c1.map((v, i) => Math.round(v + (c2[i] - v) * t));
    return `rgb(${mix[0]},${mix[1]},${mix[2]})`;
  }

  function edgePath(fromBox, toBox) {
    const x1 = fromBox.cx;
    const y1 = fromBox.y0 + fromBox.h; // bottom
    const x2 = toBox.cx;
    const y2 = toBox.y0; // top
    const midY = (y1 + y2) / 2;
    return `M ${x1} ${y1} L ${x1} ${midY} L ${x2} ${midY} L ${x2} ${y2}`;
  }

  // --- Build edges as paths from box boundary to box boundary ---
  let edges = "";
  for (const [name, node] of Object.entries(nodes)) {
    const from = boxByName.get(name);
    if (!from) continue;
    const dagKids = node.dag_children || node.children || [];
    for (const c of dagKids) {
      const to = boxByName.get(c);
      if (!to) continue;
      edges += `<path class="g-edge" d="${edgePath(from, to)}" />`;
    }
  }

  // --- Build vertices with wrapped text ---
  let verts = "";
  for (const [name, node] of Object.entries(nodes)) {
    const b = boxByName.get(name);
    if (!b) continue;

    const isSel = state.selected === name;
    const ms = node.self_total_active_ms || 0;
    const labelEsc = escapeHtml(node.label || name);

    const tspans = b.lines
      .map((ln, i) => {
        const safe = escapeHtml(ln);
        const dy = i === 0 ? 0 : NODE.lineH;
        return `<tspan x="${b.w / 2}" dy="${dy}">${safe}</tspan>`;
      })
      .join("");

    const textY0 = NODE.padY + 12;

    verts += `
      <g class="g-node${isSel ? " selected" : ""}" data-name="${name}" transform="translate(${b.x0}, ${b.y0})">
        <rect width="${b.w}" height="${b.h}" fill="${color(ms)}"></rect>
        <text x="${b.w / 2}" y="${textY0}" text-anchor="middle">${tspans}</text>
        <title>${labelEsc}\nself_ms: ${fmtMs(ms)}\nactivations: ${node.self_activations}</title>
      </g>`;
  }

  container.innerHTML = `
    <svg id="graphSvg" viewBox="0 0 ${width} ${height}">
      <g id="viewport">
        ${edges}
        ${verts}
      </g>
    </svg>
  `;

  const svg = document.getElementById("graphSvg");
  const viewport = document.getElementById("viewport");

  function applyTransform() {
    viewport.setAttribute(
      "transform",
      `translate(${state.graph.tx} ${state.graph.ty}) scale(${state.graph.scale})`
    );
  }
  applyTransform();

  // Click-to-select still works.
  svg.addEventListener("click", (e) => {
    const g = e.target.closest(".g-node");
    if (!g) return;
    const name = g.getAttribute("data-name");
    if (name) selectNode(name);
  });

  // --- Pan (drag) ---
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  svg.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    svg.setPointerCapture(e.pointerId);
  });

  svg.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;

    // divide by scale so pan speed is consistent when zoomed
    state.graph.tx += dx / state.graph.scale;
    state.graph.ty += dy / state.graph.scale;
    applyTransform();
  });

  svg.addEventListener("pointerup", () => {
    dragging = false;
  });
  svg.addEventListener("pointercancel", () => {
    dragging = false;
  });

  // --- Zoom (wheel) ---
  svg.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();

      const oldScale = state.graph.scale;
      const factor = Math.exp(-e.deltaY * 0.001);
      let newScale = oldScale * factor;
      newScale = Math.max(0.2, Math.min(4.0, newScale));
      if (newScale === oldScale) return;

      // Zoom around cursor (keep point under mouse stable).
      const pt = svg.createSVGPoint();
      pt.x = e.clientX;
      pt.y = e.clientY;
      const cursor = pt.matrixTransform(svg.getScreenCTM().inverse());

      const k = newScale / oldScale;
      state.graph.tx = state.graph.tx + (cursor.x - state.graph.tx) * (1 - k);
      state.graph.ty = state.graph.ty + (cursor.y - state.graph.ty) * (1 - k);
      state.graph.scale = newScale;

      applyTransform();
    },
    { passive: false }
  );
}

function selectNode(name) {
  state.selected = name;

  const node = DATA.nodes[name];
  document.getElementById("title").textContent = node.label;

  const extra =
    node.extra_parents && node.extra_parents.length
      ? ` extra parents: ${node.extra_parents.join(", ")}`
      : "";

  document.getElementById("meta").textContent =
    `name: ${name} | self: ${fmtMs(node.self_total_active_ms)} ms | activations: ${node.self_activations}` +
    extra;

  const tbl = document.getElementById("opsTable");
  const body = document.getElementById("opsBody");
  body.innerHTML = "";

  if (!node.operators || node.operators.length === 0) {
    tbl.style.display = "none";
  } else {
    tbl.style.display = "table";
    for (const op of node.operators) {
      const tr = document.createElement("tr");
      tr.innerHTML = `
        <td><code>[${op.addr.join(", ")}]</code></td>
        <td>${escapeHtml(op.op_name)}</td>
        <td class="num">${op.activations}</td>
        <td class="num">${fmtMs(op.total_active_ms)}</td>
      `;
      body.appendChild(tr);
    }
  }

  renderTree();
  renderGraph();
}

function expandAll() {
  for (const name of Object.keys(DATA.nodes)) {
    const node = DATA.nodes[name];
    if (node.children && node.children.length) state.expanded.add(name);
  }
  renderTree();
}

function collapseAll() {
  state.expanded.clear();
  renderTree();
}

document.getElementById("search").addEventListener("input", (e) => {
  state.search = e.target.value || "";
  renderTree();
});

document.getElementById("expandAll").onclick = expandAll;
document.getElementById("collapseAll").onclick = collapseAll;

document.getElementById("tabTree").onclick = () => {
  state.view = "tree";
  document.getElementById("detailPane").style.display = "block";
  document.getElementById("graphPane").style.display = "none";
  document.getElementById("tabTree").classList.add("active");
  document.getElementById("tabGraph").classList.remove("active");
};

document.getElementById("tabGraph").onclick = () => {
  state.view = "graph";
  document.getElementById("detailPane").style.display = "none";
  document.getElementById("graphPane").style.display = "flex"; // important
  document.getElementById("tabGraph").classList.add("active");
  document.getElementById("tabTree").classList.remove("active");
  renderGraph();
};

renderSummary();
for (const r of DATA.roots) state.expanded.add(r);
renderTree();
if (DATA.roots.length) selectNode(DATA.roots[0]);
</script>
</body>
</html>
"#;

    Ok(TEMPLATE.replace("__DATA__", &json))
}
