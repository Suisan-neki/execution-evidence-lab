// 固定コミットのコードから概要を作る。各項目は根拠の種類（作者の説明・コードの記述・解析の推測）と参照行を持つ。
// 対象のコードは実行しない。静的な呼び出し候補を、実際に実行された経路として扱わない。
import { maskCode, symbols } from './scanner.js';
import { functionRange, explainFunction } from './comprehension.js';

export const KIND = { author: 'author', code: 'code', inference: 'inference' };
export const UNANALYZED = '解析に対応していない種類（未対応の言語、画像、設定など）';
export const KIND_LABEL = {
  author: '作者の説明（README・コメント等）。コードで確かめていない',
  code: 'コードの記述から確認した事実',
  inference: '解析による推測。実行して確かめていない',
};
export function ref(path, line, endLine = line) { return { path, line, end_line: endLine }; }
/// 末尾の改行の後は行に数えない（固定コミットの行の照合と同じ数え方）。
export function lineCount(text) { const n = text.split('\n').length; return text.endsWith('\n') ? n - 1 : n; }

const EFFECT_LABEL = {
  input: '外部からの入力（引数・標準入力・リクエスト）を受け取る記述',
  env: '環境変数を読む記述',
  read: 'ファイル等から読み出す記述',
  write: 'ファイル等へ書き込む記述',
  delete: 'ファイル等を削除する記述',
  send: '外部へ送信・通信する記述',
  output: '画面（標準出力）へ表示する記述',
  error_output: 'エラー表示（標準エラー）へ出力する記述',
  raise: '例外・エラーを発生させる記述',
  catch: '例外・エラーを捕捉する記述',
  exit: 'プロセスを終了させる記述',
};
export function effectLabel(kind) { return EFFECT_LABEL[kind] || kind; }

const PY = [
  ['input', /\bsys\.argv\b|\.parse_args\s*\(|\binput\s*\(|\bsys\.stdin\b/],
  ['env', /\bos\.environ\b|\bos\.getenv\s*\(/],
  ['error_output', /\bsys\.stderr\.write\s*\(|print\s*\([^)]*file\s*=\s*sys\.stderr/],
  ['output', /\bprint\s*\(|\bsys\.stdout\.write\s*\(/],
  ['write', /\bjson\.dump\s*\(|\bpickle\.dump\s*\(|\bshutil\.(copy\w*|move)\s*\(|\bos\.(rename|replace|makedirs|mkdir)\s*\(|\.commit\s*\(|\.executemany?\s*\(/],
  ['delete', /\bos\.(remove|unlink|rmdir)\s*\(|\bshutil\.rmtree\s*\(/],
  ['send', /\brequests\.(get|post|put|patch|delete|request)\s*\(|\burllib\.request\b|\bhttp\.client\b|\bsocket\.\w+\s*\(|\bsmtplib\b/],
  ['raise', /\braise\b/],
  ['catch', /^\s*except\b/],
  ['exit', /\bsys\.exit\s*\(|\bos\._exit\s*\(/],
];
const JS = [
  ['input', /\bprocess\.argv\b|\bprocess\.stdin\b|\breadline\b|\b(req|request|ctx\.request)\.(body|query|params)\b/],
  ['env', /\bprocess\.env\b/],
  ['error_output', /\bconsole\.(error|warn)\s*\(|\bprocess\.stderr\.write\s*\(/],
  ['output', /\bconsole\.(log|info)\s*\(|\bprocess\.stdout\.write\s*\(|\bres\.(send|json|end|write)\s*\(/],
  ['read', /\breadFile(Sync)?\s*\(|\bcreateReadStream\s*\(|\blocalStorage\.getItem\s*\(|\breaddir(Sync)?\s*\(/],
  ['write', /\b(writeFile|appendFile|rename|mkdir|copyFile)(Sync)?\s*\(|\bcreateWriteStream\s*\(|\blocalStorage\.setItem\s*\(|\.(insert|insertOne|save|put)\s*\(/],
  ['delete', /\b(unlink|rm|rmdir)(Sync)?\s*\(|\blocalStorage\.removeItem\s*\(/],
  ['send', /\bfetch\s*\(|\baxios\b|\bhttps?\.request\s*\(|\bXMLHttpRequest\b|\bWebSocket\s*\(|\.sendMail\s*\(/],
  ['raise', /\bthrow\b/],
  ['catch', /\bcatch\s*[({]|\.catch\s*\(/],
  ['exit', /\bprocess\.exit\s*\(/],
];
const RS = [
  ['input', /\bstd::env::args\b|\benv::args\s*\(|\bstdin\s*\(/],
  ['env', /\benv::var(_os)?\s*\(/],
  ['error_output', /\beprintln!\s*\(/],
  ['output', /\bprintln!\s*\(|\bprint!\s*\(/],
  ['read', /\bfs::read(_to_string|_dir)?\s*\(|\bFile::open\s*\(/],
  ['write', /\bfs::(write|rename|create_dir\w*|copy|hard_link)\s*\(|\bFile::create\s*\(|\bOpenOptions\b/],
  ['delete', /\bfs::remove_\w+\s*\(/],
  ['send', /\bTcpStream::connect\w*\s*\(|\breqwest\b|\bUdpSocket\b/],
  ['raise', /\bpanic!\s*\(|\bbail!\s*\(|\breturn\s+Err\s*\(/],
  ['exit', /\bprocess::exit\s*\(/],
];

function language(path) {
  if (/\.py$/.test(path)) return 'python';
  if (/\.[cm]?[jt]sx?$/.test(path)) return 'javascript';
  if (/\.rs$/.test(path)) return 'rust';
  return null;
}

/// 関数・メソッドの範囲と、Pythonのクラス名を求める。
export function functionsOf(file) {
  const masked = maskCode(file.text, /\.rs$/.test(file.path), /\.py$/.test(file.path));
  const lines = masked.split('\n');
  const classes = [];
  if (/\.py$/.test(file.path)) lines.forEach((l, i) => { const m = /^(\s*)class\s+([A-Za-z_]\w*)/.exec(l); if (m) classes.push({ name: m[2], line: i + 1, indent: m[1].length }); });
  const syms = file.symbols || symbols(file.path, file.text);
  return syms.map(s => {
    const range = functionRange({ ...file, symbols: syms }, s, masked);
    const indent = /^\s*/.exec(lines[s.line - 1] || '')[0].length;
    const owner = classes.filter(c => c.line < s.line && c.indent < indent).at(-1);
    return { name: owner ? `${owner.name}.${s.name}` : s.name, short: s.name, line: s.line, end: Math.max(range.end, s.line), owner: owner?.name || null };
  });
}
export function enclosing(functions, line) {
  return functions.filter(f => f.line <= line && line <= f.end).sort((a, b) => b.line - a.line)[0] || null;
}

/// 記述の種類ごとに行を集める。文字列とコメントは除いて判定する。
export function effects(file) {
  const lang = language(file.path);
  if (!lang) return [];
  const rules = lang === 'python' ? PY : lang === 'rust' ? RS : JS;
  const masked = maskCode(file.text, lang === 'rust', lang === 'python').split('\n');
  const original = file.text.split('\n');
  const functions = functionsOf(file);
  const found = [];
  masked.forEach((line, i) => {
    const kinds = new Set();
    for (const [kind, pattern] of rules) if (pattern.test(line)) kinds.add(kind);
    if (lang === 'python' && /\bopen\s*\(/.test(line)) {
      const mode = /open\s*\([^)]*?,\s*(?:mode\s*=\s*)?['"]([rwaxbt+]+)['"]/.exec(original[i])?.[1] || 'r';
      kinds.add(/[wax+]/.test(mode) ? 'write' : 'read');
    }
    // 名前がwriteの呼び出しは、ファイルへの書き込みとは限らない。候補として区別する。
    const byName = lang === 'python' && /\.write\s*\(/.test(line) && !/\bsys\.std(out|err)\.write/.test(line) && !kinds.has('write');
    if (kinds.has('error_output')) kinds.delete('output');
    const fn = enclosing(functions, i + 1);
    const push = (kind, label, certainty) => found.push({ kind, label, certainty, path: file.path, line: i + 1, code: original[i].trim().slice(0, 200), function: fn?.name || null, function_line: fn?.line || null, refs: [ref(file.path, i + 1)] });
    for (const kind of kinds) push(kind, effectLabel(kind), 'statement');
    if (byName) push('write', '名前が write の呼び出し。ファイル等への書き込みの候補', 'name');
  });
  return found;
}

function isTest(path) { return /(^|\/)(tests?|__tests__|spec)(\/|$)/.test(path) || /\.(test|spec)\.[cm]?[jt]sx?$/.test(path) || /(^|\/)test_[^/]+\.py$/.test(path) || /\.t$/.test(path); }
function isReadme(path) { return /(^|\/)readme(\.[a-z]+)*$/i.test(path); }

const TOOLS = new Set(['pip', 'pip3', 'python', 'python3', 'node', 'npm', 'npx', 'yarn', 'pnpm', 'cargo', 'make', 'go', 'docker']);
/// setup.py・package.jsonで宣言されたコマンド名。READMEのコマンド例を見分けるために使う。
function declaredCommands(model) {
  const names = [];
  for (const f of model.files) {
    if (f.path === 'setup.py' || f.path === 'pyproject.toml')
      for (const m of f.text.matchAll(/['"]?\s*([\w.-]+)\s*=\s*['"]?[\w.]+:[\w.]+/g)) names.push(m[1]);
    if (f.path === 'package.json') try {
      const pkg = JSON.parse(f.text);
      if (typeof pkg.bin === 'string' && pkg.name) names.push(pkg.name);
      else if (pkg.bin) names.push(...Object.keys(pkg.bin));
    } catch { /* 読めないmanifestは使わない。 */ }
  }
  return names;
}
function readmeParts(path, text, names = []) {
  const lines = text.split('\n');
  const purpose = [], commands = [];
  let paragraph = [], start = 0, inFence = false;
  const flush = (end) => {
    const joined = paragraph.join(' ').trim();
    if (joined && purpose.length < 1 && !/^[\[!<]/.test(joined) && joined.length > 20) purpose.push({ text: joined.slice(0, 500), refs: [ref(path, start, end)] });
    paragraph = [];
  };
  lines.forEach((line, i) => {
    if (/^\s*```/.test(line)) { inFence = !inFence; flush(i); return; }
    const command = inFence || /^( {4}|\t)/.test(line) ? line.trim().replace(/^\$\s+/, '') : null;
    const first = command?.split(/\s+/)[0];
    if (command && (TOOLS.has(first) || names.includes(first) || first?.startsWith('./')) && commands.length < 6 && !commands.some(c => c.text === command))
      commands.push({ text: command.slice(0, 200), refs: [ref(path, i + 1)] });
    if (inFence) return;
    if (/^\s*$/.test(line) || /^#+\s|^[=-]{3,}\s*$/.test(line)) { flush(i); return; }
    if (!paragraph.length) start = i + 1;
    paragraph.push(line.trim());
  });
  flush(lines.length);
  return { purpose, commands };
}

function leadingDescription(file) {
  const lines = file.text.split('\n');
  if (/\.py$/.test(file.path)) {
    let i = 0;
    while (i < lines.length && /^\s*(#!.*|#.*coding[:=].*)?$/.test(lines[i])) i++;
    const m = /^\s*[rRuU]?("""|''')(.*)$/.exec(lines[i] || '');
    if (m) {
      const body = [m[2]]; let j = i;
      if (!m[2].includes(m[1])) { for (j = i + 1; j < lines.length && !lines[j].includes(m[1]); j++) body.push(lines[j]); body.push((lines[j] || '').split(m[1])[0]); }
      const text = body.join(' ').replace(m[1], '').trim();
      if (text) return { text: text.slice(0, 300), refs: [ref(file.path, i + 1, j + 1)] };
    }
    const comments = [];
    for (let k = i; k < lines.length && /^\s*#/.test(lines[k]); k++) comments.push(lines[k].replace(/^\s*#\s?/, ''));
    if (comments.join('').trim()) return { text: comments.join(' ').trim().slice(0, 300), refs: [ref(file.path, i + 1, i + comments.length)] };
  } else if (/\.([cm]?[jt]sx?|rs)$/.test(file.path)) {
    const comments = [];
    let k = 0;
    while (k < lines.length && /^\s*(\/\/|\/\*|\*)/.test(lines[k])) { comments.push(lines[k].replace(/^\s*(\/\/[!/]?|\/\*+|\*\/?)\s?/, '')); k++; }
    if (comments.join('').trim()) return { text: comments.join(' ').replace(/\*\/\s*$/, '').trim().slice(0, 300), refs: [ref(file.path, 1, k)] };
  }
  return null;
}

function startMethods(snapshot, model) {
  const items = [];
  const files = Object.fromEntries(model.files.map(f => [f.path, f]));
  for (const s of model.scripts || []) items.push({ text: `package.json のコマンド「${s.name}」: ${s.command}`, kind: KIND.code, refs: [ref(s.path, Math.max(1, s.line))] });
  const pkg = files['package.json'];
  if (pkg) try {
    const data = JSON.parse(pkg.text), lines = pkg.text.split('\n');
    const at = key => Math.max(1, lines.findIndex(l => l.includes(`"${key}"`)) + 1);
    if (typeof data.main === 'string') items.push({ text: `package.json の main に ${data.main} が指定されている`, kind: KIND.code, refs: [ref('package.json', at('main'))] });
    if (data.bin) items.push({ text: `package.json の bin にコマンド ${JSON.stringify(data.bin)} が指定されている`, kind: KIND.code, refs: [ref('package.json', at('bin'))] });
  } catch { /* 読めないmanifestから起動方法を作らない。 */ }
  const setup = files['setup.py'];
  if (setup) {
    const lines = setup.text.split('\n');
    lines.forEach((l, i) => {
      const m = /['"]\s*([\w.-]+)\s*=\s*([\w.]+):([\w.]+)\s*['"]/.exec(l);
      if (m) items.push({ text: `setup.py の console_scripts で、コマンド ${m[1]} がモジュール ${m[2]} の関数 ${m[3]} に対応づけられている`, kind: KIND.code, refs: [ref('setup.py', i + 1)] });
    });
  }
  const pyproject = files['pyproject.toml'];
  if (pyproject) {
    const lines = pyproject.text.split('\n'); let section = '';
    lines.forEach((l, i) => {
      const h = /^\s*\[([^\]]+)\]/.exec(l); if (h) { section = h[1]; return; }
      const m = /^\s*([\w.-]+)\s*=\s*"([\w.]+):([\w.]+)"/.exec(l);
      if (section === 'project.scripts' && m) items.push({ text: `pyproject.toml でコマンド ${m[1]} が ${m[2]} の ${m[3]} に対応づけられている`, kind: KIND.code, refs: [ref('pyproject.toml', i + 1)] });
    });
  }
  for (const e of model.entries) items.push({ text: `${e.path}: ${e.reason}`, kind: /__main__|Rust/.test(e.reason) ? KIND.code : KIND.inference, refs: [ref(e.path, e.line)] });
  const make = files['Makefile'];
  if (make) make.text.split('\n').forEach((l, i) => { const m = /^([A-Za-z][\w-]*):/.exec(l); if (m) items.push({ text: `Makefile の目標「${m[1]}」`, kind: KIND.code, refs: [ref('Makefile', i + 1)] }); });
  for (const f of model.files.filter(f => isReadme(f.path) && !f.path.includes('/')))
    for (const c of readmeParts(f.path, f.text, declaredCommands(model)).commands) items.push({ text: `README に書かれたコマンド: ${c.text}`, kind: KIND.author, refs: c.refs });
  return items;
}

const EXECUTION = {
  python: { runtime: 'python3', supported: true, label: 'Python 3 で実行できる（標準ライブラリで動く範囲）' },
  javascript: { runtime: 'node', supported: true, label: 'Node.js で実行できる（JavaScript。TypeScriptは変換が必要なため未対応）' },
  rust: { runtime: null, supported: false, label: '実行は未対応。理解と試験計画だけを表示する' },
};
export function languages(snapshot) {
  const counts = {};
  for (const f of snapshot.inventory) {
    const ext = /\.([A-Za-z0-9]+)$/.exec(f.path)?.[1]?.toLowerCase();
    const name = { py: 'Python', js: 'JavaScript', mjs: 'JavaScript', cjs: 'JavaScript', ts: 'TypeScript', tsx: 'TypeScript', jsx: 'JavaScript', rs: 'Rust', go: 'Go', rb: 'Ruby', java: 'Java', kt: 'Kotlin', php: 'PHP', c: 'C', cpp: 'C++', cs: 'C#', swift: 'Swift' }[ext];
    if (name) counts[name] = (counts[name] || 0) + 1;
  }
  return Object.entries(counts).sort((a, b) => b[1] - a[1]).map(([name, files]) => ({
    name, files,
    execution: name === 'Python' ? EXECUTION.python.label : name === 'JavaScript' ? EXECUTION.javascript.label : `${name} の実行は未対応。理解と試験計画だけを表示する`,
    executable: name === 'Python' || name === 'JavaScript',
  }));
}

/// 入口から静的な呼び出し候補をたどり、入力・処理・保存/送信の関係を作る。
export function flows(model, maxDepth = 3, maxSteps = 24) {
  const result = [];
  const fileByPath = Object.fromEntries(model.files.map(f => [f.path, f]));
  const effectCache = new Map();
  const effectsIn = (path, start, end) => {
    if (!effectCache.has(path)) effectCache.set(path, effects(fileByPath[path]));
    return effectCache.get(path).filter(e => e.line >= start && e.line <= end);
  };
  for (const entry of model.entries.slice(0, 4)) {
    const file = fileByPath[entry.path];
    if (!file) continue;
    const functions = functionsOf(file);
    // 入口の行から呼ばれる関数を最初の段階にする。
    const entryLine = file.text.split('\n')[entry.line - 1] || '';
    const seeds = [];
    const enclosingEntry = enclosing(functions, entry.line);
    if (enclosingEntry) seeds.push({ path: file.path, line: enclosingEntry.line });
    else {
      const lines = file.text.split('\n');
      for (let i = entry.line; i < Math.min(lines.length, entry.line + 6); i++) {
        for (const m of lines[i].matchAll(/\b([A-Za-z_]\w*)\s*\(/g)) {
          const target = functions.find(f => f.short === m[1] && !f.owner);
          if (target) seeds.push({ path: file.path, line: target.line, from: ref(file.path, i + 1) });
        }
      }
    }
    const steps = [], seen = new Set();
    const queue = seeds.map(s => ({ ...s, depth: 0 }));
    let unresolved = 0;
    while (queue.length && steps.length < maxSteps) {
      const item = queue.shift();
      const key = item.path + ':' + item.line;
      if (seen.has(key)) continue;
      seen.add(key);
      const guide = explainFunction(model, item.path, item.line);
      if (!guide) continue;
      const fnName = functionsOf(fileByPath[item.path]).find(f => f.line === guide.range.start)?.name || guide.name;
      const found = effectsIn(item.path, guide.range.start, guide.range.end);
      steps.push({ function: fnName, path: item.path, line: guide.range.start, end_line: guide.range.end, depth: item.depth,
        called_from: item.from || null, effects: found.map(e => ({ kind: e.kind, label: e.label, certainty: e.certainty, line: e.line, code: e.code, refs: e.refs })), refs: [ref(item.path, guide.range.start, guide.range.end)] });
      if (item.depth >= maxDepth) continue;
      for (const call of guide.calls) {
        if (!call.candidates.length) { if (call.unread?.length) unresolved++; continue; }
        for (const c of call.candidates.slice(0, 2)) queue.push({ path: c.path, line: c.line, depth: item.depth + 1, from: ref(item.path, call.line) });
      }
      // obj.method() と Class() は、読み取ったファイルの同名メソッド・__init__を候補にする（名前だけの対応）。
      for (const call of guide.calls.filter(c => !c.candidates.length)) {
        const parts = call.name.split('.');
        const method = parts.length > 1 ? parts.at(-1) : null;
        for (const f of model.files) for (const fn of functionsOf(f).filter(fn => fn.owner && (method ? fn.short === method : fn.owner === call.name && fn.short === '__init__')).slice(0, 2))
          queue.push({ path: f.path, line: fn.line, depth: item.depth + 1, from: ref(item.path, call.line) });
      }
    }
    const pick = kinds => steps.flatMap(s => s.effects.filter(e => kinds.includes(e.kind)).map(e => ({ ...e, function: s.function })));
    result.push({
      entry: { path: entry.path, line: entry.line, reason: entry.reason, code: entryLine.trim() },
      kind: KIND.inference,
      basis: '入口から、名前と参照宣言で対応づけた呼び出し候補をたどった。分岐の条件・実際に実行される順序・到達は確かめていない',
      inputs: pick(['input', 'env', 'read']),
      storage: pick(['write', 'delete']),
      sending: pick(['send']),
      outputs: pick(['output', 'error_output']),
      errors: pick(['raise', 'catch', 'exit']),
      steps,
      truncated: queue.length > 0,
      unresolved_calls: unresolved,
    });
  }
  return result;
}

export function testsOf(snapshot, model) {
  const items = [];
  for (const f of snapshot.inventory.filter(f => isTest(f.path))) {
    const read = model.files.find(m => m.path === f.path);
    const kind = /\.t$/.test(f.path) ? 'cram（シェルのコマンドと期待する出力を並べた形式）' : /\.py$/.test(f.path) ? 'Pythonのテスト' : /\.[cm]?[jt]sx?$/.test(f.path) ? 'JavaScriptのテスト' : 'テスト';
    items.push({ path: f.path, kind, read: Boolean(read), refs: read ? [ref(f.path, 1, Math.max(1, lineCount(read.text)))] : [] });
  }
  return items;
}

export function overview(snapshot, model) {
  const files = model.files;
  const purpose = [];
  if (snapshot.description) purpose.push({ text: snapshot.description, kind: KIND.author, origin: 'GitHubのリポジトリ説明（作者の記入）', refs: [] });
  for (const f of files.filter(f => isReadme(f.path) && !f.path.includes('/')))
    for (const p of readmeParts(f.path, f.text).purpose) purpose.push({ ...p, kind: KIND.author, origin: `${f.path}（作者の説明）` });
  const components = [];
  const behaviors = [];
  for (const f of files.filter(f => language(f.path) && !isTest(f.path))) {
    const found = effects(f);
    const functions = functionsOf(f);
    const description = leadingDescription(f);
    const counts = {};
    for (const e of found) counts[e.kind] = (counts[e.kind] || 0) + 1;
    components.push({
      path: f.path, role: f.role, lines: lineCount(f.text),
      description: description ? { ...description, kind: KIND.author, origin: 'コード中のコメント・説明文' } : null,
      functions: functions.slice(0, 40).map(fn => ({ name: fn.name, line: fn.line, end_line: fn.end, refs: [ref(f.path, fn.line, fn.end)] })),
      effect_counts: counts,
      refs: [ref(f.path, 1, Math.max(1, lineCount(f.text)))],
    });
    const byFunction = new Map();
    for (const e of found.filter(e => e.function)) {
      if (!byFunction.has(e.function)) byFunction.set(e.function, []);
      byFunction.get(e.function).push(e);
    }
    for (const [name, list] of byFunction) {
      const kinds = [...new Set(list.map(e => e.kind))];
      const labels = [...new Set(list.map(e => e.label))];
      const fn = functions.find(x => x.name === name);
      behaviors.push({
        function: name, path: f.path, line: fn?.line || list[0].line,
        text: `${name} には、${labels.map(l => `${l}（${list.filter(e => e.label === l).map(e => e.line).join('・')}行目）`).join('、')}がある`,
        kind: KIND.code,
        kinds,
        refs: list.slice(0, 8).map(e => e.refs[0]),
      });
    }
  }
  const omitted = new Set(snapshot.omitted.map(o => o.path));
  const unread = [...snapshot.omitted.map(o => ({ path: o.path, reason: o.reason })),
    ...snapshot.inventory.filter(f => !(f.path in snapshot.files) && !omitted.has(f.path)).map(f => ({ path: f.path, reason: UNANALYZED }))];
  const unknown = [
    { text: 'README やコメントの説明が、現在のコードの動作と一致するかは確かめていない', kind: KIND.inference, refs: [] },
    { text: '処理の関係は、名前と参照宣言から作った呼び出し候補。実際にどの経路が実行されるかは、試験で確かめる必要がある', kind: KIND.inference, refs: [] },
    { text: '外部サービス、実行環境の設定、利用者の権限、保存先の実際の状態は、コードを読むだけでは確かめられない', kind: KIND.inference, refs: [] },
  ];
  if (unread.length) unknown.unshift({ text: `読み取っていないファイルが ${unread.length} 件ある。必要に応じて読み込む`, kind: KIND.code, refs: [] });
  if (snapshot.truncated) unknown.unshift({ text: 'GitHubのファイル一覧が途中で切れている。一覧にないファイルがある可能性がある', kind: KIND.code, refs: [] });
  return {
    schema_version: 1,
    repository: snapshot.fullName,
    commit: snapshot.commit,
    reference: snapshot.reference,
    read: { inventory: snapshot.inventory.length, files_read: files.length, unread, truncated: snapshot.truncated === true },
    languages: languages(snapshot),
    purpose,
    start: startMethods(snapshot, model),
    components,
    behaviors,
    flows: flows(model),
    tests: testsOf(snapshot, model),
    unknown,
    kinds: KIND_LABEL,
    limits: 'コードの記述から作った説明です。実行していないため、実際の動作・到達・成否は試験で確かめる必要があります。',
  };
}
