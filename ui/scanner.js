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
  const inventory = tree.tree.filter(f => f.type === "blob").map(({ path, size, mode }) => ({ path, size, mode }));
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

// コメント・文字列の中の宣言や呼び出しを候補にしない。文字数と改行位置は保持する。
// 完全な構文解析器ではない。Rustのraw文字列や各言語の特殊構文は未対応。
export function maskCode(text, rust = false) {
  const pattern = rust
    ? /\/\*[\s\S]*?\*\/|\/\/[^\n]*|"(?:\\[\s\S]|[^"\\])*"|'(?:\\.|[^'\\\n])'/g
    : /\/\*[\s\S]*?\*\/|\/\/[^\n]*|#[^\n]*|"(?:\\[\s\S]|[^"\\])*"|'(?:\\[\s\S]|[^'\\])*'|`(?:\\[\s\S]|[^`\\])*`/g;
  return text.replace(pattern,
    match => match.replace(/[^\n]/g, " "));
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
  return [base, ...[".rs", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx", ".py", "/mod.rs", "/index.js", "/index.ts", "/index.tsx", "/__init__.py"].map(ext => base + ext)].find(p => paths.has(p)) || null;
}
function symbols(path, text) {
  if (!SOURCE.test(path)) return [];
  const masked = maskCode(text, /\.rs$/.test(path)), lines = masked.split("\n"), found = [];
  const pattern = /(?:^|\s)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][\w]*)\s*\(|(?:^|\s)(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(|^\s*(?:async\s+)?def\s+([A-Za-z_][\w]*)\s*\(|(?:^|\s)(?:export\s+)?(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=.*=>/;
  lines.forEach((line, i) => { const m = pattern.exec(line); if (m) found.push({ name: m.slice(1).find(Boolean), line: i + 1 }); });
  return found.map((s, i) => ({ ...s, end: (found[i + 1]?.line || lines.length + 1) - 1 }));
}
function imports(path, text, paths, crateName) {
  const edges = [];
  const clean = maskCode(text, /\.rs$/.test(path));
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
  const files = Object.entries(snapshot.files).map(([path, text]) => ({ path, text, symbols: symbols(path, text), imports: imports(path, text, paths, crateName), role: role(path) }));
  const entries = files.filter(f => /(^|\/)(main\.(rs|py)|server\.[cm]?[jt]s|app\.[jt]sx?|index\.[cm]?[jt]sx?)$/.test(f.path)).map(f => ({ path: f.path, line: f.symbols.find(s => s.name === "main")?.line || 1, reason: /main\.rs$/.test(f.path) ? "Rustの実行入口に使われるファイル" : "名前から選んだ入口の候補" }));
  const scripts = [];
  for (const f of files.filter(f => /(^|\/)package\.json$/.test(f.path))) {
    try {
      const pkg = JSON.parse(f.text);
      for (const [name, command] of Object.entries(pkg.scripts || {}))
        if (typeof command === "string") scripts.push({ path: f.path, name, command, line: f.text.split("\n").findIndex(l => l.includes(JSON.stringify(name))) + 1 });
    } catch { /* 読めないmanifestを成功した起動設定としない。 */ }
  }
  return { files, entries, scripts, edges: files.flatMap(f => f.imports) };
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
