// 合成データの保存・読み出しと、失敗・観測不足の接続を試す。
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { createObserver } from '../../adapters/observation/observe.mjs';
const scenario = process.argv[2] || 'normal';
const input = JSON.parse(readFileSync(process.env.EEL_INPUT, 'utf8'));
const emit = createObserver();
function observe(outcome, detail, overrides = {}) {
  const event = emit('stored_matches_input','record-1',outcome,detail);
  if (Object.keys(overrides).length) appendFileSync(process.env.EEL_OBSERVATIONS,JSON.stringify({ ...event, sequence:event.sequence+1, ...overrides })+'\n');
}
if (scenario === 'timeout') { await new Promise(resolve => setTimeout(resolve, 60000)); }
if (scenario === 'exit-zero') { process.exit(0); }
if (scenario === 'failure') { observe('fail', '実験条件で保存前に中止した'); }
else {
  const destination = join(process.env.EEL_WORK_DIR, 'stored.json');
  writeFileSync(destination, JSON.stringify(input));
  const matches = JSON.stringify(JSON.parse(readFileSync(destination, 'utf8'))) === JSON.stringify(input);
  if (scenario !== 'missing') observe(matches ? 'pass' : 'fail', '試行用の保存物を読み出して入力と照合した');
  if (scenario === 'conflict') observe('fail', '同じ対象について食い違う人工的な観測');
  if (scenario === 'foreign') observe('pass', '別の試行の観測', { correlation: { run_id:'other-run',record_id:process.env.EEL_RECORD_ID,attempt_id:process.env.EEL_ATTEMPT_ID } });
  if (scenario === 'mutate') { writeFileSync(new URL('./extra.mjs', import.meta.url), 'export const modified = true;'); }
}

if (scenario === 'bad-line') observe('pass', '存在しない行', { line: 999999 });
if (scenario === 'sequence-gap') observe('pass', '欠番の観測', { sequence: 99 });
if (scenario === 'too-much-output') { console.log('x'.repeat(2 * 1024 * 1024)); }
if (scenario === 'env-check') observe(process.env.EEL_TEST_SECRET ? 'fail' : 'pass', '親の秘密用環境変数を継承していないことを確認');
