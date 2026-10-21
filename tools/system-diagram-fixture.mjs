// 自動試験だけで使う合成のAI応答。実モデルの品質確認とは区別する。
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
export async function startSyntheticArchitectureAI() {
  const graph = JSON.parse(await readFile(new URL('../tests/fixtures/reader/system-ai.json', import.meta.url), 'utf8'));
  const state = { calls: 0, questions: 0, mode: 'valid', revision: 1, sent: [] };
  const server = createServer(async (req, res) => {
    let bytes = ''; for await (const chunk of req) bytes += chunk;
    const body = JSON.parse(bytes); state.sent.push(body);
    if (state.mode === 'unavailable') { res.writeHead(503); res.end('synthetic outage'); return; }
    if (body.messages[0].content.includes('EEL_CONNECTION_TEST')) {
      res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify({ model: 'synthetic-architecture', choices: [{ message: { content: '{"connected":true}' } }] })); return;
    }
    const question = body.messages[0].content.includes('test_candidates');
    if (question) state.questions++; else state.calls++;
    const result = structuredClone(graph);
    if (state.revision > 1) result.summary.workflow += ' これは再生成した説明です。';
    if (body.messages[1].content.includes('repository: example/other')) {
      result.nodes.push({ ...result.nodes[2], id: 'worker', label: '添付ファイルの変換処理', role: 'application', path: 'compose.yaml', refs: [{ path: 'compose.yaml', line: 1, end_line: 1 }] });
      result.edges.push({ ...result.edges[0], from: 'worker', to: 'records_api', label: 'APIの起動を待つ', relation_type: 'startup', evidence: 'code', refs: [{ path: 'compose.yaml', line: 1, end_line: 1 }] });
    }
    if (state.mode === 'invalid' || state.mode === 'repair' && state.calls === 1) result.nodes[1].refs = [{ path: 'unseen.py', line: 1, end_line: 1 }];
    const response = question ? { answer: '保存の失敗を利用者が確認できるか、期待する動作と実際の結果を分けて確かめます。', claims: [{ text: '記録管理のAPIが定義されています。', refs: [{ path: 'server/main.py', line: 3, end_line: 3 }] }], unknowns: ['通信が途切れたときの表示は未検証です。'], test_candidates: [] } : result;
    res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify({ model: 'synthetic-architecture', choices: [{ message: { content: JSON.stringify(response) } }] }));
  });
  await new Promise(accept => server.listen(0, '127.0.0.1', accept));
  return { url: `http://127.0.0.1:${server.address().port}/v1`, state, close: () => new Promise(accept => server.close(accept)) };
}
