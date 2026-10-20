// 固定版から得た構成だけを描く。通信や試験の結果を図へ補わない。
const NS = 'http://www.w3.org/2000/svg';
function svg(tag, attrs = {}, text) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (text !== undefined) e.textContent = text;
  return e;
}
export function mountDiagram(root, graph, { technical, onSelect, selected = null }) {
  root.replaceChildren();
  const nodes = graph.nodes || [];
  if (!nodes.length) {
    const p = document.createElement('p');
    p.textContent = '読んだファイルから構成を作れませんでした。読み取れなかった範囲を確認し、全体への疑問を残してください。';
    root.append(p); return;
  }
  const cols = nodes.length === 1 ? 1 : 2, width = cols * 300 + 32;
  const rows = Math.ceil(nodes.length / cols), height = rows * 148 + 32;
  const chart = svg('svg', { viewBox: `0 0 ${width} ${height}`, role: 'group', 'aria-label': `${graph.repository}の構成図` });
  const defs = svg('defs'), marker = svg('marker', { id: 'repository-arrow', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' });
  marker.append(svg('path', { d: 'M 0 0 L 10 5 L 0 10 z', fill: '#607386' })); defs.append(marker); chart.append(defs);
  const positions = new Map(nodes.map((n, i) => [n.id, { x: 20 + (i % cols) * 300, y: 20 + Math.floor(i / cols) * 148 }]));
  for (const edge of graph.edges || []) {
    const a = positions.get(edge.from), b = positions.get(edge.to);
    if (!a || !b) continue;
    const sameRow = a.y === b.y;
    const x1 = a.x + (sameRow ? (a.x < b.x ? 268 : 0) : 134), y1 = a.y + (sameRow ? 55 : a.y < b.y ? 110 : 0);
    const x2 = b.x + (sameRow ? (a.x < b.x ? 0 : 268) : 134), y2 = b.y + (sameRow ? 55 : a.y < b.y ? 0 : 110);
    const line = svg('path', { d: `M${x1},${y1} L${x2},${y2}`, class: 'map-edge', 'marker-end': 'url(#repository-arrow)' });
    line.append(svg('title', {}, `${edge.from}から${edge.to}への参照宣言`)); chart.append(line);
  }
  const buttons = [];
  for (const node of nodes) {
    const { x, y } = positions.get(node.id);
    const group = svg('g', { transform: `translate(${x},${y})`, role: 'button', tabindex: 0, 'aria-label': `${technical ? node.path : node.title}（${node.path}）`, 'aria-pressed': String(node.id === selected), class: 'map-node', 'data-node': node.id });
    group.append(svg('rect', { width: 268, height: 110, rx: 12 }));
    const label = technical ? node.path : node.title;
    const chunks = Array.from(label).reduce((a, ch, i) => { const row = Math.floor(i / 17); a[row] = (a[row] || '') + ch; return a; }, []);
    chunks.slice(0, 2).forEach((line, i) => group.append(svg('text', { x: 16, y: 30 + i * 22, class: 'map-node-title' }, line + (i === 1 && chunks.length > 2 ? '…' : ''))));
    const filename = technical ? node.title : node.path;
    group.append(svg('text', { x: 16, y: 87, class: 'map-node-file' }, filename.length > 30 ? filename.slice(0, 27) + '…' : filename));
    const select = () => { for (const g of buttons) g.setAttribute('aria-pressed', String(g === group)); onSelect(node); };
    group.addEventListener('click', select);
    group.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(); } });
    buttons.push(group); chart.append(group);
  }
  root.append(chart);
}
