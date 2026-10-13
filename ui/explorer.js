import { scanPublicRepository, readSource, analyze, codeLink, validateSnapshot } from "/ui/scanner.js";

import { sourceReference, sourceURL, validateCodeReference, explainFunction, compareSnapshots, lineChanges, cacheSnapshot, cachedSnapshot } from '/ui/comprehension.js';
let comparison, comparisonBefore, comparisonAfter, linkedCheckId = '', caseState;
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
    for (const id of ["file-role", "file-purpose", "file-links", "symbol-list", "outgoing", "incoming", "selected-line", "source", "function-guide"]) $(id).replaceChildren();
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
  renderGuide(file);
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
  cacheSnapshot(snapshot, sessionStorage);
  const data = { source: codeLink(snapshot, selectedPath, selectedLine), reference: currentSource(), subject: `${snapshot.fullName} / ${selectedPath}${symbol ? " / " + symbol.name : ""}` };
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
fetch("/api/case").then(async r => { hasCase = r.ok; if (r.ok) caseState = await r.json(); $("test-nav").href = hasCase ? "/review" : "#test-help"; if (!hasCase) $("test-nav").addEventListener("click", e => { e.preventDefault(); notice("テスト記録を開くには cargo run -- case init CASE_DIR の後に cargo run -- review CASE_DIR で起動してください。"); }); }).then(restoreSource).catch(e => notice(e.message, true));

function currentSource() {
  const guide = explainFunction(model, selectedPath, selectedLine);
  // 選択行を入口として保持し、関数末尾までの範囲を残す。
  return sourceReference(snapshot, selectedPath, selectedLine, guide?.range.end || selectedLine);
}
function renderGuide(file) {
  const guide = explainFunction(model, file.path, selectedLine);
  if (!guide) { $('function-guide').innerHTML = '<p class="muted">関数を選ぶと、入力・分岐・呼び出しの記述をたどれます。対応する関数範囲を特定できない行では案内を作りません。</p>'; return; }
  $('function-guide').innerHTML = `<section class="guide"><h3>${esc(guide.name)}の処理を読む</h3><p>入力・型・戻り値は、この宣言で確認できます。</p><pre class="guide-signature">${esc(guide.signature)}</pre><p class="muted">${guide.range.start}〜${guide.range.end}行の範囲。下の項目はコードの行順です。</p><ul>${guide.steps.map(step => `<li>${esc(step.text)} ${codeButton(file.path, step.line, step.line + '行を見る')}</li>`).join('') || '<li>対応する分岐・繰り返し・戻りの記述は抽出できませんでした。</li>'}</ul><h3>呼び出しの記述をたどる</h3>${guide.calls.map(call => `<div class="link-row"><code>${esc(call.name)}${call.macro ? '!' : ''}</code> ${codeButton(file.path, call.line, call.line + '行')}${call.candidates.length ? call.candidates.map(c => codeButton(c.path, c.line, '候補：' + c.path + ' / ' + c.name)).join('') : '<span class="muted">対応先は未解決</span>'}</div>`).join('') || '<p class="muted">対応する呼び出しの記述は抽出できませんでした。</p>'}<p class="muted">${esc(guide.limits)}</p>${guide.truncated ? '<p class="muted">先頭80件まで表示しています。</p>' : ''}</section>`;
}
async function restoreSource() {
  if (!location.hash.startsWith('#source=')) return;
  const request = JSON.parse(decodeURIComponent(location.hash.slice(8)));
  const source = validateCodeReference(request.source);
  if (typeof request.check_id !== 'string') throw Error('確認事項の識別子を確認してください。');
  linkedCheckId = request.check_id;
  $('repository-url').value = 'https://github.com/' + source.repository;
  $('repository-ref').value = source.commit;
  snapshot = cachedSnapshot(source, sessionStorage);
  if (!snapshot) snapshot = await scanPublicRepository($('repository-url').value, source.commit, notice);
  refresh(); await selectFile(source.path, source.line);
  if (linkedCheckId && caseState?.workspace.checks.some(c => c.id === linkedCheckId)) {
    $('code-update').hidden = false;
    $('linked-check').textContent = '確認事項：' + caseState.workspace.checks.find(c => c.id === linkedCheckId).text;
    $('back-check').href = '/review#check-' + encodeURIComponent(linkedCheckId);
  }
  notice('確認事項が参照する固定版のコードを開きました。試行で実行したコードとの一致は別途確認が必要です。');
}
$('compare-form').addEventListener('submit', async event => {
  event.preventDefault(); if (busy || !snapshot) return;
  busy = true; $('compare-version').disabled = true;
  try {
    const newer = await scanPublicRepository('https://github.com/' + snapshot.fullName, $('compare-ref').value, notice);
    comparisonBefore = snapshot; comparisonAfter = newer; comparison = compareSnapshots(snapshot, newer);
    renderComparison(); notice('版を比較しました。変更の影響と再試験の必要性を確認してください。');
  } catch (e) { notice(e.message, true); } finally { busy = false; $('compare-version').disabled = false; }
});
function renderComparison() {
  const labels = { changed: '変更あり', added: '追加', removed: '削除', unchanged: '同じ', unknown: '分からない' };
  const counts = Object.entries(labels).map(([status, label]) => label + ' ' + comparison.entries.filter(e => e.status === status).length + '件').join('・');
  const entries = comparison.entries.filter(e => e.status !== 'unchanged');
  $('comparison-result').innerHTML = `<p>${esc(comparison.before.slice(0, 12))} → ${esc(comparison.after.slice(0, 12))}</p><p>${esc(counts)}</p><button type="button" data-compared-overview>比較した版を読む</button><p class="muted">${esc(comparison.limits)}</p>${entries.slice(0, 200).map(e => {
    const affected = caseState?.workspace.checks.filter(c => c.source?.repository === comparison.repository && c.source.path === e.path) || [];
    const diff = lineChanges(comparisonBefore.files[e.path], comparisonAfter.files[e.path]);
    return `<div class="version-row"><strong>${esc(labels[e.status])}：${esc(e.path)}</strong><p>${esc(e.basis)}</p>${affected.length ? `<p>このファイルを参照する確認事項：${affected.map(c => `<a href="/review#check-${esc(c.id)}">${esc(c.text)}</a>`).join('、')}</p>` : ''}${comparisonAfter.inventory.some(f => f.path === e.path) ? `<button data-version-path="${esc(e.path)}">比較した版のコードを開く</button>` : ''}${diff ? `<details><summary>異なる範囲のコードを見る</summary><p class="muted">前後で一致する先頭と末尾を除いた範囲です。行ごとの最小差分ではありません。</p><div class="diff-grid"><div><h3>以前の版</h3><pre>${esc(diff.before.map(l => l.line + ': ' + l.text).join('\n'))}</pre></div><div><h3>比較した版</h3><pre>${esc(diff.after.map(l => l.line + ': ' + l.text).join('\n'))}</pre></div></div>${diff.truncated ? '<p>先頭120行まで表示しています。</p>' : ''}</details>` : '<p class="muted">本文の比較には両方のコードの読取が必要です。</p>'}</div>`;
  }).join('')}${entries.length > 200 ? '<p>先頭200件まで表示しています。全体の比較は読取記録で確認してください。</p>' : ''}`;
}
document.addEventListener('click', async event => {
  const target = event.target.closest('button[data-version-path],button[data-compared-overview]'); if (!target || busy || !comparisonAfter) return;
  snapshot = comparisonAfter; selectedPath = null; selectedLine = 1; refresh();
  if (target.dataset.versionPath) await selectFile(target.dataset.versionPath);
  else tab('overview');
  notice('比較した版のコードを開きました。確認事項の参照先は、更新するまで以前の版のままです。');
});
$('bind-source').addEventListener('click', async () => {
  if (!linkedCheckId || !snapshot || !selectedPath || busy) return;
  const actor = $('binding-actor').value.trim(), reason = $('binding-reason').value.trim();
  if (!actor || !reason) { notice('記入者と更新する理由を入力してください。', true); return; }
  busy = true; $('bind-source').disabled = true;
  try {
    const response = await fetch('/api/case');
    if (!response.ok) throw Error('確認事項を読み取れませんでした。');
    const current = await response.json();
    // 画面を開いた後の変更を上書きしない。最新の版は競合検査のためだけに使わない。
    if (caseState.workspace.revision !== current.workspace.revision) { caseState = current; throw Error('事例が更新されています。内容をテスト画面で確認してから再度更新してください。'); }
    const token = document.querySelector('meta[name=review-token]').content;
    if (!token) throw Error('操作トークンを取得できませんでした。');
    const result = await fetch('/api/change', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Review-Token': token }, body: JSON.stringify({ expected_revision: caseState.workspace.revision, actor, reason, action: { op: 'bind_source', check_id: linkedCheckId, source: currentSource() } }) });
    const body = await result.json(); if (!result.ok) throw Error(body.error);
    caseState = body; cacheSnapshot(snapshot, sessionStorage);
    notice('参照先と理由を保存しました。以前の結果は履歴として残り、現在の条件には再確認が必要です。');
  } catch (e) { notice(e.message, true); } finally { busy = false; $('bind-source').disabled = false; }
});
