// 利用者の質問や選んだ関数から、読み取ったコードの該当箇所を集める。
// 記述が見つからない場合は「見つからない」と返し、処理を補わない。
import { maskCode } from './scanner.js';
import { explainFunction, callersOf } from './comprehension.js';
import { KIND, ref, effects, functionsOf, enclosing, effectLabel, testsOf } from './overview.js';

export const INTENTS = [
  { id: 'storage', label: '保存・書き込み', words: /保存|保管|書き込|書込|記録|格納|永続|残る|残す|データベース|\bDB\b|save|store|persist|write/i, kinds: ['write', 'delete'] },
  { id: 'failure', label: '失敗したときの扱い', words: /失敗|エラー|できなかった|できない|例外|異常|壊れ|落ち|止ま|error|fail|exception/i, kinds: ['raise', 'catch', 'exit', 'error_output'] },
  { id: 'send', label: '送信・通知', words: /通知|送信|送る|メール|通信|連携|\bAPI\b|notify|send|mail/i, kinds: ['send'] },
  { id: 'input', label: '入力', words: /入力|引数|受け取|オプション|パラメータ|argument|input|option/i, kinds: ['input', 'env'] },
  { id: 'read', label: '読み出し・表示', words: /読み出|読む|読ん|表示|一覧|出力|見る|見え|read|list|show|print/i, kinds: ['read', 'output'] },
  { id: 'start', label: '起動方法', words: /起動|始め|使い方|実行方法|インストール|動かし|start|run|install|usage/i, kinds: [] },
  { id: 'test', label: '既存のテスト', words: /テスト|試験|検証|\btest/i, kinds: [] },
  { id: 'config', label: '設定', words: /設定|環境変数|config|setting/i, kinds: ['env'] },
];

function isTest(path) { return /(^|\/)(tests?|__tests__|spec)(\/|$)/.test(path) || /\.(test|spec)\.[cm]?[jt]sx?$/.test(path) || /(^|\/)test_[^/]+\.py$/.test(path) || /\.t$/.test(path); }
function code(file, line) { return (file.text.split('\n')[line - 1] || '').trim().slice(0, 200); }

/// 質問文の英数字の語（関数名・ファイル名など）と、「」で囲んだ語を取り出す。
export function terms(question) {
  const words = new Set();
  for (const m of question.matchAll(/[A-Za-z_][\w.-]{2,}/g)) if (!/^(the|and|for|with|what|where|how|API|DB)$/i.test(m[0])) words.add(m[0]);
  for (const m of question.matchAll(/[「『"]([^」』"]{1,40})[」』"]/g)) words.add(m[1]);
  return [...words].slice(0, 8);
}

function optionDefinitions(model) {
  const options = [];
  for (const f of model.files.filter(f => /\.py$/.test(f.path) && !isTest(f.path))) {
    const masked = maskCode(f.text, false, true).split('\n');
    const lines = f.text.split('\n');
    masked.forEach((m, i) => {
      if (!/\.(add_option|add_argument)\s*\(/.test(m)) return;
      let text = lines[i], j = i;
      while ((text.match(/\(/g) || []).length > (text.match(/\)/g) || []).length && j < i + 6) text += ' ' + lines[++j].trim();
      const flags = [...text.matchAll(/['"](-{1,2}[\w-]+)['"]/g)].map(x => x[1]);
      const field = name => new RegExp(`${name}\\s*=\\s*(['"])(.*?)\\1`).exec(text)?.[2] ?? new RegExp(`${name}\\s*=\\s*([\\w.]+)`).exec(text)?.[1] ?? null;
      options.push({ flags, dest: field('dest') || flags.find(f => f.startsWith('--'))?.replace(/^--/, '').replaceAll('-', '_') || null,
        default: field('default'), help: field('help'), metavar: field('metavar'), action: field('action'), path: f.path, line: i + 1, refs: [ref(f.path, i + 1, j + 1)] });
    });
  }
  for (const f of model.files.filter(f => /\.[cm]?[jt]sx?$/.test(f.path) && !isTest(f.path))) {
    f.text.split('\n').forEach((l, i) => {
      const m = /\.option\s*\(\s*['"]([^'"]+)['"]\s*(?:,\s*['"]([^'"]*)['"])?/.exec(l);
      if (m) options.push({ flags: m[1].split(/[ ,]+/).filter(x => x.startsWith('-')), dest: null, default: null, help: m[2] || null, metavar: null, action: null, path: f.path, line: i + 1, refs: [ref(f.path, i + 1)] });
    });
  }
  return options;
}
export { optionDefinitions };

function parameters(signature, python) {
  const inner = /\(([\s\S]*)\)/.exec(signature)?.[1] || '';
  let depth = 0, start = 0; const parts = [];
  for (let i = 0; i <= inner.length; i++) {
    const c = inner[i];
    if (i === inner.length || (c === ',' && depth === 0)) { parts.push(inner.slice(start, i).trim()); start = i + 1; }
    else if ('([{<'.includes(c)) depth++; else if (')]}>'.includes(c)) depth--;
  }
  return parts.filter(p => p && !(python && /^(self|cls)$/.test(p)));
}

/// 例外名を捕捉する記述を、読み取った全ファイルから探す。
function handlersOf(model, name) {
  const found = [];
  for (const f of model.files) {
    const masked = maskCode(f.text, /\.rs$/.test(f.path), /\.py$/.test(f.path)).split('\n');
    const functions = functionsOf(f);
    masked.forEach((m, i) => {
      if (new RegExp(`^\\s*except\\b[^:]*\\b${name}\\b`).test(m) || new RegExp(`\\bcatch\\b.*\\b${name}\\b|instanceof\\s+${name}\\b`).test(m)) {
        const fn = enclosing(functions, i + 1);
        const lines = f.text.split('\n');
        const body = [];
        for (let k = i + 1; k < Math.min(lines.length, i + 5); k++) { if (/^\s*(except|finally|else)\b/.test(masked[k])) break; if (lines[k].trim()) body.push({ line: k + 1, code: lines[k].trim() }); }
        found.push({ path: f.path, line: i + 1, function: fn?.name || null, code: code(f, i + 1), body, refs: [ref(f.path, i + 1, body.at(-1)?.line || i + 1)] });
      }
    });
  }
  return found;
}

function testMentions(model, names) {
  const hits = [];
  for (const f of model.files.filter(f => isTest(f.path))) {
    const lines = f.text.split('\n');
    lines.forEach((l, i) => { if (names.some(n => n && n.length > 1 && new RegExp(`(^|[^\\w-])${n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}([^\\w-]|$)`).test(l))) hits.push({ path: f.path, line: i + 1, code: l.trim().slice(0, 200), refs: [ref(f.path, i + 1)] }); });
  }
  return hits.slice(0, 12);
}

/// 選んだ関数の入力・戻り値・分岐・エラー処理・副作用・呼び出し関係・関連設定・既存テスト。
export function functionDetail(model, path, line) {
  const file = model.files.find(f => f.path === path);
  if (!file) return { found: false, reason: `${path} はまだ読み取っていない`, needed_files: [path] };
  const guide = explainFunction(model, path, line);
  if (!guide) return { found: false, reason: `${path} の${line}行目を含む関数を抽出できなかった。対応していない構文の可能性がある`, needed_files: [] };
  const python = /\.py$/.test(path);
  const fn = functionsOf(file).find(f => f.line === guide.range.start);
  const name = fn?.name || guide.name;
  const range = guide.range;
  const fx = effects(file).filter(e => e.line >= range.start && e.line <= range.end);
  const masked = maskCode(file.text, /\.rs$/.test(path), python).split('\n');
  const returns = guide.steps.filter(s => s.kind === 'return').map(s => ({ line: s.line, code: code(file, s.line), refs: [ref(path, s.line)] }));
  const branches = guide.steps.filter(s => s.kind === 'branch' || s.kind === 'loop').map(s => ({ kind: s.kind, line: s.line, code: code(file, s.line), refs: [ref(path, s.line)] }));
  const raised = [];
  for (let i = range.start - 1; i < range.end; i++) {
    const m = /\b(?:raise|throw\s+new|throw)\s+([A-Za-z_$][\w$.]*)/.exec(masked[i]);
    if (m) {
      const exception = m[1].split('.').at(-1);
      const handlers = handlersOf(model, exception);
      raised.push({ exception, line: i + 1, code: code(file, i + 1), refs: [ref(path, i + 1)], handlers,
        handled: handlers.length ? { kind: KIND.inference, text: `${exception} を捕捉する記述が ${handlers.map(h => `${h.function || h.path}（${h.path} ${h.line}行目）`).join('、')} にある。この関数から実際にそこへ戻るかは、呼び出し関係の候補から推測した` } : { kind: KIND.code, text: `読み取った範囲に ${exception} を捕捉する記述は見つからなかった。未読のファイルや、より広い例外の捕捉で扱われる可能性がある` } });
    }
  }
  const caught = fx.filter(e => e.kind === 'catch').map(e => ({ line: e.line, code: e.code, refs: e.refs }));
  const callees = [];
  for (const c of guide.calls) if (!callees.some(x => x.name === c.name)) callees.push({ name: c.name, line: c.line, candidates: c.candidates.map(x => ({ ...x, refs: [ref(x.path, x.line)] })), unread: c.unread, reason: c.reason, refs: [ref(path, c.line)] });
  const callers = callersOf(model, path, range.start);
  const nameCallers = [];
  if (fn?.owner || !callers.callers.length) {
    const short = fn?.short || guide.name;
    for (const f of model.files.filter(f => !isTest(f.path))) {
      const m = maskCode(f.text, /\.rs$/.test(f.path), /\.py$/.test(f.path)).split('\n');
      const functions = functionsOf(f);
      const constructor = fn?.owner && fn.short === '__init__';
      m.forEach((l, i) => {
        if (f.path === path && i + 1 >= range.start && i + 1 <= range.end) return;
        if (/^\s*class\s/.test(l) || /\bsys\.std(out|err)\.|\bconsole\./.test(l)) return;
        const matches = constructor ? new RegExp(`(^|[^\\w.])${fn.owner}\\s*\\(`).test(l) : new RegExp(`\\.${short}\\s*\\(`).test(l);
        if (matches) {
          const caller = enclosing(functions, i + 1);
          nameCallers.push({ path: f.path, line: i + 1, function: caller?.name || null, code: code(f, i + 1), refs: [ref(f.path, i + 1)],
            reason: constructor ? `クラス ${fn.owner} を作る記述。__init__ が呼ばれる候補` : `名前 ${short} だけが一致する呼び出し。受け手の型は確かめていない` });
        }
      });
    }
  }
  const options = optionDefinitions(model);
  const body = file.text.split('\n').slice(range.start - 1, range.end).join('\n');
  const callerText = nameCallers.concat(callers.callers).map(c => model.files.find(f => f.path === c.path)?.text.split('\n')[c.line - 1] || '').join('\n');
  const related = options.filter(o => o.dest && new RegExp(`\\boptions\\.${o.dest}\\b|\\b${o.dest}\\b`).test(body + '\n' + callerText));
  const env = fx.filter(e => e.kind === 'env');
  return {
    found: true, name, path, line: range.start, end_line: range.end,
    signature: guide.signature, parameters: parameters(guide.signature, python),
    returns, branches, raised, caught,
    effects: fx.map(e => ({ kind: e.kind, label: e.label, certainty: e.certainty, line: e.line, code: e.code, refs: e.refs })),
    callees, callers: callers.callers.map(c => ({ ...c, refs: [ref(c.path, c.line)] })), name_callers: nameCallers.slice(0, 20),
    config: { options: related, env },
    tests: testMentions(model, [fn?.short || guide.name, ...related.flatMap(o => o.flags)]),
    limits: guide.limits,
    refs: [ref(path, range.start, range.end)],
  };
}

function unreadRelated(snapshot, model, paths, words) {
  const unread = new Set(snapshot.omitted.filter(o => o.reason === '初回の読取範囲外').map(o => o.path));
  const needed = new Set();
  for (const e of model.edges) if (paths.has(e.from) && unread.has(e.to)) needed.add(e.to);
  for (const f of model.files.filter(f => paths.has(f.path)))
    for (const b of f.bindings || []) if (b.target && unread.has(b.target)) needed.add(b.target);
  if (words) for (const p of unread) if (words.test(p)) needed.add(p);
  return [...needed];
}

const PATH_WORDS = {
  storage: /(store|storage|save|db|database|repo|persist|model|file)/i,
  send: /(notif|mail|send|client|api|http|webhook)/i,
  failure: /(error|except|fail)/i,
  config: /(config|setting|env)/i,
  test: /(test|spec)/i,
};

/// 保存先として使う変数が関数内でどう作られるかを探す（open(path, 'w') の path など）。
function originOf(file, effect, fnRange) {
  const m = /\bopen\s*\(\s*([A-Za-z_]\w*)/.exec(effect.code) || /\b(?:writeFile|appendFile|createWriteStream)(?:Sync)?\s*\(\s*([A-Za-z_$][\w$]*)/.exec(effect.code);
  if (!m || !fnRange) return null;
  const lines = file.text.split('\n');
  for (let i = effect.line - 2; i >= fnRange.line - 1; i--) {
    if (new RegExp(`(^|[^\\w.])${m[1]}\\s*=(?!=)`).test(lines[i]) || new RegExp(`for\\s+.*\\b${m[1]}\\b.*\\bin\\b`).test(lines[i]))
      return { variable: m[1], line: i + 1, code: lines[i].trim().slice(0, 200), refs: [ref(file.path, i + 1)] };
  }
  return { variable: m[1], line: null, code: null, refs: [] };
}

export function inquire(snapshot, model, question, focus = null) {
  const q = String(question || '').trim();
  const intents = INTENTS.filter(i => i.words.test(q));
  const words = terms(q);
  const findings = [], missing = [], functions = new Map();
  const sources = model.files.filter(f => !isTest(f.path));
  const allEffects = sources.flatMap(f => effects(f));
  for (const intent of intents) {
    if (!intent.kinds.length) continue;
    const list = allEffects.filter(e => intent.kinds.includes(e.kind));
    if (!list.length) {
      missing.push({ text: `読み取った ${sources.length} 件のファイルに、「${intent.label}」に当たる記述は見つからなかった。読み取っていないファイルにある可能性がある`, kind: KIND.code, refs: [] });
      continue;
    }
    const byFn = new Map();
    for (const e of list) { const key = `${e.path}:${e.function_line || 0}`; if (!byFn.has(key)) byFn.set(key, []); byFn.get(key).push(e); }
    for (const [key, items] of byFn) {
      const first = items[0];
      const file = model.files.find(f => f.path === first.path);
      const fnRange = first.function_line ? functionsOf(file).find(f => f.line === first.function_line) : null;
      const strong = items.filter(e => e.certainty !== 'name');
      const parts = [...new Set(items.map(e => e.label))].map(l => `${l}（${items.filter(e => e.label === l).map(e => e.line).join('・')}行目）`);
      const finding = { intent: intent.id, text: `${first.function ? `${first.function}（${first.path}）` : first.path} に、${parts.join('、')}がある`, kind: strong.length ? KIND.code : KIND.inference,
        refs: items.slice(0, 8).map(e => e.refs[0]), lines: items.map(e => ({ line: e.line, code: e.code, label: e.label, refs: e.refs })) };
      if (intent.id === 'storage') {
        const origin = items.map(e => originOf(file, e, fnRange)).find(o => o?.line);
        if (origin) finding.storage_origin = { text: `書き込み先の ${origin.variable} は、${origin.line}行目「${origin.code}」で作られている`, kind: KIND.code, refs: origin.refs };
      }
      findings.push(finding);
      if (first.function_line) functions.set(key, { path: first.path, line: first.function_line, score: (functions.get(key)?.score || 0) + items.length + (strong.length ? 2 : 0) });
    }
  }
  for (const word of words) {
    const escaped = word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    let hit = false;
    for (const f of model.files) {
      for (const fn of functionsOf(f).filter(fn => fn.short === word || fn.name === word)) {
        hit = true;
        functions.set(`${f.path}:${fn.line}`, { path: f.path, line: fn.line, score: 100 });
        findings.push({ intent: 'name', text: `質問の「${word}」と同じ名前の関数 ${fn.name} が ${f.path} の${fn.line}行目にある`, kind: KIND.code, refs: [ref(f.path, fn.line, fn.end)] });
      }
      if (!hit) {
        const lines = f.text.split('\n').map((l, i) => [l, i + 1]).filter(([l]) => new RegExp(escaped, 'i').test(l)).slice(0, 3);
        if (lines.length) { hit = true; findings.push({ intent: 'word', text: `「${word}」を含む記述が ${f.path} にある（${lines.map(x => x[1]).join('・')}行目）`, kind: KIND.code, refs: lines.map(([, n]) => ref(f.path, n)) }); }
      }
    }
    if (!hit) missing.push({ text: `「${word}」を含む記述は、読み取った範囲に見つからなかった`, kind: KIND.code, refs: [] });
  }
  if (focus?.path && focus?.line) {
    const file = model.files.find(f => f.path === focus.path);
    const fn = file && enclosing(functionsOf(file), focus.line);
    if (fn) functions.set(`${focus.path}:${fn.line}`, { path: focus.path, line: fn.line, score: 1000 });
  }
  const details = [...functions.values()].sort((a, b) => b.score - a.score).slice(0, 3).map(f => functionDetail(model, f.path, f.line));
  if (intents.some(i => i.id === 'failure')) {
    for (const d of details.filter(d => d.found)) for (const r of d.raised)
      findings.push({ intent: 'failure', text: `${d.name} は ${r.line}行目で ${r.exception} を発生させる。${r.handled.text}`, kind: r.handled.kind, refs: [...r.refs, ...r.handlers.flatMap(h => h.refs)] });
  }
  const extra = {};
  if (intents.some(i => i.id === 'test')) extra.tests = testsOf(snapshot, model);
  if (intents.some(i => i.id === 'config' || i.id === 'input')) extra.options = optionDefinitions(model);
  const paths = new Set(details.filter(d => d.found).map(d => d.path).concat(findings.flatMap(f => f.refs.map(r => r.path))));
  const pathWords = intents.map(i => PATH_WORDS[i.id]).filter(Boolean);
  const needed = unreadRelated(snapshot, model, paths.size ? paths : new Set(model.files.map(f => f.path)), pathWords.length ? new RegExp(pathWords.map(w => w.source).join('|'), 'i') : null);
  for (const d of details) if (!d.found && d.needed_files) needed.push(...d.needed_files);
  if (!intents.length && !words.length && !focus) missing.push({ text: '質問から、調べる観点（保存・失敗・送信・入力・表示・起動・テスト・設定）や関数名を読み取れなかった。関数名や「保存」「失敗」などの語を含めてください', kind: KIND.code, refs: [] });
  const unread = snapshot.omitted.length;
  return {
    schema_version: 1,
    question: q,
    intents: intents.map(i => ({ id: i.id, label: i.label })),
    terms: words,
    findings: findings.slice(0, 40),
    functions: details,
    ...extra,
    missing,
    needed_files: [...new Set(needed)].slice(0, 8),
    unread_files: unread,
    limits: [
      { text: 'コードに記述があることは、実行時にその処理が行われることを示さない。実際の動作は試験で確かめる', kind: KIND.inference, refs: [] },
      ...(unread ? [{ text: `読み取っていないファイルが ${unread} 件あり、そこに該当する処理がある可能性がある`, kind: KIND.code, refs: [] }] : []),
    ],
    effect_kinds: Object.fromEntries(['input', 'env', 'read', 'write', 'delete', 'send', 'output', 'error_output', 'raise', 'catch', 'exit'].map(k => [k, effectLabel(k)])),
  };
}
