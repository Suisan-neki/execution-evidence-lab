// 一つのプロセスが、条件に対応する観測を試行の識別子とともに出力する。
import { appendFileSync } from 'node:fs';
import { relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
export function createObserver() {
  const variables = ['EEL_RUN_ID','EEL_RECORD_ID','EEL_ATTEMPT_ID','EEL_OBSERVATIONS','EEL_SOURCE_ROOT'];
  if (variables.some(name => !process.env[name])) throw Error('mugiurushiから試行を起動してください。');
  const correlation = { run_id:process.env.EEL_RUN_ID, record_id:process.env.EEL_RECORD_ID, attempt_id:process.env.EEL_ATTEMPT_ID };
  let sequence = 0;
  return function observe(assertion, subject, outcome, detail) {
    if (![assertion,subject,detail].every(value => typeof value === 'string' && value.trim()) || !['pass','fail'].includes(outcome)) throw Error('観測名・対象・結果・説明を確認してください。');
    // 呼び出し元の場所を記録する。独立したトレースや到達の証明ではない。
    const caller = new Error().stack.split('\n')[2];
    const match = /(file:\/\/[^\s)]+):(\d+):(\d+)/.exec(caller);
    if (!match) throw Error('観測の呼び出し元を取得できません。ES Modulesのコードで使用してください。');
    const path = relative(process.env.EEL_SOURCE_ROOT,fileURLToPath(match[1])).split('\\').join('/');
    if (isAbsolute(path) || path.split('/').includes('..')) throw Error('固定版の外から呼ばれた観測です。');
    const event = { schema_version:1, sequence:++sequence, correlation, assertion, subject, outcome, detail, path, line:Number(match[2]) };
    appendFileSync(process.env.EEL_OBSERVATIONS,JSON.stringify(event)+'\n');
    return event;
  };
}
