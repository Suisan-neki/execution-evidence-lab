// 公開GitHubのコードを読む。対象リポジトリのコードやコマンドは実行しない。
export const MAX_FILES = 24;
export const MAX_FILE_BYTES = 128 * 1024;
const SOURCE = /\.(rs|[cm]?[jt]sx?|py)$/i;
const SKIP = /(^|\/)(node_modules|vendor|target|dist|build|\.git)(\/|$)/;
const encoder = new TextEncoder();

export function repositoryAddress(value) {
  let url;
  try { url = new URL(value.trim()); } catch { throw Error("https://github.com/所有者/リポジトリ のURLを入力してください。"); }
  if (url.protocol !== "https:" || url.hostname !== "github.com" || url.port || url.username || url.password || url.search || url.hash)
    throw Error("公開GitHubリポジトリのURLを入力してください。認証情報やクエリは指定できません。");
  const match = /^\/([A-Za-z0-9-]+)\/([A-Za-z0-9_.-]+)\/?$/.exec(url.pathname);
  if (!match) throw Error("ファイルやブランチのURLではなくリポジトリのURLを入力してください。ブランチは別の欄に指定できます。");
  const repo = match[2].replace(/\.git$/, "");
  if (!repo || repo === "." || repo === "..") throw Error("リポジトリ名を確認してください。");
  return { owner: match[1], repo, fullName: `${match[1]}/${repo}` };
}

async function response(url, json = false) {
  let r;
  try { r = await fetch(url, { credentials: "omit", signal: AbortSignal.timeout(20000), headers: json ? { Accept: "application/vnd.github+json" } : {} }); }
  catch { throw Error("GitHubに接続できませんでした。ネットワークを確認して再実行してください。"); }
  if (!r.ok) {
    if (r.status === 403 || r.status === 429) throw Error("GitHubの公開APIの利用上限、またはアクセス制限に達しました。時間を置いて再実行してください。");
    if (r.status === 404) throw Error("公開リポジトリまたは指定した版が見つかりません。URLとブランチ名を確認してください。");
    throw Error(`GitHubから取得できませんでした（HTTP ${r.status}）。`);
  }
  return json ? r.json() : r;
}

function readable(path) {
  return !SKIP.test(path) && (SOURCE.test(path) || /(^|\/)(readme[^/]*\.md|Cargo\.toml|package\.json|pyproject\.toml|requirements\.txt)$/i.test(path));
}
function priority(path) {
  if (/^readme[^/]*\.md$/i.test(path)) return 0;
  if (/^(Cargo\.toml|package\.json|pyproject\.toml)$/.test(path)) return 1;
  if (/(^|\/)(main\.(rs|py|[jt]sx?)|lib\.rs|server\.[cm]?[jt]s|app\.[jt]sx?|index\.[cm]?[jt]sx?)$/.test(path)) return 2;
  if (/(^|\/)(test[s]?|__tests__|tools)(\/|\.)/.test(path) || /\.(test|spec)\./.test(path)) return 5;
  return SOURCE.test(path) ? 3 : 4;
}

export async function readSource(snapshot, path) {
  const entry = snapshot.inventory.find(f => f.path === path);
  if (!entry || !["100644", "100755"].includes(entry.mode)) throw Error("このファイルはコードの読取対象ではありません。");
  if (entry.size > MAX_FILE_BYTES) throw Error("128KiBを超えるファイルはこの試作の読取範囲外です。GitHubで確認してください。");
  const encodedPath = path.split("/").map(encodeURIComponent).join("/");
  const url = `https://raw.githubusercontent.com/${snapshot.fullName}/${snapshot.commit}/${encodedPath}`;
  const r = await response(url);
  const reader = r.body.getReader();
  const parts = [];
  let total = 0;
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > MAX_FILE_BYTES) { await reader.cancel(); throw Error("ファイルが読取上限の128KiBを超えました。"); }
    parts.push(value);
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.byteLength; }
  let text;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { throw Error("UTF-8のテキストとして読み取れませんでした。"); }
  if (text.includes("\0")) throw Error("バイナリファイルは読み取りません。");
  snapshot.files[path] = text;
  snapshot.omitted = snapshot.omitted.filter(f => f.path !== path);
  return text;
}

export async function scanPublicRepository(value, reference = "", progress = () => {}) {
  const address = repositoryAddress(value);
  const api = `https://api.github.com/repos/${address.fullName}`;
  progress("公開リポジトリの情報を取得しています");
  const repo = await response(api, true);
  if (repo.private) throw Error("最初の試作は公開リポジトリに対応しています。");
  progress("読むコミットを固定しています");
  const ref = reference.trim() || repo.default_branch;
  const commit = await response(`${api}/commits/${encodeURIComponent(ref)}`, true);
  if (!/^[a-f0-9]{40}$/.test(commit.sha) || !/^[a-f0-9]{40}$/.test(commit.commit?.tree?.sha)) throw Error("GitHubのコミット情報を読み取れませんでした。");
  progress("ファイル構成を取得しています");
  const tree = await response(`${api}/git/trees/${commit.commit.tree.sha}?recursive=1`, true);
  const inventory = tree.tree.filter(f => f.type === "blob").map(({ path, size, mode, sha }) => ({ path, size, mode, sha }));
  const snapshot = { schema: 1, fullName: address.fullName, description: repo.description || "", reference: ref, commit: commit.sha, scannedAt: new Date().toISOString(), truncated: tree.truncated === true, inventory, files: {}, omitted: [] };
  const candidates = inventory.filter(f => readable(f.path) && ["100644", "100755"].includes(f.mode))
    .sort((a, b) => priority(a.path) - priority(b.path) || a.path.localeCompare(b.path));
  const selected = candidates.filter(f => f.size <= MAX_FILE_BYTES).slice(0, MAX_FILES);
  snapshot.omitted = candidates.filter(f => !selected.includes(f)).map(f => ({ path: f.path, reason: f.size > MAX_FILE_BYTES ? "サイズ上限" : "初回の読取範囲外" }));
  let next = 0, complete = 0;
  await Promise.all(Array.from({ length: Math.min(4, selected.length) }, async () => {
    while (next < selected.length) {
      const file = selected[next++];
      try { await readSource(snapshot, file.path); }
      catch (e) { snapshot.omitted.push({ path: file.path, reason: e.message }); }
      complete++;
      progress(`コードを読んでいます ${complete} / ${selected.length}ファイル`);
    }
  }));
  return snapshot;
}

export function codeLink(snapshot, path, line = 1) {
  return `https://github.com/${snapshot.fullName}/blob/${snapshot.commit}/${path.split("/").map(encodeURIComponent).join("/")}#L${line}`;
}

// コメント・文字列を空白に置き換え、位置と改行を保つ。未対応構文を意味解析しない。
export function maskCode(text, rust = false, python = false) {
  const out = text.split('');
  function blank(start, end) { for (let j = start; j < end; j++) if (text[j] !== '\n') out[j] = ' '; }
  let i = 0;
  while (i < text.length) {
    const start = i;
    if ((!python && text.startsWith('//', i)) || (python && text[i] === '#')) {
      i = text.indexOf('\n', i); if (i < 0) i = text.length; blank(start, i); continue;
    }
    if (!python && text.startsWith('/*', i)) {
      i += 2; let depth = 1;
      while (i < text.length && depth) {
        if (rust && text.startsWith('/*', i)) { depth++; i += 2; }
        else if (text.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      blank(start, i); continue;
    }
    const raw = rust && (text[i] === "r" || text[i] === "b") && /^(?:br|r)(#*)"/.exec(text.slice(i));
    if (raw && (i === 0 || !/[\w]/.test(text[i - 1]))) {
      const close = '"' + raw[1], end = text.indexOf(close, i + raw[0].length);
      i = end < 0 ? text.length : end + close.length; blank(start, i); continue;
    }
    const quote = text[i];
    if (quote === '"' || quote === "'" || (!rust && !python && quote === '`')) {
      // Rustのライフタイムは文字リテラルとして消さない。
      if (rust && quote === "'" && !/^'(?:\\.|[^'\\\n])'/.test(text.slice(i))) { i++; continue; }
      const triple = python && text.startsWith(quote.repeat(3), i);
      const delimiter = triple ? quote.repeat(3) : quote;
      i += delimiter.length;
      while (i < text.length) {
        if (text[i] === '\\') { i += 2; continue; }
        if (text.startsWith(delimiter, i)) { i += delimiter.length; break; }
        i++;
      }
      blank(start, Math.min(i, text.length)); continue;
    }
    // JSの正規表現のよく使う位置のみ除外。完全な構文解析は行わない。
    if (!rust && !python && quote === '/' && /(?:^|[=(:,!&|?;{}]|\breturn|=>)\s*$/.test(text.slice(Math.max(0, i - 100), i))) {
      i++; let bracket = false;
      while (i < text.length && text[i] !== '\n') {
        if (text[i] === '\\') { i += 2; continue; }
        if (text[i] === '[') bracket = true;
        if (text[i] === ']') bracket = false;
        if (text[i] === '/' && !bracket) { i++; while (/[a-z]/i.test(text[i] || '0')) i++; break; }
        i++;
      }
      blank(start, i); continue;
    }
    i++;
  }
  return out.join('');
}

function normalize(path) {
  const out = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") { if (!out.length) return null; out.pop(); } else out.push(part);
  }
  return out.join("/");
}
function resolvePath(path, specifier, paths) {
  const directory = path.split("/").slice(0, -1).join("/");
  const base = normalize(`${directory}/${specifier}`);
  if (!base) return null;
  const extensions = /\.py$/.test(path) ? [".py", "/__init__.py"] : /\.rs$/.test(path) ? [".rs", "/mod.rs"] : [".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx", "/index.js", "/index.ts", "/index.tsx"];
  return [base, ...extensions.map(ext => base + ext)].find(p => paths.has(p)) || null;
}
export function symbols(path, text) {
  if (!SOURCE.test(path)) return [];
  const masked = maskCode(text, /\.rs$/.test(path), /\.py$/.test(path)), lines = masked.split("\n"), found = [];
  const pattern = /(?:^|\s)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][\w]*)\s*\(|(?:^|\s)(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(|^\s*(?:async\s+)?def\s+([A-Za-z_][\w]*)\s*\(|(?:^|\s)(?:export\s+)?(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=.*=>/;
  lines.forEach((line, i) => { const m = pattern.exec(line); if (m) found.push({ name: m.slice(1).find(Boolean), line: i + 1 }); });
  return found.map((s, i) => ({ ...s, end: (found[i + 1]?.line || lines.length + 1) - 1 }));
}
function imports(path, text, paths, crateName) {
  const edges = [];
  const clean = maskCode(text, /\.rs$/.test(path), /\.py$/.test(path));
  const masked = clean.split("\n");
  text.split("\n").forEach((line, i) => {
    const code = masked[i].trim();
    let target;
    if (/^(?:import|export)\b/.test(code) || /\brequire\s*\(/.test(code)) {
      const spec = /(?:from\s*|import\s*|require\s*\(\s*)["']([^"']+)["']/.exec(line)?.[1];
      if (spec?.startsWith(".")) target = resolvePath(path, spec, paths);
    }
    if (/\.rs$/.test(path)) {
      const module = /^(?:pub\s+)?mod\s+([A-Za-z_]\w*)\s*;/.exec(code)?.[1];
      if (module) {
        const moduleDir = /\/(?:lib|main|mod)\.rs$/.test(path) ? path.split("/").slice(0, -1).join("/") : path.replace(/\.rs$/, "");
        target = [`${moduleDir}/${module}.rs`, `${moduleDir}/${module}/mod.rs`].find(p => paths.has(p));
      }
      const uses = /^use\s+crate::([A-Za-z_]\w*)/.exec(code)?.[1];
      if (uses) target = [`src/${uses}.rs`, `src/${uses}/mod.rs`].find(p => paths.has(p));
    }
    if (/\.py$/.test(path)) {
      const relative = /^from\s+(\.+)([\w.]*)\s+import\b/.exec(code);
      if (relative) {
        const prefix = "../".repeat(relative[1].length - 1);
        const module = relative[2].replaceAll(".", "/");
        if (module) target = resolvePath(path, `./${prefix}${module}`, paths);
      }
    }
    if (target && target !== path) edges.push({ from: path, to: target, line: i + 1, kind: "参照宣言" });
  });
  if (/\.rs$/.test(path)) {
    for (const match of clean.matchAll(/\buse\s+([A-Za-z_]\w*)::\{([\s\S]*?)\}\s*;/g)) {
      if (match[1] !== "crate" && match[1] !== crateName) continue;
      let depth = 0, start = 0;
      const parts = [];
      for (let i = 0; i <= match[2].length; i++) {
        if ((match[2][i] === "," && depth === 0) || i === match[2].length) { parts.push(match[2].slice(start, i)); start = i + 1; }
        else if (match[2][i] === "{") depth++;
        else if (match[2][i] === "}") depth--;
      }
      const line = clean.slice(0, match.index).split("\n").length;
      for (const part of parts) {
        const module = /^\s*([A-Za-z_]\w*)/.exec(part)?.[1];
        const target = [`src/${module}.rs`, `src/${module}/mod.rs`].find(p => paths.has(p));
        if (target && target !== path) edges.push({ from: path, to: target, line, kind: "参照宣言" });
      }
    }
  }
  return edges;
}

export function analyze(snapshot) {
  const paths = new Set(snapshot.inventory.map(f => f.path));
  const crateName = /^name\s*=\s*"([^"]+)"/m.exec(snapshot.files["Cargo.toml"] || "")?.[1]?.replaceAll("-", "_");
  const files = Object.entries(snapshot.files).map(([path, text]) => {
    const bindings = importBindings(path, text, paths, crateName);
    const edges = imports(path, text, paths, crateName);
    for (const binding of bindings) if (binding.target && binding.target !== path && !edges.some(e => e.to === binding.target && e.line === binding.line))
      edges.push({ from: path, to: binding.target, line: binding.line, kind: "参照宣言" });
    return { path, text, symbols: symbols(path, text), imports: edges, bindings, role: role(path) };
  });
  const entries = files.filter(f => /(^|\/)(main\.(rs|py)|server\.[cm]?[jt]s|app\.[jt]sx?|index\.[cm]?[jt]sx?)$/.test(f.path)).map(f => ({ path: f.path, line: f.symbols.find(s => s.name === "main")?.line || 1, reason: /main\.rs$/.test(f.path) ? "Rustの実行入口に使われるファイル" : "名前から選んだ入口の候補" }));
  const scripts = [];
  for (const f of files.filter(f => /(^|\/)package\.json$/.test(f.path))) {
    try {
      const pkg = JSON.parse(f.text);
      for (const [name, command] of Object.entries(pkg.scripts || {}))
        if (typeof command === "string") scripts.push({ path: f.path, name, command, line: f.text.split("\n").findIndex(l => l.includes(JSON.stringify(name))) + 1 });
    } catch { /* 読めないmanifestを成功した起動設定としない。 */ }
  }
  return { files, entries, scripts, inventory: snapshot.inventory, inventory_truncated: snapshot.truncated === true, edges: files.flatMap(f => f.imports) };
}

export function role(path) {
  if (/readme/i.test(path)) return "プロジェクトの説明";
  if (/(Cargo\.toml|package\.json|pyproject\.toml|requirements\.txt)$/.test(path)) return "起動設定・依存パッケージ";
  if (/(^|\/)(test[s]?|__tests__|tools)(\/|\.)/.test(path) || /\.(test|spec)\./.test(path)) return "テスト・補助処理";
  if (/\.rs$/.test(path)) return "Rustのコード";
  if (/\.[cm]?[jt]sx?$/.test(path)) return "JavaScript / TypeScriptのコード";
  if (/\.py$/.test(path)) return "Pythonのコード";
  return "資料";
}

// 外部へ送る記録には同じコミットのソースを含める。ダウンロード後も出所をたどれる。
export function validateSnapshot(value) {
  if (value?.schema !== 1 || !/^[a-f0-9]{40}$/.test(value.commit) || !Array.isArray(value.inventory) || value.inventory.length > 100000 || !value.files || !Array.isArray(value.omitted)) throw Error("この画面から保存した読取記録を選んでください。");
  const address = repositoryAddress("https://github.com/" + value.fullName);
  if (address.fullName !== value.fullName) throw Error("リポジトリ情報が不正です。");
  for (const f of value.inventory) if (typeof f.path !== "string" || !f.path || f.path.split("/").some(p => p === ".." || p === ".") || f.path.startsWith("/") || /[\x00-\x1f]/.test(f.path)) throw Error("ファイルのパスが不正です。");
  for (const [path, text] of Object.entries(value.files))
    if (!value.inventory.some(f => f.path === path) || typeof text !== "string" || encoder.encode(text).length > MAX_FILE_BYTES) throw Error("読取記録のコードが不正です。");
  return value;
}

// 読み込み宣言に現れる別名。対応する構文だけを記録する。
const IDENT = /^[A-Za-z_$][\w$]*$/;
export function rustPath(parts, paths) {
  const base = 'src/' + parts.join('/');
  return [base+'.rs',base+'/mod.rs'].find(p=>paths.has(p)) || null;
}
function splitUse(text) {
  let depth=0, start=0;const result=[];
  for(let i=0;i<=text.length;i++) {
    if(i===text.length || (text[i]===',' && depth===0)) {result.push(text.slice(start,i).trim());start=i+1;}
    else if(text[i]==='{') depth++;else if(text[i]==='}') depth--;
  }
  return result.filter(Boolean);
}
export function importBindings(path, text, paths, crateName) {
  const clean=maskCode(text,/\.rs$/.test(path),/\.py$/.test(path)), result=[];
  const lineAt=offset=>clean.slice(0,offset).split('\n').length;
  const add=(match, values)=>{
    const prefix=clean.slice(0,match.index);
    const nested=/\.py$/.test(path) ? /^[ \t]+/.test(match[0]) : (prefix.match(/{/g)||[]).length > (prefix.match(/}/g)||[]).length;
    result.push({...values,nested,line:lineAt(match.index),end_line:lineAt(match.index+match[0].length),declaration:text.slice(match.index,match.index+match[0].length).trim()});
  };
  if(/\.[cm]?[jt]sx?$/.test(path)) {
    for(const match of text.matchAll(/\bimport\s+([\w$\s{},*]+?)\s+from\s*(['"])([^'"\n]+)\2\s*;?/g)) {
      if(!/^import\b/.test(clean.slice(match.index))) continue;
      const clause=match[1].trim(), specifier=match[3];
      if(/^type\s/.test(clause)) continue;
      const target=specifier.startsWith('.') ? resolvePath(path,specifier,paths) : null;
      const named=/{([^}]*)}/.exec(clause);
      if(named) for(const item of named[1].split(',')) {
        const value=item.trim();if(/^type\s/.test(value)) continue;
        const binding=/^([A-Za-z_$][\w$]*)(?:\s+as\s+([A-Za-z_$][\w$]*))?$/.exec(value);
        if(binding) add(match,{kind:'named',local:binding[2]||binding[1],imported:binding[1],target,specifier});
      }
      const namespace=/(?:^|,)\s*\*\s+as\s+([A-Za-z_$][\w$]*)\s*$/.exec(clause);
      if(namespace) add(match,{kind:'namespace',local:namespace[1],imported:null,target,specifier});
      const defaultName=/^([A-Za-z_$][\w$]*)(?:\s*,|\s*$)/.exec(clause)?.[1];
      if(defaultName) add(match,{kind:'default',local:defaultName,imported:'default',target,specifier});
    }
  } else if(/\.rs$/.test(path)) {
    for(const match of clean.matchAll(/\buse\s+([^;]+);/g)) {
      function visit(part, prefix=[]) {
        const brace=part.indexOf('{');
        if(brace>=0) {
          const end=part.lastIndexOf('}');if(end<brace || part.slice(end+1).trim()) return;
          const head=part.slice(0,brace).trim().replace(/::$/,'');
          const next=prefix.concat(head ? head.split('::').map(s=>s.trim()) : []);
          for(const inner of splitUse(part.slice(brace+1,end))) visit(inner,next);
          return;
        }
        const alias=part.split(/\s+as\s+/);if(alias.length>2) return;
        const names=prefix.concat(alias[0].split('::').map(s=>s.trim()));
        if(!names.every(n=>IDENT.test(n)) || (alias[1] && !IDENT.test(alias[1].trim()))) return;
        const own=names[0]==='crate'||names[0]===crateName, relative=names.slice(1);
        if(!relative.length) return;
        const self=relative.at(-1)==='self';
        const module=self ? relative.slice(0,-1) : relative;
        const direct=own ? rustPath(module,paths) : null;
        const kind=self||direct ? 'namespace' : 'named';
        const target=own ? direct||rustPath(relative.slice(0,-1),paths) : null;
        const imported=kind==='namespace' ? null : relative.at(-1);
        const local=alias[1]?.trim() || (self ? relative.at(-2) : relative.at(-1));
        if(local) add(match,{kind,local,imported,target,specifier:names.join('::')});
      }
      visit(match[1].trim());
    }
  } else if(/\.py$/.test(path)) {
    for(const match of clean.matchAll(/^[ \t]*from\s+([.\w]+)\s+import\s+(\([^)]*\)|[^\n]+)/gm)) {
      const module=match[1], relative=/^(\.+)([\w.]*)$/.exec(module);
      const specifier=relative ? './'+'../'.repeat(relative[1].length-1)+relative[2].replaceAll('.','/') : module;
      const list=match[2].replace(/^\(|\)$/g,'');
      for(const item of list.split(',')) {
        const binding=/^\s*([A-Za-z_]\w*)(?:\s+as\s+([A-Za-z_]\w*))?\s*$/.exec(item);if(!binding) continue;
        const namespace=relative && !relative[2];
        const target=relative ? resolvePath(path,specifier+(namespace ? binding[1] : ''),paths) : null;
        add(match,{kind:namespace?'namespace':'named',local:binding[2]||binding[1],imported:namespace?null:binding[1],target,specifier});
      }
    }
    // 絶対importの探索ルートは不明なので、外部の束縛として残す。
    for(const match of clean.matchAll(/^[ \t]*import\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)(?:\s+as\s+([A-Za-z_]\w*))?[ \t]*(?:$|#)/gm))
      add(match,{kind:'namespace',local:match[2]||match[1].split('.')[0],imported:null,target:null,specifier:match[1]});
  }
  return result;
}
