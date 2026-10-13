// 取り込まれた記録の比較。判定はサーバーのEvidenceViewを使い、ここでは再計算しない。
export function sameValue(a, b) {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== typeof b || typeof a !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) return a.length === b.length && a.every((v, i) => sameValue(v, b[i]));
  const keys = Object.keys(a).sort(), other = Object.keys(b).sort();
  return keys.length === other.length && keys.every((k, i) => k === other[i] && sameValue(a[k], b[k]));
}

export function compareTrials(state, checkId, beforeId, afterId) {
  const w = state.workspace, check = state.checks.find(c => c.check_id === checkId);
  if (!check) throw Error('比較する確認事項がありません。');
  function trial(id) {
    const record = w.evidence.find(e => e.id === id && e.check_id === checkId);
    const result = check.evidence.find(e => e.id === id);
    if (!record || !result) throw Error('同じ確認事項へ取り込んだ試行を選んでください。');
    return { record, result };
  }
  const a = trial(beforeId), b = trial(afterId);
  if (beforeId === afterId || a.record.manifest.correlation.run_id === b.record.manifest.correlation.run_id)
    throw Error('二つの異なる試行を選んでください。');
  const rows = [];
  function add(key, label, before, after, { nullable = false, parse = false } = {}) {
    // nullが意味を持つ欄（故障指定等）と、記録がない欄（コード参照等）を分ける。
    const read = value => {
      if (value === undefined || (!nullable && value === null) || value === '') return { value: null, recorded: false };
      if (parse) {
        try { return { value: JSON.parse(value), recorded: true }; }
        catch { return { value: null, recorded: false }; }
      }
      return { value, recorded: true };
    };
    const a = read(before), b = read(after);
    rows.push({ key, label, before: a.value, after: b.value, before_recorded: a.recorded, after_recorded: b.recorded,
      status: !a.recorded || !b.recorded ? 'unknown' : sameValue(a.value, b.value) ? 'same' : 'changed' });
  }
  const old = a.record, next = b.record, m = old.manifest, n = next.manifest;
  add('check', '取り込み時の条件版・判定規則', { version: old.check_version, rule: old.rule }, { version: next.check_version, rule: next.rule });
  add('condition', '試行に記録された条件文・出所', m.condition, n.condition);
  add('reference', '取り込み時の参照コード', old.source, next.source);
  if (old.rule === 'observed_assertion' || next.rule === 'observed_assertion')
    add('observation', '条件へ対応づけた観測名・対象', old.observation, next.observation);
  for (const [key, label, nullable] of [
    ['source_fingerprint', '基盤のコード・依存関係'], ['program_version', '基盤のバージョン'],
    ['mode', '試行の実行方法'], ['command', '起動コマンド'], ['input', '基盤の入力', true],
    ['fault', '基盤の故障指定', true], ['os', 'OS'], ['architecture', 'アーキテクチャ'],
    ['timeout_ms', '基盤の待機上限・ミリ秒'], ['initial_storage', '初期状態の記録'], ['observation_scope', '観測範囲の記録'],
  ]) add('manifest.' + key, label, m[key], n[key], { nullable });
  if (old.execution || next.execution) {
    const x = old.execution, y = next.execution;
    for (const [key, label] of [
      ['source', '実行対象の固定版・コード参照'], ['runtime', '実行言語'], ['entrypoint', '起動ファイル'],
      ['args', '起動引数'], ['input', '外部コードの試験入力'], ['criterion', '試験設定の観測名・対象'],
      ['timeout_ms', '対象プロセスの待機上限・ミリ秒'],
    ]) add('recipe.' + key, label, x?.recipe[key], y?.recipe[key], { nullable: key === 'input' });
    for (const [key, label] of [['tree', '固定版のGit tree'], ['runtime_path', 'ランタイムのパス'], ['runtime_version', 'ランタイムのバージョン'], ['mode', '外部コードの実行方式']])
      add('execution.' + key, label, x?.[key], y?.[key]);
  }
  if (old.adapter || next.adapter) {
    const x = old.adapter?.target, y = next.adapter?.target;
    for (const [key, label] of [['repository', 'アダプターの対象リポジトリ'], ['revision', 'アダプターの固定版'], ['expectedBlobs', 'アダプターの対象コード'], ['adapterSha256', 'アダプターのコード'], ['node', 'アダプターのランタイム'], ['fault', 'アダプターの故障指定'], ['scope', 'アダプターの確認範囲']])
      add('adapter.' + key, label, x?.[key], y?.[key]);
  }
  const keys = [...new Set([...Object.keys(old.dependencies), ...Object.keys(next.dependencies)])].sort();
  add('dependency_keys', '宣言した依存先の一覧', Object.keys(old.dependencies).sort(), Object.keys(next.dependencies).sort());
  for (const key of keys) {
    // 固定版実行の依存値はJSON。同じ値のキー順だけを変更扱いにしない。
    add('dependency.' + key, '依存先：' + key, old.dependencies[key], next.dependencies[key], { parse: key.startsWith('execution-') });
    add('dependency_origin.' + key, '依存先の値の出所：' + key, old.dependency_origins?.[key], next.dependency_origins?.[key]);
  }
  const summary = ({ record, result }) => ({ evidence_id: record.id, run_id: result.run_id,
    check_version: record.check_version, source: record.source ?? null, execution_source: record.execution?.recipe.source ?? null,
    observed: result.observed, applicable: result.applicable, invalidated_by: result.invalidated_by,
    execution_outcome: record.execution ? {
      exit_code: record.execution.exit_code, timed_out: record.execution.timed_out, output_limit: record.execution.output_limit,
      snapshot_unchanged: record.execution.snapshot_unchanged, observation_read: record.execution.observation_read ?? 'unspecified',
      diagnostics: record.execution.diagnostics,
    } : null });
  return { schema_version: 1, case_id: w.id, case_revision: w.revision, check_id: checkId,
    before: summary(a), after: summary(b), rows,
    changed_count: rows.filter(r => r.status === 'changed').length,
    unknown_count: rows.filter(r => r.status === 'unknown').length,
    limits: '取り込み時に保存した記録の比較です。観測時の判定と現在の条件への適用範囲を分けて確認してください。結果の変化だけではコード改修の効果を特定できません。比較する項目が同じでも、記録していない依存先や実環境が同じとは保証しません。' };
}
