// 同じ要素・接続・配置を保ち、説明の詳しさだけを変える。外部へ図を送信しない。
const NS = 'http://www.w3.org/2000/svg', layouts = new WeakMap(), generations = new WeakMap();
const WIDTH = 272, HEIGHT = 144;
function svg(tag, attrs = {}, text) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (text !== undefined) e.textContent = text;
  return e;
}
function wrap(text, width, size = 16) {
  const lines = []; let line = '', length = 0;
  for (const ch of String(text)) {
    const w = ch.charCodeAt(0) > 255 ? size : size * .56;
    if (length + w > width && line) { lines.push(line); line = ''; length = 0; }
    line += ch; length += w;
  }
  if (line) lines.push(line);
  return lines;
}
export function objectTitle(object, technical) {
  if (!object) return 'システム全体について';
  return technical && object.technology?.length ? `${object.title} / ${object.technology.join(' / ')}` : object.title;
}
export function selectDiagramObject(root, selected) {
  for (const n of root.querySelectorAll('[data-object]')) n.setAttribute('aria-pressed', String(n.dataset.object === selected));
}
function structure(graph) {
  const children = graph.boundaries.map(g => ({ id: g.id, layoutOptions: { 'elk.padding': '[top=48,left=20,bottom=20,right=20]', 'elk.spacing.nodeNode': '32' }, children: graph.nodes.filter(n => n.parent === g.id).map(n => ({ id: n.id, width: WIDTH, height: HEIGHT })) }));
  children.push(...graph.nodes.filter(n => !n.parent).map(n => ({ id: n.id, width: WIDTH, height: HEIGHT })));
  return {
    id: 'system', layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.edgeRouting': 'ORTHOGONAL', 'elk.hierarchyHandling': 'INCLUDE_CHILDREN', 'elk.layered.spacing.nodeNodeBetweenLayers': '40', 'elk.spacing.edgeNode': '16', 'elk.spacing.nodeNode': '24', 'elk.spacing.edgeEdge': '24', 'elk.padding': '[top=20,left=20,bottom=20,right=20]' }, children,
    edges: graph.edges.map(e => { const lines = wrap(e.title, 170, 14); return { id: e.id, sources: [e.from], targets: [e.to], labels: [{ text: e.title, width: Math.min(180, Math.max(...lines.map(l => Array.from(l).length * 14), 70)), height: lines.length * 20 + 10 }] }; })
  };
}
function icon(group, role) {
  const i = svg('g', { transform: 'translate(16,18)', class: 'map-icon', 'aria-hidden': 'true' });
  if (role === 'actor') {
    i.append(svg('circle', { cx: 14, cy: 6, r: 6 }), svg('path', { d: 'M2 28 V23 Q14 11 26 23 V28' }));
  } else if (['database', 'storage', 'cache'].includes(role)) {
    i.append(svg('path', { d: 'M0 4 C0 -1 26 -1 26 4 L26 24 C26 29 0 29 0 24 Z M0 4 C0 9 26 9 26 4 M0 17 C0 22 26 22 26 17' }));
  } else if (role === 'ui') {
    i.append(svg('rect', { x: 0, y: 0, width: 28, height: 24, rx: 3 }), svg('path', { d: 'M0 7 H28 M4 3 H5 M9 3 H10' }));
  } else if (role === 'auth') {
    i.append(svg('path', { d: 'M14 0 L26 5 V14 Q26 24 14 29 Q2 24 2 14 V5 Z M8 13 L12 18 L21 9' }));
  } else {
    i.append(svg('rect', { x: 0, y: 0, width: 28, height: 26, rx: 4 }), svg('path', { d: 'M6 7 H22 M6 13 H22 M6 19 H16' }));
  }
  group.append(i);
}
export async function mountDiagram(root, graph, { technical, onSelect, selected = null }) {
  const generation = (generations.get(root) || 0) + 1; generations.set(root, generation);
  root.setAttribute('aria-busy', 'true');
  if (!graph.nodes?.length) {
    root.replaceChildren(Object.assign(document.createElement('p'), { textContent: '構成要素を特定できませんでした。「図の根拠と読み取れなかった範囲」で不足を確認できます。全体への疑問は記録できます。' }));
    root.setAttribute('aria-busy', 'false'); return;
  }
  try {
    if (!globalThis.ELK) throw Error('図の配置処理を読み込めませんでした。ページを読み直してください。');
    if (!layouts.has(graph)) layouts.set(graph, new globalThis.ELK().layout(structure(graph)));
    const layout = await layouts.get(graph);
    if (generations.get(root) !== generation) return;
    const chart = svg('svg', { viewBox: `0 0 ${layout.width} ${layout.height}`, role: 'group', 'aria-label': `${graph.repository}のシステム構成図`, 'data-layout-width': layout.width });
    const defs = svg('defs'), marker = svg('marker', { id: 'repository-arrow', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' });
    marker.append(svg('path', { d: 'M 0 0 L 10 5 L 0 10 z', fill: '#455e75' })); defs.append(marker); chart.append(defs);
    const positions = new Map([['system', { x: 0, y: 0 }]]);
    function groups(parent, dx = 0, dy = 0) {
      for (const child of parent.children || []) {
        const x = dx + child.x, y = dy + child.y;
        positions.set(child.id, { x, y });
        if (child.children) {
          const definition = graph.boundaries.find(g => g.id === child.id);
          chart.append(svg('rect', { x, y, width: child.width, height: child.height, rx: 12, class: `map-boundary map-boundary-${child.id}` }), svg('text', { x: x + 18, y: y + 28, class: 'map-boundary-title' }, definition?.title || child.id));
          groups(child, x, y);
        } else positions.set(child.id, { x, y });
      }
    }
    groups(layout);
    const activate = (g, object, objectType) => {
      const select = () => { selectDiagramObject(root, object.id); onSelect({ ...object, object_type: objectType }); };
      g.addEventListener('click', select);
      g.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(); } });
    };
    const edgeLayouts = [];
    function edges(parent, dx = 0, dy = 0) {
      for (const edge of parent.edges || []) edgeLayouts.push({ edge, dx, dy });
      for (const child of parent.children || []) if (child.children) edges(child, dx + child.x, dy + child.y);
    }
    edges(layout);
    for (const { edge: placed, dx, dy } of edgeLayouts) {
      const edge = graph.edges.find(e => e.id === placed.id); if (!edge) continue;
      const origin = positions.get(placed.container) || { x: dx, y: dy };
      const from = graph.nodes.find(n => n.id === edge.from), to = graph.nodes.find(n => n.id === edge.to);
      const g = svg('g', { role: 'button', tabindex: 0, 'aria-label': `${from.title}から${to.title}：${edge.title}`, 'aria-pressed': String(edge.id === selected), class: 'map-connection', 'data-object': edge.id, 'data-edge': edge.id, transform: `translate(${origin.x},${origin.y})` });
      for (const section of placed.sections || []) {
        const points = [section.startPoint, ...(section.bendPoints || []), section.endPoint];
        const d = points.map((p, i) => `${i ? 'L' : 'M'}${p.x},${p.y}`).join(' ');
        g.append(svg('path', { d, class: 'map-edge-hit' }), svg('path', { d, class: `map-edge ${(edge.evidence_kind === 'inference' || ['startup', 'configuration'].includes(edge.relation_type)) ? 'map-edge-startup' : ''}`, 'marker-end': 'url(#repository-arrow)' }));
      }
      for (const label of placed.labels || []) {
        g.append(svg('rect', { x: label.x - 5, y: label.y, width: label.width + 10, height: label.height, rx: 4, class: 'map-edge-label-background' }));
        wrap(edge.title, 170, 14).forEach((line, i) => g.append(svg('text', { x: label.x + label.width / 2, y: label.y + 19 + i * 20, 'text-anchor': 'middle', class: 'map-edge-label' }, line)));
      }
      g.append(svg('title', {}, `${edge.title}。${edge.description}`)); activate(g, edge, 'relation'); chart.append(g);
    }
    for (const node of graph.nodes) {
      const position = positions.get(node.id); if (!position) continue;
      const g = svg('g', { transform: `translate(${position.x},${position.y})`, role: 'button', tabindex: 0, 'aria-label': objectTitle(node, technical), 'aria-pressed': String(node.id === selected), class: `map-node map-role-${node.role}`, 'data-object': node.id, 'data-node': node.id });
      g.append(svg('rect', { width: WIDTH, height: HEIGHT, rx: 10 })); icon(g, node.role);
      const clipped = (text, width, size, count) => { const lines = wrap(text, width, size); return lines.slice(0, count).map((line, i) => i === count - 1 && lines.length > count ? line.slice(0, -1) + '…' : line); };
      clipped(node.title, WIDTH - 66, 16, 2).forEach((line, i) => g.append(svg('text', { x: 54, y: 34 + i * 21, class: 'map-node-title' }, line)));
      const descriptions = { ui: '入力と結果の表示', api: '要求を受け付けて処理する', database: '記録を残し、取り出す', storage: 'ファイルを残し、取り出す', cache: '一時的なデータを扱う', auth: '利用者の本人確認', external: '別のサービスへ接続', application: 'アプリを起動して処理する', queue: '別の処理へ仕事を渡す' };
      const subtitle = node.description || descriptions[node.role] || '';
      clipped(subtitle, WIDTH - 28, 13, 2).forEach((line, i) => g.append(svg('text', { x: 14, y: 82 + i * 17, class: 'map-node-subtitle' }, line)));
      const footer = technical ? (node.technology?.join(' / ') || '技術名：未特定') : node.evidence_kind === 'inference' ? 'AIの推定・直接の根拠は未確認' : '選ぶと説明と根拠を表示';
      clipped(footer, WIDTH - 28, 12, 1).forEach(line => g.append(svg('text', { x: 14, y: 129, class: 'map-node-subtitle' }, line)));
      g.append(svg('title', {}, `${objectTitle(node, technical)}。${node.description}`)); activate(g, node, 'element'); chart.append(g);
    }
    root.replaceChildren(chart);
    const fitWidth = Math.min(root.clientWidth - 20, (root.clientHeight - 20) * layout.width / layout.height);
    chart.dataset.fitWidth = String(fitWidth); chart.dataset.zoom = '1'; chart.style.width = `${fitWidth}px`;
  } catch (e) {
    if (generations.get(root) === generation) root.replaceChildren(Object.assign(document.createElement('p'), { className: 'input-error', textContent: e.message || '構成図を配置できませんでした。' }));
    throw e;
  } finally { if (generations.get(root) === generation) root.setAttribute('aria-busy', 'false'); }
}
