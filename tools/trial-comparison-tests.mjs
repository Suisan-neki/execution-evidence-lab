import assert from 'node:assert/strict';
import { compareTrials, sameValue } from '../ui/trial-comparison.js';
const source = { repository: 'example/store', commit: 'a'.repeat(40), path: 'app.mjs', line: 1, end_line: 5 };
function evidence(id, verdict = 'satisfied') {
  return { id, check_id: 'stored', check_version: 1, rule: 'demo_save', source: null, observation: null,
    dependencies: {}, dependency_origins: {}, execution: null, adapter: null,
    manifest: { correlation: { run_id: 'run-' + id }, previous_run: null, started_unix_ms: id.length,
      condition: { id: 'stored', version: 1, text: '保存する', origin: '合成の条件' },
      source_fingerprint: 'engine', program_version: '0.1.0', mode: 'demo', command: [], input: { name: '架空', age: 30 },
      fault: 'normal', os: 'linux', architecture: 'x86_64', timeout_ms: 5000, initial_storage: 'new work', observation_scope: 'synthetic' },
    result: { id, run_id: 'run-' + id, observed: { verdict, reason: '観測の根拠', evidence: ['events.jsonl#1'], unconfirmed: ['他の条件'] }, applicable: true, invalidated_by: [] } };
}
function state(...records) {
  return { workspace: { id: 'synthetic', revision: 3, evidence: records },
    checks: [{ check_id: 'stored', evidence: records.filter(e => e.check_id === 'stored').map(e => e.result) }] };
}
const a = evidence('old', 'not_satisfied'), b = evidence('new');
let s = state(a, b), saved = JSON.stringify(s), r = compareTrials(s, 'stored', 'old', 'new');
assert.equal(r.changed_count, 0);assert.equal(r.rows.find(x => x.key === 'reference').status, 'unknown');
assert.equal(r.before.observed.verdict, 'not_satisfied');assert.equal(r.after.observed.verdict, 'satisfied');
assert.equal(JSON.stringify(s), saved);
assert.equal(sameValue({ z: [1, { a: 2 }], a: 3 }, { a: 3, z: [1, { a: 2 }] }), true);
assert.equal(sameValue([1, 2], [2, 1]), false);
assert.equal(sameValue({ a: null }, {}), false);
assert.throws(() => compareTrials(s, 'stored', 'old', 'old'), /異なる試行/);
assert.throws(() => compareTrials(s, 'other', 'old', 'new'), /確認事項/);
const other = evidence('other');other.check_id = 'other';
assert.throws(() => compareTrials(state(a, b, other), 'stored', 'old', 'other'), /同じ確認事項/);
const duplicate = evidence('duplicate');duplicate.manifest.correlation.run_id = a.manifest.correlation.run_id;
assert.throws(() => compareTrials(state(a, duplicate), 'stored', 'old', 'duplicate'), /異なる試行/);
// 旧記録にコード参照がないとき、両方の欠落を変更なしとしない。
const missing = structuredClone(b);delete missing.manifest.os;delete missing.manifest.input;missing.manifest.source_fingerprint = '';
r = compareTrials(state(a, missing), 'stored', 'old', 'new');
for (const key of ['manifest.os', 'manifest.input', 'manifest.source_fingerprint']) assert.equal(r.rows.find(x => x.key === key).status, 'unknown');
const nullInput = structuredClone(a);nullInput.manifest.input = null;
const nullAfter = structuredClone(b);nullAfter.manifest.input = null;
assert.equal(compareTrials(state(nullInput, nullAfter), 'stored', 'old', 'new').rows.find(x => x.key === 'manifest.input').status, 'same');
// 実行版・入力・引数・観測・環境を同時に変えても、一つの差へまとめない。
function pinned(record) {
  const e = structuredClone(record);e.rule = 'observed_assertion';e.source = source;e.observation = { assertion: 'matches', subject: 'one' };
  e.execution = { recipe: { source, runtime: 'node', entrypoint: 'app.mjs', args: [], input: { b: 2, a: 1 }, criterion: e.observation, timeout_ms: 5000 },
    tree: 'tree', runtime_path: '/usr/bin/node', runtime_version: 'v26', mode: 'local', snapshot_unchanged: true, exit_code: 0, timed_out: false,
    output_limit: false, diagnostics: [], observation_read: 'valid' };
  e.dependencies = { 'execution-stored': '{"a":1,"b":2}' };e.dependency_origins = { 'execution-stored': '実行記録' };return e;
}
const x = pinned(a), y = pinned(b);y.dependencies['execution-stored'] = '{"b":2,"a":1}';y.execution.recipe.input = { a: 1, b: 2 };
r = compareTrials(state(x, y), 'stored', 'old', 'new');assert.equal(r.changed_count, 0);
y.source = { ...source, commit: 'b'.repeat(40) };y.execution.recipe.source = y.source;y.execution.recipe.args = ['normal'];
y.execution.recipe.input = { a: 9 };y.execution.runtime_version = 'v27';y.execution.recipe.criterion = { assertion: 'matches', subject: 'two' };
y.observation = y.execution.recipe.criterion;y.check_version = 2;y.manifest.condition.version = 2;
x.result.applicable = false;x.result.invalidated_by = ['コードと入力が変わった'];
r = compareTrials(state(x, y), 'stored', 'old', 'new');
for (const key of ['reference', 'recipe.source', 'recipe.args', 'recipe.input', 'recipe.criterion', 'execution.runtime_version', 'check', 'condition', 'observation'])
  assert.equal(r.rows.find(x => x.key === key).status, 'changed', key);
assert.equal(r.before.applicable, false);assert.equal(r.before.observed.verdict, 'not_satisfied');
assert.equal(compareTrials(state(x, y), 'stored', 'new', 'old').before.evidence_id, 'new');
const oldFormat = structuredClone(y);oldFormat.execution = null;
assert.equal(compareTrials(state(x, oldFormat), 'stored', 'old', 'new').rows.find(x => x.key === 'recipe.input').status, 'unknown');
delete y.dependencies['execution-stored'];y.dependencies.authorization = 'declared value';
r = compareTrials(state(x, y), 'stored', 'old', 'new');
assert.equal(r.rows.find(x => x.key === 'dependency_keys').status, 'changed');
assert.equal(r.rows.find(x => x.key === 'dependency.execution-stored').status, 'unknown');
assert.equal(r.rows.find(x => x.key === 'dependency.authorization').status, 'unknown');
// 対象版・故障条件は比較するが、試行ごとの識別子だけを構成変更としない。
const ad = evidence('old'), bd = evidence('new');
const target = { repository: 'example/project', revision: 'a'.repeat(40), expectedBlobs: { 'app.js': 'blob' }, adapterSha256: 'sha', node: 'v26', fault: 'normal', scope: 'local' };
ad.adapter = { target: { ...target, correlation: { run_id: 'old' } } };bd.adapter = { target: { ...target, correlation: { run_id: 'new' } } };
assert.equal(compareTrials(state(ad, bd), 'stored', 'old', 'new').changed_count, 0);
bd.adapter.target.fault = 'failure';assert.equal(compareTrials(state(ad, bd), 'stored', 'old', 'new').rows.find(x => x.key === 'adapter.fault').status, 'changed');
console.log('trial comparison: scoped pairs, observed versus applicable, missing records, nullable input, unordered objects, multiple changes, adapters and immutable state passed');
