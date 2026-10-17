// 通常時・故障時の試験を準備するための材料を、固定コミットのコードから集める。
// ここで作る案はコード・テストからの提案で、人が確認して保存するまで決定ではない。対象は実行しない。
import { KIND, ref, effects, languages, testsOf } from './overview.js';
import { optionDefinitions } from './inquiry.js';

export const FAULTS = [
  { id: 'normal', label: '故障なし（通常時）', needs_storage: false },
  { id: 'storage_read_only', label: '保存先を書き込み不可にする', needs_storage: true, how: '試行の前に、基盤が保存先へ空のファイルを作り、書き込み権限を外す。書き込めないことを基盤が確かめてから対象を起動する' },
  { id: 'storage_is_directory', label: '保存先に同名のディレクトリを置く', needs_storage: true, how: '試行の前に、基盤が保存先と同じ名前のディレクトリを作る。ファイルとして開けない状態になる' },
  { id: 'skip_state_read', label: '基盤による保存先の読み出しを止める（観測の欠落）', needs_storage: false, how: '対象は通常どおり動かし、試行後に基盤が保存先を読み出す処理だけを止める。観測がないときに成功と扱わないかを確かめる' },
  { id: 'network_unavailable', label: '外部への通信を遮断する', needs_storage: false, how: 'Linuxのbubblewrap方式で、ネットワークを持たない環境で起動する。local方式では使えない' },
  { id: 'storage_directory_read_only', label: '保存先のディレクトリを書き込み不可にする', needs_storage: true, how: '試行の前に、基盤が保存先のディレクトリを空で作り、書き込み権限を外す。ディレクトリへ書き込めないことを基盤が確かめてから起動する' },
  { id: 'server_not_started', label: '待つプロセス（受信側など）を起動しない', needs_server: true, how: '待つプロセスを起動せず、操作するプロセスだけを動かす。割り当てたポートが応答しないことを基盤が確かめる' },
];

function dependencies(model) {
  const items = [];
  for (const f of model.files) {
    if (/(^|\/)requirements[^/]*\.txt$/.test(f.path)) f.text.split('\n').forEach((l, i) => { const name = l.replace(/#.*/, '').trim(); if (name) items.push({ name, file: f.path, refs: [ref(f.path, i + 1)] }); });
    if (/(^|\/)package\.json$/.test(f.path)) try {
      const pkg = JSON.parse(f.text), lines = f.text.split('\n');
      for (const group of ['dependencies', 'devDependencies', 'peerDependencies'])
        for (const name of Object.keys(pkg[group] || {})) items.push({ name: `${name}（${group}）`, file: f.path, refs: [ref(f.path, Math.max(1, lines.findIndex(l => l.includes(`"${name}"`)) + 1))] });
    } catch { /* 読めないmanifestは依存関係なしと扱わない。 */ items.push({ name: 'package.json を読み取れなかった', file: f.path, refs: [] }); }
    if (/(^|\/)pyproject\.toml$/.test(f.path)) {
      const m = /dependencies\s*=\s*\[([\s\S]*?)\]/.exec(f.text);
      if (m) for (const d of m[1].matchAll(/["']([^"']+)["']/g)) items.push({ name: d[1], file: f.path, refs: [ref(f.path, f.text.slice(0, m.index).split('\n').length)] });
    }
  }
  return items;
}

/// cram形式のテスト（$ コマンドと期待する出力）を例として取り出す。
function cramExamples(model) {
  const examples = [];
  for (const f of model.files.filter(f => /\.t$/.test(f.path))) {
    const lines = f.text.split('\n');
    let alias = null;
    for (let i = 0; i < lines.length && examples.length < 40; i++) {
      const m = /^  \$ (.*)$/.exec(lines[i]);
      if (!m) continue;
      const a = /^alias\s+(\w+)="(.*)"$/.exec(m[1]);
      if (a) { alias = { name: a[1], command: a[2], line: i + 1 }; continue; }
      const expected = [];
      let j = i + 1;
      while (j < lines.length && /^  (?!\$ )\S/.test(lines[j])) { expected.push(lines[j].slice(2)); j++; }
      examples.push({ path: f.path, command: m[1], expected, alias: alias && m[1].split(' ')[0] === alias.name ? alias.command : null, refs: [ref(f.path, i + 1, Math.max(i + 1, j))] });
    }
  }
  return examples;
}

function testCommands(snapshot, model, deps) {
  const commands = [];
  const files = Object.fromEntries(model.files.map(f => [f.path, f]));
  for (const s of model.scripts || []) if (/test/.test(s.name)) commands.push({ text: `npm run ${s.name}（package.json: ${s.command}）`, refs: [ref(s.path, Math.max(1, s.line))], runnable: false, reason: 'npm scripts は依存パッケージのインストールを伴う場合がある。この基盤はインストールを行わないため、画面からは実行しない' });
  const make = files['Makefile'];
  if (make) make.text.split('\n').forEach((l, i) => { if (/^test\w*:/.test(l)) commands.push({ text: `make ${l.split(':')[0]}`, refs: [ref('Makefile', i + 1)], runnable: false, reason: 'Makefile の処理は任意のコマンドを含むため、画面からは実行しない' }); });
  for (const f of model.files.filter(f => /^\.github\/workflows\//.test(f.path))) f.text.split('\n').forEach((l, i) => { const m = /^\s*(?:-\s*)?run:\s*(.+)$/.exec(l); if (m && /test|pytest|cram|npm|node/.test(m[1])) commands.push({ text: `GitHub Actions: ${m[1].trim()}`, refs: [ref(f.path, i + 1)], runnable: false, reason: '自動テストの設定にあるコマンド。依存関係の準備を含むため、画面からは実行しない' }); });
  if (snapshot.inventory.some(f => /\.t$/.test(f.path))) {
    const cram = deps.find(d => /^cram\b/i.test(d.name));
    commands.push({ text: 'cram tests/*.t（cram形式のテスト）', refs: cram?.refs || [], runnable: false, reason: cram ? `cram は ${cram.file} に依存パッケージとして書かれている。この基盤は依存パッケージをインストールしないため、既存テストは実行しない。テストの例を判定条件の案に使う` : 'cram が必要。この基盤は依存パッケージをインストールしない' });
  }
  return commands;
}

function entryCandidates(snapshot, model) {
  const items = [];
  const files = Object.fromEntries(model.files.map(f => [f.path, f]));
  const has = path => Boolean(files[path] || snapshot.inventory.some(f => f.path === path));
  for (const e of model.entries) {
    const runtime = /\.py$/.test(e.path) ? 'python3' : /\.(m?js|cjs)$/.test(e.path) ? 'node' : null;
    items.push({ entrypoint: e.path, launch: 'file', runtime, reason: e.reason, kind: /__main__/.test(e.reason) ? KIND.code : KIND.inference, refs: [ref(e.path, e.line)], supported: Boolean(runtime), unsupported_reason: runtime ? null : 'この言語の実行は未対応' });
  }
  // パッケージの __main__.py は、ファイルを直接起動すると相対的な import が解決できないことがある。
  for (const f of snapshot.inventory.filter(f => /(^|\/)__main__\.py$/.test(f.path))) {
    const module = f.path.replace(/\/__main__\.py$/, '').replaceAll('/', '.');
    if (/^[A-Za-z_][\w.]*$/.test(module) && has(f.path.replace(/__main__\.py$/, '__init__.py')))
      items.push({ entrypoint: module, launch: 'module', runtime: 'python3', reason: `${f.path} があり、python -m ${module} で起動できるパッケージ`, kind: KIND.code, refs: [ref(f.path, 1)], supported: true });
  }
  const pyproject = files['pyproject.toml'];
  if (pyproject) {
    let section = '';
    pyproject.text.split('\n').forEach((l, i) => {
      const header = /^\s*\[([^\]]+)\]\s*$/.exec(l);
      if (header) { section = header[1].trim(); return; }
      const m = section === 'project.scripts' && /^\s*([\w.-]+)\s*=\s*["']([\w.]+):(\w+)["']/.exec(l);
      if (!m) return;
      const base = m[2].replaceAll('.', '/');
      items.push({ entrypoint: m[1], launch: 'script', runtime: 'python3', reason: `pyproject.toml の [project.scripts] でコマンド ${m[1]} の入口が ${m[2]} の関数 ${m[3]} とされている`, kind: KIND.code, refs: [ref('pyproject.toml', i + 1)], supported: has(`${base}.py`) || has(`${base}/__init__.py`) });
    });
  }
  const setup = files['setup.py'];
  if (setup) setup.text.split('\n').forEach((l, i) => {
    const m = /['"]\s*([\w.-]+)\s*=\s*([\w.]+):([\w.]+)\s*['"]/.exec(l);
    const path = m && `${m[2].replaceAll('.', '/')}.py`;
    if (m && !items.some(x => x.entrypoint === path)) items.push({ entrypoint: path, runtime: 'python3', reason: `setup.py でコマンド ${m[1]} の入口とされる関数 ${m[3]} がある`, kind: KIND.code, refs: [ref('setup.py', i + 1)], supported: Boolean(files[path] || snapshot.inventory.some(f => f.path === path)) });
  });
  const pkg = files['package.json'];
  if (pkg) try {
    const data = JSON.parse(pkg.text);
    const add = (p, why) => { const path = p.replace(/^\.\//, ''); if (!items.some(x => x.entrypoint === path)) items.push({ entrypoint: path, runtime: /\.(m?js|cjs)$/.test(path) ? 'node' : null, reason: why, kind: KIND.code, refs: [ref('package.json', 1)], supported: /\.(m?js|cjs)$/.test(path) }); };
    if (typeof data.main === 'string') add(data.main, 'package.json の main');
    if (typeof data.bin === 'string') add(data.bin, 'package.json の bin');
    else for (const [n, p] of Object.entries(data.bin || {})) add(p, `package.json の bin（${n}）`);
  } catch { /* 読めないmanifestから入口を作らない。 */ }
  return items;
}

function storageCandidates(model, options) {
  const items = [];
  for (const f of model.files) for (const e of effects(f).filter(e => e.kind === 'write' && e.certainty !== 'name'))
    items.push({ text: `${e.function || f.path} の${e.line}行目に書き込みの記述がある: ${e.code}`, kind: KIND.code, refs: e.refs });
  for (const o of options.filter(o => /dir|file|path|list|name|db|out|store/i.test([o.dest, o.metavar, ...o.flags].join(' '))))
    items.push({ text: `オプション ${o.flags.join(' / ')}${o.help ? `（説明: ${o.help}）` : ''}${o.default != null ? `、既定値 ${o.default}` : ''}。保存先の指定に関係する可能性がある`, kind: KIND.inference, refs: o.refs, option: o });
  return items;
}

export function plan(snapshot, model) {
  const langs = languages(snapshot);
  const options = optionDefinitions(model);
  const deps = dependencies(model);
  const entries = entryCandidates(snapshot, model);
  const storage = storageCandidates(model, options);
  const sends = model.files.flatMap(f => effects(f).filter(e => e.kind === 'send'));
  const executable = entries.some(e => e.supported);
  const runtimeDeps = deps.filter(d => !/^cram\b|pytest|mocha|jest|vitest|playwright|eslint|typescript|devDependencies/i.test(d.name));
  const missing = [];
  if (!executable) missing.push({ text: '実行できる入口が見つからない。この基盤は Python 3 と Node.js（JavaScript）だけを実行できる', kind: KIND.code });
  missing.push({ text: '試行で使う引数。試験用の値を入れる場所には {{test_value}} と書く', kind: KIND.inference });
  missing.push({ text: '保存先のパス（試行用の作業ディレクトリからの相対パス）。保存先を指定しないと、基盤による保存先の読み出しと保存先の故障は使えない', kind: KIND.inference });
  missing.push({ text: '後から読み出すコマンド（任意）。書き込みとは別のプロセスで読み出せるかを確かめる', kind: KIND.inference });
  if (runtimeDeps.length) missing.push({ text: `依存パッケージ（${runtimeDeps.slice(0, 6).map(d => d.name).join('、')}）は、この基盤ではインストールしない。Pythonだけで書かれた依存先は、公開GitHubのリポジトリと固定コミットを「依存先」に指定すれば読み込める。どのリポジトリ・コミットが対応するかは、コードからは決めない。C拡張やビルドが必要な依存先は扱えない`, kind: KIND.code, refs: runtimeDeps.slice(0, 6).flatMap(d => d.refs) });
  missing.push({ text: '先に起動して待つプロセス（任意）。受信サーバーのように待ち受けるプロセスは、引数のポートを {{port}} と書くと、基盤が 127.0.0.1 の空いているポートを割り当てる', kind: KIND.inference });
  return {
    schema_version: 1,
    repository: snapshot.fullName,
    commit: snapshot.commit,
    languages: langs,
    executable,
    unsupported_reason: executable ? null : `${langs.map(l => l.name).join('・') || 'この言語'} の実行は未対応。理解と試験計画だけを表示する`,
    entries,
    options,
    storage,
    tests: testsOf(snapshot, model),
    test_commands: testCommands(snapshot, model, deps),
    examples: cramExamples(model),
    dependencies: deps,
    faults: FAULTS.map(f => ({ ...f, applicable: f.id === 'network_unavailable' ? (sends.length ? 'bubblewrap方式（Linux）でだけ使える。送信の記述がある' : '送信の記述が見つからないため、効果を確かめにくい。bubblewrap方式（Linux）でだけ使える') : f.needs_server ? '待つプロセスを指定したときに使える' : f.id === 'storage_directory_read_only' ? 'ディレクトリを含む保存先のパス（例: received/file）を指定したときに使える' : f.needs_storage ? '保存先のパスを指定したときに使える' : '使える' })),
    observation: [
      { method: 'exit_code', label: '終了コード', source: '対象プログラムの報告' },
      { method: 'stdout', label: '標準出力', source: '対象プログラムの報告' },
      { method: 'stderr', label: '標準エラー（エラー表示）', source: '対象プログラムの報告' },
      { method: 'stored_file', label: '保存先の内容', source: '基盤が保存先を直接読み出した結果' },
      { method: 'readback_output', label: '後からの読み出し', source: '別のプロセスで対象の読み出し機能を動かした結果' },
      { method: 'server_output', label: '待つプロセスの出力', source: '待つプロセス（受信側など）の出力' },
    ],
    missing,
    isolation: '対象は試行ごとに新しい作業ディレクトリで動く。固定コミットのファイルは読み取り専用で複製し、環境変数は消し、HOMEも作業ディレクトリにする。local方式はOSによる隔離を行わないため、対象が絶対パスで書き込む処理は止められない',
    origin: 'コード・テストからの提案。人が確認して保存するまで、期待や判定条件の決定ではない',
  };
}
