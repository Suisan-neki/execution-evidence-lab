// 私有の対象ソースをコピーせず、指定した固定版の実ハンドラーを通して確認する。
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, readdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';

const [backendArgument, revision] = process.argv.slice(2);
if (!backendArgument || !revision || process.argv.length !== 4) throw Error('usage: node tools/adapter-smoke.mjs /absolute/path/to/backend pinned-revision');
const root = resolve(import.meta.dirname, '..');
const binary = process.env.REVIEW_BINARY || join(root, 'target/debug/execution-evidence-lab');
const parent = join(root, 'cases');
await import('node:fs/promises').then(fs => fs.mkdir(parent, { recursive: true }));
const output = await mkdtemp(join(parent, 'adapter-'));
const casePath = join(output, 'case');
const cli = args => execFileSync(binary, args, { encoding: 'utf8' });
let current = JSON.parse(cli(['case', 'init', casePath, '--actor', 'Codex', '--reason', '固定版の限定したローカル実行']));
const apply = async action => {
  const path = join(output, 'request.json');
  await writeFile(path, JSON.stringify({ expected_revision: current.workspace.revision, actor: 'Codex', reason: '合成入力での実ハンドラー確認。医療機関の採用要件ではない', action }));
  current = JSON.parse(cli(['case', 'apply', casePath, path]));
};
await apply({ op: 'add_check', check: { id: 'local-index', version: 1,
  text: 'このローカル試行で、合成S3通知による模擬索引のuploadedへの更新を観測した',
  origin: 'Codexの試験条件。実AWSの確認ではない', rule: 'local_index_uploaded',
  dependencies: ['engine', 'environment', 'target', 'adapter', 'fault', 'command'], questions: [] } });
const results = [];
for (const [fault, expected, stored] of [['normal', 'satisfied', 'uploaded'], ['index-failure', 'not_satisfied', 'issued'], ['missing-observation', 'unknown', 'uploaded']]) {
  const runs = join(output, fault);
  cli(['capture', '--condition', '管理者が保存データを閲覧できる', '--output', runs, '--timeout-ms', '10000', '--', process.execPath, join(root, 'adapters/vitalsensing-local.mjs'), resolve(backendArgument), fault, revision]);
  const names = await readdir(runs);
  assert.equal(names.length, 1);
  const run = join(runs, names[0]);
  const assessment = JSON.parse(await readFile(join(run, 'assessment.json')));
  assert.equal(assessment.verdict, 'unknown');
  assert.equal(JSON.parse(await readFile(join(run, 'adapter-assessment.json'))).verdict, expected);
  assert.equal(JSON.parse(await readFile(join(run, 'adapter-truth.json'))).uploadStatus, stored);
  current = JSON.parse(cli(['case', 'import', casePath, run, '--condition', 'local-index', '--actor', 'Codex', '--reason', '対象版と観測記録を取り込む', '--use-run-context', 'true']));
  assert.equal(current.checks.find(c => c.check_id === 'local-index').verdict, expected);
  assert.equal(current.checks.find(c => c.check_id === 'later-view').verdict, 'unknown');
  results.push({ fault, localVerdict: expected, localReadback: stored, businessVerdict: assessment.verdict, run });
}
cli(['case', 'export', casePath, '--output', join(output, 'comparison')]);
const report = { revision, casePath, results, human_evaluation: 'not_conducted' };
await writeFile(join(output, 'smoke-result.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
