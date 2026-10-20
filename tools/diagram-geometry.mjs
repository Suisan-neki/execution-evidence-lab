// 配置計算の成功だけでなく、実画面の線の両端が対応する箱に接していることを確かめる。
import assert from 'node:assert/strict';
export async function verifyConnections(page, graph) {
  const errors = await page.evaluate(edges => {
    const errors = [];
    for (const edge of edges) {
      const group = document.querySelector(`[data-edge="${edge.id}"]`), paths = group?.querySelectorAll('.map-edge-hit');
      if (!paths?.length) { errors.push(edge.id + ': missing line'); continue; }
      for (const [side, object, path] of [['from', edge.from, paths[0]], ['to', edge.to, paths[paths.length - 1]]]) {
        const box = document.querySelector(`[data-node="${object}"] > rect`)?.getBoundingClientRect();
        if (!box) { errors.push(edge.id + ': missing node'); continue; }
        const point = path.getPointAtLength(side === 'from' ? 0 : path.getTotalLength());
        const p = new DOMPoint(point.x, point.y).matrixTransform(path.getScreenCTM()), epsilon = 4;
        const within = p.x >= box.left - epsilon && p.x <= box.right + epsilon && p.y >= box.top - epsilon && p.y <= box.bottom + epsilon;
        const border = Math.min(Math.abs(p.x - box.left), Math.abs(p.x - box.right), Math.abs(p.y - box.top), Math.abs(p.y - box.bottom)) <= epsilon;
        if (!within || !border) errors.push(`${edge.id}: ${side} is detached`);
      }
    }
    return errors;
  }, graph.edges);
  assert.deepEqual(errors, [], '線の両端を正しい構成要素へ接続する');
}
