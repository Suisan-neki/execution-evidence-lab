// 静的なコードの読解案内。実行順序・業務上の意味・呼び出し先を保証しない。
import { maskCode, codeLink, repositoryAddress, validateSnapshot } from './scanner.js';
import { callTargets } from './call-navigation.js';

export function sourceReference(snapshot, path, line = 1, endLine = line) {
  return validateCodeReference({ repository: snapshot.fullName, commit: snapshot.commit, path, line, end_line: endLine });
}
export function validateCodeReference(source) {
  const address = repositoryAddress('https://github.com/' + source?.repository);
  if (address.fullName !== source.repository || !/^[a-f0-9]{40}$/.test(source.commit)
    || typeof source.path !== 'string' || !source.path || source.path.length > 1024
    || source.path.split('/').some(p => !p || p === '.' || p === '..') || /[\\\x00-\x1f\x7f]/.test(source.path)
    || !Number.isInteger(source.line) || source.line < 1 || !Number.isInteger(source.end_line)
    || source.end_line < source.line || source.end_line > 1000000) throw Error('固定したコード参照を確認してください。');
  return source;
}
export function sourceURL(source) {
  validateCodeReference(source);
  return codeLink({ fullName: source.repository, commit: source.commit }, source.path, source.line);
}
export function referenceFromURL(value) {
  const url = new URL(value);
  const match = /^\/([^/]+\/[^/]+)\/blob\/([a-f0-9]{40})\/(.+)$/.exec(url.pathname);
  if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.port || url.username || url.password || url.search || !match) throw Error('固定コミットのGitHubコードへのURLが必要です。');
  const line = /^#L(\d+)(?:-L(\d+))?$/.exec(url.hash);
  if (!line) throw Error('コードの行を指定してください。');
  return validateCodeReference({ repository: match[1], commit: match[2], path: decodeURIComponent(match[3]), line: Number(line[1]), end_line: Number(line[2] || line[1]) });
}
export function readerURL(source, checkId = '') {
  validateCodeReference(source);
  return '/#source=' + encodeURIComponent(JSON.stringify({ source, check_id: checkId }));
}
export function cacheSnapshot(snapshot, storage) {
  const bytes = JSON.stringify(snapshot);
  if (new TextEncoder().encode(bytes).length > 8 * 1024 * 1024) return false;
  try { storage.setItem('evidence-reader-current', bytes); return true; } catch { return false; }
}
export function cachedSnapshot(source, storage) {
  try {
    const snapshot = validateSnapshot(JSON.parse(storage.getItem('evidence-reader-current')));
    return snapshot.fullName === source.repository && snapshot.commit === source.commit ? snapshot : null;
  } catch { return null; }
}

export function functionRange(file, symbol, masked = maskCode(file.text, /\.rs$/.test(file.path), /\.py$/.test(file.path))) {
  const lines = masked.split('\n');
  if (/\.py$/.test(file.path)) {
    const indent = /^\s*/.exec(lines[symbol.line - 1])[0].length;
    let end = symbol.line;
    for (let i = symbol.line; i < lines.length; i++) {
      if (!lines[i].trim()) continue;
      if (/^\s*/.exec(lines[i])[0].length <= indent) break;
      end = i + 1;
    }
    return { start: symbol.line, end };
  }
  const offset = lines.slice(0, symbol.line - 1).join('\n').length + (symbol.line > 1 ? 1 : 0);
  let brace = -1, parentheses = 0;
  for (let i = offset; i < masked.length; i++) {
    if (masked[i] === '(') parentheses++;
    if (masked[i] === ')') parentheses--;
    if (masked[i] === '{' && parentheses === 0) { brace = i; break; }
  }
  // 多行宣言を許すが、未対応の宣言から後の別関数まで範囲を広げない。
  const boundary = file.symbols.find(s => s.line > symbol.line)?.line || lines.length + 1;
  if (brace < 0 || masked.slice(0, brace).split('\n').length >= boundary)
    return { start: symbol.line, end: symbol.line };
  let depth = 0;
  for (let i = brace; i < masked.length; i++) {
    if (masked[i] === '{') depth++;
    if (masked[i] === '}' && --depth === 0) return { start: symbol.line, end: masked.slice(0, i).split('\n').length, header_end: brace };
  }
  return { start: symbol.line, end: symbol.line };
}
const guideCaches = new WeakMap();
const RESERVED = new Set(['if', 'for', 'while', 'switch', 'catch', 'match', 'fn', 'function', 'def', 'return', 'sizeof', 'Some', 'Ok', 'Err']);
export function explainFunction(model, path, selectedLine) {
  const file = model.files.find(f => f.path === path);
  if (!file) return null;
  let cache = guideCaches.get(model);
  if (!cache) { cache = new Map(); guideCaches.set(model, cache); }
  let analyzed = cache.get(file.path);
  if (!analyzed) {
    const masked = maskCode(file.text, /\.rs$/.test(path), /\.py$/.test(path));
    analyzed = { ranges: file.symbols.map(s => ({ symbol: s, range: functionRange(file, s, masked) })),
      lines: masked.split('\n'), original: file.text.split('\n'), guides: new Map() };
    cache.set(file.path, analyzed);
  }
  const ranges = analyzed.ranges;
  const enclosing = ranges.filter(s => s.range.start <= selectedLine && selectedLine <= s.range.end).at(-1);
  if (!enclosing) return null;
  const { symbol, range } = enclosing;
  if (analyzed.guides.has(symbol.line)) return analyzed.guides.get(symbol.line);
  const original = analyzed.original;
  const lines = analyzed.lines;
  const steps = [], calls = [];
  // 表示順はコードの行順。分岐や非同期処理の実行順序ではない。
  for (let i = range.start - 1; i < range.end; i++) {
    const line = lines[i], number = i + 1;
    if (ranges.some(other => other.range.start > range.start && other.range.start <= number && number <= other.range.end)) continue;
    if (/\b(if|else|match|switch)\b/.test(line)) steps.push({ kind: 'branch', line: number, text: '条件に応じて処理を分ける記述があります。条件式と各分岐を確認してください。' });
    if (/\b(for|while|loop)\b/.test(line)) steps.push({ kind: 'loop', line: number, text: '処理を繰り返す記述があります。繰り返す対象と終了条件を確認してください。' });
    if (/\b(throw|raise|panic)\b|\b(?:panic|bail)!/.test(line)) steps.push({ kind: 'error', line: number, text: '例外やエラーを発生させる記述があります。呼び出し元の扱いを確認してください。' });
    if (/\breturn\b/.test(line)) steps.push({ kind: 'return', line: number, text: 'この位置で呼び出し元へ戻る記述があります。返す値をコードで確認してください。' });
    if (/\bawait\b|\.await\b/.test(line)) steps.push({ kind: 'wait', line: number, text: '非同期処理の完了を待つ記述があります。待機後の成功・失敗の扱いを確認してください。' });
    if (/\.rs$/.test(path) && /\?/.test(line)) steps.push({ kind: 'propagate', line: number, text: '?演算子があります。ResultやOptionの値に応じて呼び出し元へ戻る可能性があります。' });
    for (const match of line.matchAll(/\b([A-Za-z_$][\w$]*(?:(?:::|\.)[A-Za-z_$][\w$]*)*)\s*(!)?\s*\(/g)) {
      const name = match[1];
      const preceding = line.slice(0, match.index);
      if (RESERVED.has(name) || /(?:fn|function|def)\s*$/.test(preceding)) continue;
      const targets = callTargets(model, file, name, range);
      calls.push({ name, line: number, macro: Boolean(match[2]), ...targets });
    }
  }
  const startOffset = original.slice(0, range.start - 1).join('\n').length + (range.start > 1 ? 1 : 0);
  const signature = range.header_end !== undefined ? file.text.slice(startOffset, range.header_end).trim() : original[range.start - 1].trim();
  const result = { name: symbol.name, range, signature, steps: steps.slice(0, 80), calls: calls.slice(0, 80), truncated: steps.length > 80 || calls.length > 80,
    limits: 'コードの記述から作った読解案内です。呼び出し先は名前と参照宣言からの候補で、実際の到達・実行順序・通信や保存の成否は未確認です。動的呼び出しや未対応構文は取りこぼす場合があります。' };
  analyzed.guides.set(symbol.line, result);
  return result;
}

// 読取済みの関数だけを調べる。未読・未対応の呼び出しをないものにしない。
export function callersOf(model, path, declarationLine, maxFunctions = 400, maxCallers = 80) {
  const callers = [], seen = new Set(); let scanned = 0, limited = false;
  outer: for (const file of model.files) for (const symbol of file.symbols) {
    if (scanned >= maxFunctions) { limited = true; break outer; }
    scanned++;
    const guide = explainFunction(model, file.path, symbol.line);
    if (!guide) continue;
    if (guide.truncated) limited = true;
    for (const call of guide.calls) if (call.candidates.some(c => c.path === path && c.line === declarationLine)) {
      const key = file.path + ':' + call.line + ':' + call.name;
      if (seen.has(key)) continue;
      seen.add(key);
      if (callers.length >= maxCallers) { limited = true; continue; }
      callers.push({ path: file.path, line: call.line, function: guide.name, function_line: guide.range.start,
        call: call.name, bindings: call.bindings, reason: call.reason });
    }
  }
  return { callers, scanned_functions: scanned, limited, inventory_truncated: model.inventory_truncated === true, max_functions: maxFunctions, max_callers: maxCallers,
    unread_files: (model.inventory || []).filter(f => /\.(rs|[cm]?[jt]sx?|py)$/.test(f.path) && !model.files.some(read => read.path === f.path)).length };
}

export function compareSnapshots(before, after) {
  validateSnapshot(before); validateSnapshot(after);
  if (before.fullName !== after.fullName) throw Error('同じリポジトリの二つの版を選んでください。');
  const left = new Map(before.inventory.map(f => [f.path, f])), right = new Map(after.inventory.map(f => [f.path, f]));
  const entries = [...new Set([...left.keys(), ...right.keys()])].sort().map(path => {
    const a = left.get(path), b = right.get(path);
    let status = 'unknown', basis = '両方のコードを読み取っていません。';
    if (!a) { status = before.truncated ? 'unknown' : 'added'; basis = before.truncated ? '以前の一覧が途中で切れており、追加と断定できません。' : '完全な以前の一覧にパスがありません。'; }
    else if (!b) { status = after.truncated ? 'unknown' : 'removed'; basis = after.truncated ? '新しい一覧が途中で切れており、削除と断定できません。' : '完全な新しい一覧にパスがありません。'; }
    else if (a.mode !== b.mode) { status = 'changed'; basis = 'ファイルの種別・実行権限が異なります。'; }
    else if (/^[a-f0-9]{40}$/.test(a.sha) && /^[a-f0-9]{40}$/.test(b.sha)) { status = a.sha === b.sha ? 'unchanged' : 'changed'; basis = 'GitHubの一覧に含まれるGit blobの識別値を比較しました。'; }
    else if (typeof before.files[path] === 'string' && typeof after.files[path] === 'string') { status = before.files[path] === after.files[path] ? 'unchanged' : 'changed'; basis = '読取記録に含まれるコード本文を比較しました。'; }
    return { path, status, basis };
  });
  return { repository: before.fullName, before: before.commit, after: after.commit, entries,
    limited: before.truncated || after.truncated,
    limits: 'パス・blob・読取本文の比較です。変更の業務上の意味、未宣言の依存先、実行環境への影響は判定していません。保存した記録の真正性は保証しません。' };
}
export function lineChanges(before, after, limit = 120) {
  if (typeof before !== 'string' || typeof after !== 'string') return null;
  const a = before.split('\n'), b = after.split('\n');
  let prefix = 0, suffix = 0;
  while (prefix < a.length && prefix < b.length && a[prefix] === b[prefix]) prefix++;
  while (suffix < a.length - prefix && suffix < b.length - prefix && a[a.length - 1 - suffix] === b[b.length - 1 - suffix]) suffix++;
  return { before: a.slice(prefix, a.length - suffix).slice(0, limit).map((text, i) => ({ line: prefix + i + 1, text })),
    after: b.slice(prefix, b.length - suffix).slice(0, limit).map((text, i) => ({ line: prefix + i + 1, text })),
    truncated: a.length - suffix - prefix > limit || b.length - suffix - prefix > limit };
}
