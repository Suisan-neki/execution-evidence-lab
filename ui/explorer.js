import { scanPublicRepository, readSource, analyze, codeLink, validateSnapshot } from "/ui/scanner.js";

const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
let snapshot, model, selectedPath, selectedLine = 1, hasCase = false, busy = false;

function notice(text, error = false) { $("notice").hidden = false; $("notice").className = error ? "error" : ""; $("notice").textContent = text; }
function tab(name) {
  for (const button of document.querySelectorAll("[data-tab]")) {
    const selected = button.dataset.tab === name;
    button.setAttribute("aria-selected", String(selected)); button.tabIndex = selected ? 0 : -1;
    $(button.getAttribute("aria-controls")).hidden = !selected;
  }
}
function codeButton(path, line = 1, text = path) { return `<button type="button" data-path="${esc(path)}" data-line="${line}">${esc(text)}</button>`; }
function sourceLink(path, line = 1, label = "根拠のコード") { return `<a href="${esc(codeLink(snapshot, path, line))}" target="_blank" rel="noopener noreferrer">${esc(label)}</a>`; }
function authorDescription(file) {
  const lines = file.text.split("\n");
  const comments = [];
  for (let i = 0; i < Math.min(lines.length, 12); i++) {
    if (/^\s*\/\/[/!]?(\s|$)/.test(lines[i])) comments.push({ text: lines[i].replace(/^\s*\/\/[/!]?\s?/, ""), line: i + 1 });
    else if (lines[i].trim() && !/^\s*[#!]/.test(lines[i])) break;
  }
  return comments;
}
function refresh() {
  model = analyze(snapshot);
  $("welcome").hidden = true; $("workspace").hidden = false;
  $("repo-name").textContent = snapshot.fullName;
  $("repo-description").textContent = snapshot.description;
  $("scan-scope").textContent = `コミット ${snapshot.commit.slice(0, 12)} · ${Object.keys(snapshot.files).length}ファイル読取済み / ${snapshot.inventory.length}ファイル取得 · ${snapshot.reference}`;
  $("coverage-note").textContent = snapshot.truncated
    ? "GitHubからのファイル一覧が途中で切れています。全体の構成としては未確認です。"
    : "固定したコミットの一部を読んでいます。未読ファイルや動的な処理はこの説明に含めていません。";
  const readme = model.files.find(f => /^readme[^/]*\.md$/i.test(f.path)) || model.files.find(f => /readme/i.test(f.path));
  $("readme").textContent = readme?.text || "READMEはまだ読み取っていません。";
  const paragraph = readme?.text.split(/\n\s*\n/).find(p => p.trim() && !/^\s*(#|```|\||!\[|<)/.test(p)) || "READMEの説明を抽出できませんでした。全文やGitHubのコードを確認してください。";
  $("readme-summary").innerHTML = `<p class="readme-paragraph">${esc(paragraph.slice(0, 800))}</p>${readme ? sourceLink(readme.path, 1, "説明の出所：" + readme.path) : ""}`;
  $("entry-list").innerHTML = model.entries.map(e => `<div class="entry">${codeButton(e.path, e.line)}<p>${esc(e.reason)} · ${e.line}行</p></div>`).join("") || `<p class="muted">取得したコードでは入口を特定できませんでした。READMEや起動設定からファイルを選んでください。</p>`;
  $("script-list").innerHTML = model.scripts.map(s => `<div class="script-row"><strong>${esc(s.name)}</strong><pre>${esc(s.command)}</pre>${sourceLink(s.path, s.line, `${s.path}:${s.line}`)}</div>`).join("") || `<p class="muted">package.jsonのscriptsは見つかりませんでした。Rustなどの起動方法はREADMEと設定ファイルを確認してください。</p>`;
  const folders = new Map();
  for (const f of snapshot.inventory) { const name = f.path.includes("/") ? f.path.split("/")[0] + "/" : "ルート"; if (!folders.has(name)) folders.set(name, []); folders.get(name).push(f); }
  $("folder-list").innerHTML = [...folders].map(([name, files]) => `<div class="folder"><button type="button" data-folder="${esc(name === "ルート" ? "" : name)}">${esc(name)}</button><p>${files.length}ファイル · ${files.filter(f => snapshot.files[f.path] !== undefined).length}読取済み</p></div>`).join("");
  $("omitted-files").innerHTML = `<p class="muted">初回に読み取らなかった候補：${snapshot.omitted.length}件</p><ul>${snapshot.omitted.map(f => `<li>${esc(f.path)}：${esc(f.reason)}</li>`).join("")}</ul>`;
  renderFiles();
  if (selectedPath && snapshot.files[selectedPath] !== undefined) renderSource();
  else {
    $("file-name").textContent = "読むファイルを選んでください";
    for (const id of ["file-role", "file-purpose", "file-links", "symbol-list", "outgoing", "incoming", "selected-line", "source"]) $(id).replaceChildren();
    $("to-test").disabled = true;
    $("bridge-note").textContent = "コードを選ぶと、その出所を確認事項へ引き継げます。";
  }
}
function renderFiles() {
  if (!snapshot) return;
  const query = $("file-search").value.toLowerCase();
  const files = snapshot.inventory.filter(f => f.path.toLowerCase().includes(query));
  $("file-count").textContent = `${files.length}件${files.length > 300 ? "（先頭300件を表示。名前で絞れます）" : ""}`;
  $("file-list").innerHTML = files.slice(0, 300).map(f => `<button class="file-button ${f.path === selectedPath ? "selected" : ""}" type="button" data-path="${esc(f.path)}" data-line="1">${esc(f.path)}<small>${snapshot.files[f.path] !== undefined ? "読取済み" : "未読・選ぶと読取"}</small></button>`).join("");
}
function renderSource() {
  const file = model.files.find(f => f.path === selectedPath);
  if (!file) return;
  $("file-name").textContent = file.path; $("file-role").textContent = file.role;
  const comments = authorDescription(file);
  $("file-purpose").innerHTML = comments.length ? `<div class="source-note"><p>${esc(comments.map(c => c.text).join("\n"))}</p>${sourceLink(file.path, comments[0].line, "作者のコメントを見る")}</div>` : `<p class="muted">目的を説明する先頭コメントは抽出できませんでした。宣言とコードを読んで確認してください。</p>`;
  $("file-links").innerHTML = sourceLink(file.path, selectedLine, "固定した版をGitHubで開く");
  $("symbol-list").innerHTML = `<h3>関数の宣言へ移動する</h3><div class="symbols">${file.symbols.map(s => codeButton(file.path, s.line, `${s.name} · ${s.line}行`)).join("") || '<p class="muted">対応する関数宣言は抽出できませんでした。</p>'}</div>`;
  $("outgoing").innerHTML = file.imports.map(e => `<div class="link-row">${codeButton(e.to)}${sourceLink(e.from, e.line, `参照宣言 · ${e.line}行`)}</div>`).join("") || '<p class="muted">この簡易解析で解決できた参照先はありません。参照がないと断定した結果ではありません。</p>';
  $("incoming").innerHTML = model.edges.filter(e => e.to === file.path).map(e => `<div class="link-row">${codeButton(e.from, e.line)}${sourceLink(e.from, e.line, `${e.line}行`)}</div>`).join("") || '<p class="muted">読取済みのファイルから解決できた参照宣言はありません。</p>';
  $("selected-line").textContent = `${selectedLine}行を選択`;
  $("source").innerHTML = file.text.split("\n").map((line, i) => `<div class="code-line ${i + 1 === selectedLine ? "target" : ""}" id="source-line-${i + 1}"><span class="line-number">${i + 1}</span><code>${esc(line)}</code></div>`).join("");
  // ページ全体を跳ばさずコード領域だけをスクロールする。
  const target = $("source-line-" + selectedLine);
  if (target) $("source").scrollTop = Math.max(0, target.offsetTop - $("source").firstElementChild.offsetTop - 40);
  $("to-test").disabled = !hasCase;
  $("bridge-note").textContent = hasCase ? "確認事項はまだ未検証として追加します。読んだだけでテスト済みにはしません。" : "確認事項の保存には事例を開く必要があります。cargo run -- review CASE_DIRで起動した画面から使えます。";
}
async function selectFile(path, line = 1) {
  if (busy) return;
  busy = true;
  try {
    if (snapshot.files[path] === undefined) { notice(`${path}を読み取っています`); await readSource(snapshot, path); refresh(); }
    selectedPath = path;
    selectedLine = Math.max(1, Math.min(Number(line) || 1, snapshot.files[path].split("\n").length));
    tab("code"); renderFiles(); renderSource();
  } catch (e) { notice(e.message, true); } finally { busy = false; }
}
$("scan-form").addEventListener("submit", async event => {
  event.preventDefault(); if (busy) return;
  busy = true; $("scan").disabled = true;
  try {
    const result = await scanPublicRepository($("repository-url").value, $("repository-ref").value, notice);
    snapshot = result; selectedPath = null; selectedLine = 1; $("file-search").value = "";
    refresh(); tab("overview");
    notice(`${Object.keys(snapshot.files).length}ファイルを読み取りました。まず説明と読む入口を確認してください。`);
  } catch (e) { notice(e.message, true); } finally { busy = false; $("scan").disabled = false; }
});
$("file-search").addEventListener("input", renderFiles);
document.addEventListener("click", event => {
  const target = event.target.closest("button"); if (!target) return;
  if (target.dataset.path) selectFile(target.dataset.path, target.dataset.line);
  else if (target.dataset.folder !== undefined) { $("file-search").value = target.dataset.folder; tab("code"); renderFiles(); $("file-search").focus(); }
  else if (target.dataset.tab) tab(target.dataset.tab);
});
document.querySelector(".reader-tabs").addEventListener("keydown", event => {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  const buttons = [...document.querySelectorAll("[data-tab]")];
  const current = buttons.indexOf(document.activeElement); if (current < 0) return;
  const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (current + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length;
  event.preventDefault(); tab(buttons[next].dataset.tab); buttons[next].focus();
});
$("to-test").addEventListener("click", () => {
  if (!hasCase || !selectedPath) return;
  const symbol = model.files.find(f => f.path === selectedPath)?.symbols.find(s => s.line === selectedLine);
  const data = { source: codeLink(snapshot, selectedPath, selectedLine), subject: `${snapshot.fullName} / ${selectedPath}${symbol ? " / " + symbol.name : ""}` };
  location.href = "/review#code=" + encodeURIComponent(JSON.stringify(data));
});
$("download").addEventListener("click", () => {
  const blob = new Blob([JSON.stringify(snapshot, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob), link = document.createElement("a");
  link.href = url; link.download = `repository-${snapshot.fullName.replace("/", "-")}-${snapshot.commit.slice(0, 12)}.json`;
  link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
});
$("snapshot-file").addEventListener("change", async event => {
  const file = event.target.files[0]; if (!file || busy) return;
  try {
    if (file.size > 8 * 1024 * 1024) throw Error("読取記録は8MiB以内で選んでください。");
    snapshot = validateSnapshot(JSON.parse(await file.text())); selectedPath = null; selectedLine = 1; $("file-search").value = ""; refresh(); tab("overview");
    notice("保存された読取記録を開きました。現在のGitHubと照合した結果ではありません。");
  } catch (e) { notice(e.message, true); } finally { event.target.value = ""; }
});
fetch("/api/case").then(r => { hasCase = r.ok; $("test-nav").href = hasCase ? "/review" : "#test-help"; if (!hasCase) $("test-nav").addEventListener("click", e => { e.preventDefault(); notice("テスト記録を開くには cargo run -- case init CASE_DIR の後に cargo run -- review CASE_DIR で起動してください。"); }); }).catch(() => {});
