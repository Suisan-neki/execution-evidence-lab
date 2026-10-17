"use strict";
const token = "__REVIEW_TOKEN__";
let state, pendingSource, codeHelpers;
const codeTools = import('/ui/comprehension.js');
let browserEvidence = () => '';
import('/ui/browser-evidence.js').then(module => { browserEvidence = module.renderBrowserEvidence; module.installBrowserScreenshots(token); if (state) render(); }).catch(error => message(error.message, true));
let renderedChecks = [];
const recipeBases = new Map();
let executionState = { enabled: false }, executionPollTimer;
const $ = (id) => document.getElementById(id);
const esc = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const labels = {
  satisfied: "条件を満たす",
  not_satisfied: "条件を満たさない",
  unknown: "判定できない",
  developer: "開発者",
  operator: "運用担当",
  other: "その他・模擬的な確認",
  release: "公開・導入すると判断",
  hold: "保留",
  modify: "改修",
  verify: "追加確認",
  ask: "質問",
  statement: "認識・発言",
  code_fact: "コード上の事実",
  proposal: "提案",
  unanswered: "未回答",
};
function message(text, error = false) {
  $("message").textContent = text;
  $("message").className = error ? "error" : "";
  $("message").style.display = "block";
}
function common() {
  if (!$("actor").value.trim() || !$("reason").value.trim())
    throw Error("記入者と今回の理由を入力してください");
  return {
    expected_revision: state.workspace.revision,
    actor: $("actor").value.trim(),
    reason: $("reason").value.trim(),
  };
}
async function post(path, data) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Review-Token": token },
    body: JSON.stringify(data),
  });
  const body = await response.json();
  if (!response.ok) throw Error(body.error || "保存できませんでした");
  state = body;
  render();
  message("記録しました。版 " + state.workspace.revision + "。");
}
async function act(action) {
  await post("/api/change", { ...common(), action });
}
async function load() {
  // 完了状態を読んでから事例を読む。取り込み前の事例と完了表示を混ぜない。
  const statusResponse = await fetch("/api/execution");
  if (!statusResponse.ok) throw Error("実行状態を読み取れません。最新版を読み直してください。");
  executionState = await statusResponse.json();
  const response = await fetch("/api/case");
  const body = await response.json();
  if (!response.ok) throw Error(body.error);
  codeHelpers = await codeTools;
  state = body;
  render();
  scheduleExecutionPoll();
}
function button(id, fn) {
  $(id).addEventListener("click", async (event) => {
    event.preventDefault();
    const b = event.currentTarget;
    b.disabled = true;
    try {
      await fn();
    } catch (e) {
      message(e.message, true);
    } finally {
      b.disabled = false;
    }
  });
}
function evidenceLinks(evidence) {
  return evidence.observed.evidence
    .map((ref) => {
      const match = /^(adapter-)?events\.jsonl#(\d+)$/.exec(ref);
      const observation = /^observations\.jsonl#(\d+)$/.exec(ref);
      const reported = /^reported-observations#(\d+)$/.exec(ref);
      const target = reported ? `evidence-${evidence.id}-reported-${reported[1]}` : observation ? `evidence-${evidence.id}-observation-${observation[1]}` : match
        ? `evidence-${evidence.id}-${match[1] ? "adapter" : "events"}-${match[2]}`
        : `evidence-${evidence.id}`;
      return `<a href="#${esc(target)}">${esc(ref)}</a>`;
    })
    .join("、");
}
function originMarkup(origin) {
  try {
    const url = new URL(origin);
    if (url.protocol === "https:" && url.hostname === "github.com" && !url.username && !url.password)
      return `<a href="${esc(url.href)}" target="_blank" rel="noopener noreferrer">${esc(origin)}</a>`;
  } catch { /* 自由記述の出所は文字列として表示する。 */ }
  return esc(origin);
}
function checkReference(check) {
  if (check.source) return check.source;
  try { return codeHelpers?.referenceFromURL(check.origin); } catch { return null; }
}
function codeMarkup(source, checkId, label = '理解画面でコードを読む', note = '確認事項の参照コードです。固定版の実行記録がある場合は、根拠で版と起動ファイルを照合できます。') {
  if (!source) return '';
  const payload = encodeURIComponent(JSON.stringify({ source, check_id: checkId }));
  return `<p class="code-reference"><a href="/#source=${payload}">${esc(label)}</a><br><span class="muted">${esc(source.repository)} / ${esc(source.path)}：${source.line}〜${source.end_line}行・${esc(source.commit.slice(0, 12))}<br>${esc(note)}</span></p>`;
}
function executionSetup(check) {
  if (!check.source) return '';
  const id = esc(check.id), criterion = check.observation;
  const ready = !!criterion && check.rule === "observed_assertion";
  const executeButton = executionState.enabled && executionState.repository === check.source.repository
    ? `<button class="execute-recipe" data-check="${id}" data-ready="${ready}" ${ready ? "" : "disabled"}>この固定版で試験を実行する</button><p class="muted">入力した起動設定と入力を使って実行します。記入者と理由を上で入力してください。</p>`
    : executionState.enabled ? '<p class="muted">画面からの実行対象と、この条件のリポジトリが異なります。</p>' : "";
  const trials = state.workspace.evidence.filter(e =>
    e.check_id === check.id && e.execution?.recipe.source.repository === check.source.repository,
  );
  const reuse = trials.length ? `
    <label for="prior-trial-${id}">設定を読み込む過去の試行</label>
    <select id="prior-trial-${id}">${trials.map(e => `<option value="${esc(e.id)}">${esc(e.id)}・${esc(e.execution.recipe.source.commit.slice(0,12))}・条件版 ${e.check_version}</option>`).join('')}</select>
    <button class="secondary load-prior-recipe" data-check="${id}">過去の設定を入力欄へ読み込む</button>
    <p class="muted">入力欄の言語・起動ファイル・引数・入力・待機上限を置き換えます。新しい試験設定には現在のコード版と観測条件を使います。</p>
    <p id="recipe-basis-${id}" class="callout" hidden></p>` : '';
  return `<details class="external-setup"><summary>このコードの試験を設定する</summary>
    <p>条件に対応する観測名と対象を指定し、固定版を動かす設定を保存します。対象アプリは観測形式に対応している必要があります。</p>
    <label for="assertion-${id}">判定に使う観測名</label>
    <input id="assertion-${id}" value="${esc(criterion?.assertion || '')}" placeholder="例：stored_matches_input">
    <label for="subject-${id}">観測する対象の識別名</label>
    <input id="subject-${id}" value="${esc(criterion?.subject || '')}" placeholder="例：record-1">
    <button class="secondary configure-observation" data-check="${id}">観測名・対象を条件に対応づける</button>
    <p class="muted">対応づけを変えると以前の結果は再確認待ちになります。実行・独立した状態確認は別の操作です。</p>
    ${reuse}
    <label for="runtime-${id}">実行言語</label>
    <select id="runtime-${id}"><option value="node" ${check.source.path.endsWith('.py') ? '' : 'selected'}>Node.js</option><option value="python3" ${check.source.path.endsWith('.py') ? 'selected' : ''}>Python 3</option></select>
    <label for="entry-${id}">固定版の起動ファイル</label><input id="entry-${id}" value="${esc(check.source.path)}">
    <label for="args-${id}">起動引数・JSON配列</label><textarea id="args-${id}">[]</textarea>
    <label for="input-${id}">試験入力・JSON</label><textarea id="input-${id}">{}</textarea>
    <label for="timeout-${id}">待機上限・ミリ秒</label><input type="number" id="timeout-${id}" min="1" max="300000" value="5000">
    <p class="muted">保存するコード版：${esc(check.source.commit)}。観測名・対象は上で対応づけた現在の条件を使います。起動ファイルがこの版にあるかは実行時に照合します。</p>
    ${executeButton}
    <button class="secondary download-recipe" data-check="${id}" ${criterion && check.rule === 'observed_assertion' ? '' : 'disabled'}>この条件の試験設定を保存</button>
    <p class="muted">設定を保存してCLIから実行することもできます。local方式はOSによる隔離がないため、自分で確認したコードを使ってください。</p>
    <pre>cargo run -- repo prepare NEW_REPOSITORY_DIR RECIPE_JSON
cargo run -- case execute CASE_DIR RECIPE_JSON --repository-root REPOSITORY_DIR --mode local --actor 記入者 --reason 試験理由</pre></details>`;
}
function loadPriorRecipe(checkId) {
  const check = state.workspace.checks.find(c => c.id === checkId);
  const evidence = state.workspace.evidence.find(e => e.id === $('prior-trial-' + checkId).value && e.check_id === checkId);
  const recipe = evidence?.execution?.recipe;
  if (!recipe || recipe.source.repository !== check.source.repository)
    throw Error('この条件とリポジトリに対応する実行記録が必要です。');
  const values = { runtime: recipe.runtime, entry: recipe.entrypoint,
    args: JSON.stringify(recipe.args, null, 2), input: JSON.stringify(recipe.input, null, 2), timeout: recipe.timeout_ms };
  for (const [prefix, value] of Object.entries(values)) $(prefix + '-' + checkId).value = value;
  recipeBases.set(checkId, evidence.id);
  showRecipeBasis(check, evidence);
  message('過去の試験設定を入力欄へ読み込みました。実行結果と判定は変わっていません。');
}
function showRecipeBasis(check, evidence) {
  const recipe = evidence.execution.recipe;
  const changes = [];
  if (recipe.source.commit !== check.source.commit) changes.push('コード版が変わっています');
  if (evidence.check_version !== check.version) changes.push('確認事項の版が変わっています');
  if (recipe.criterion.assertion !== check.observation?.assertion || recipe.criterion.subject !== check.observation?.subject)
    changes.push('観測名・対象が変わっています');
  const basis = $('recipe-basis-' + check.id);
  if (!basis) return;
  basis.hidden = false;
  basis.textContent = `${evidence.id}（${recipe.source.commit.slice(0,12)}・条件版 ${evidence.check_version}）の設定を読み込みました。${changes.length ? changes.join('。') + '。' : ''}保存先は現在のコード版 ${check.source.commit.slice(0,12)}・条件版 ${check.version}です。入力と起動設定がこの条件に使えるか確認してください。`;
}


function recipeFromForm(checkId) {
  const check = state.workspace.checks.find(c => c.id === checkId);
  if (!check.source || !check.observation || check.rule !== 'observed_assertion') throw Error('コード参照と観測名・対象を先に保存してください。');
  if ($('assertion-' + check.id).value.trim() !== check.observation.assertion || $('subject-' + check.id).value.trim() !== check.observation.subject) throw Error('変更した観測名・対象を先に保存してください。');
  const args = JSON.parse($('args-' + check.id).value), input = JSON.parse($('input-' + check.id).value);
  const entrypoint = $('entry-' + check.id).value.trim(), timeout = Number($('timeout-' + check.id).value);
  const bytes = value => new TextEncoder().encode(value).length;
  if (!Array.isArray(args) || args.length > 64 || args.some(a => typeof a !== 'string' || a.includes('\0') || bytes(a) > 16384) || !entrypoint || entrypoint.startsWith('/') || /[\\\x00-\x1f\x7f]/.test(entrypoint) || entrypoint.split('/').some(p => !p || p === '..' || p === '.' || p === '.git') || !Number.isInteger(timeout) || timeout < 1 || timeout > 300000) throw Error('起動ファイル・引数・待機上限を確認してください。');
  const recipe = { schema_version: 1, check_id: check.id, source: check.source, runtime: $('runtime-' + check.id).value, entrypoint, args, input, criterion: check.observation, timeout_ms: timeout };
  if (bytes(JSON.stringify(input)) > 128 * 1024 || bytes(JSON.stringify(recipe)) > 192 * 1024) throw Error('試験入力は128KiB、設定全体は192KiB以内にしてください。');
  return recipe;
}
function renderExecution() {
  $('execution-panel').hidden = !executionState.enabled;
  const job = executionState.job;
  if (executionState.enabled) {
    const status = job?.status === 'running' ? `${job.check_id}の試験を実行中です。終了後に結果を読み直します。`
      : job?.status === 'completed' ? `${job.check_id}の試験が終了し、結果を版 ${job.resulting_revision}へ取り込みました。各条件の判定と根拠を確認してください。`
      : job?.status === 'failed' ? `試験の実行または取り込みを完了できませんでした。${job.error}` : '試験を実行できます。';
    $('execution-status').innerHTML = `<p>対象：${esc(executionState.repository)}<br>方式：${esc(executionState.mode)}<br><span class="muted">${esc(executionState.repository_root)}</span></p><p>${esc(status)}</p><p class="muted">${executionState.mode === 'local' ? 'local方式はOSの隔離を行いません。確認したコードを試してください。' : 'bubblewrap方式を要求します。起動できない場合はlocalへ切り替えません。'}</p>`;
  }
  document.querySelectorAll('.execute-recipe').forEach(b => { b.disabled = job?.status === 'running' || b.dataset.ready !== 'true'; });
}
function scheduleExecutionPoll() {
  if (executionPollTimer || executionState.job?.status !== 'running') return;
  executionPollTimer = setTimeout(async () => {
    executionPollTimer = null;
    try {
      const response = await fetch('/api/execution');
      if (!response.ok) throw Error('試験の実行状態を読み取れません。最新版を読み直してください。');
      executionState = await response.json();
      if (executionState.job?.status !== 'running') {
        await load();
        message(executionState.job?.status === 'completed' ? '試験が終了し、新しい結果を読み込みました。判定と根拠を確認してください。' : executionState.job?.error || '試験の状態を確認してください。', executionState.job?.status === 'failed');
      } else { renderExecution(); scheduleExecutionPoll(); }
    } catch (error) { message(error.message, true); }
  }, 500);
}
async function startExternal(checkId) {
  const data = { ...common(), recipe: recipeFromForm(checkId) };
  try {
    const response = await fetch('/api/execute', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Review-Token': token }, body: JSON.stringify(data) });
    const body = await response.json();
    if (!response.ok) throw Error(body.error || '試験を開始できませんでした。');
    executionState.job = body;
    renderExecution(); scheduleExecutionPoll();
    message('試験を開始しました。終了後に新しい結果と根拠を表示します。');
  } catch (error) {
    // 応答が失われても、受け付けられた試験を二重に起動しないよう状態を読み直す。
    await load();
    throw error;
  }
}

function downloadJSON(name, value) {
  const url = URL.createObjectURL(new Blob([JSON.stringify(value, null, 2)], { type: 'application/json' }));
  const link = document.createElement('a'); link.href = url; link.download = name; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function render() {
  const w = state.workspace;
  // 再描画で、未保存の回答や選択した条件を消さない。
  const drafts = [
    ...$("checks").querySelectorAll("input[id],textarea[id],select[id]"),
  ]
    .filter((el) => {
      const c = renderedChecks.find((c) =>
        ["text-", "rule-", "deps-", "check-origin-"].some(
          (prefix) => el.id === prefix + c.id,
        ),
      );
      if (!c) return true;
      const old =
        el.id === "text-" + c.id
          ? c.text
          : el.id === "rule-" + c.id
            ? c.rule
            : el.id === "deps-" + c.id
              ? c.dependencies.join(",")
              : c.origin;
      return el.value !== old;
    })
    .map((el) => [el.id, el.value]);
  const choices = ["statement-checks", "decision-checks"].map((id) => [
    id,
    new Map(
      [...$(id).querySelectorAll("input")].map((el) => [el.value, el.checked]),
    ),
  ]);
  const details = [...$("checks").querySelectorAll("details")].map((el) => [
    el.closest("section").id,
    el.querySelector("summary").textContent,
    el.open,
  ]);
  $("case-title").textContent = w.title;
  $("revision").textContent = "事例 " + w.id + "・版 " + w.revision;
  $("overview").innerHTML =
    ["satisfied", "not_satisfied", "unknown"]
      .map(
        (kind) =>
          `<span class="badge ${kind}">${esc(labels[kind])} ${state.checks.filter((v) => v.verdict === kind).length}件</span>`,
      )
      .join(" ") +
    (w.checks.length
      ? `<p>未回答 ${state.checks.reduce((n, v) => n + v.unanswered.length, 0)}件／以前の根拠の再確認 ${state.checks.filter((v) => v.evidence.at(-1)?.applicable === false).length}件</p>`
      : "<p>確認事項はまだありません。「条件・実行環境を更新する」から、確かめたい条件と出所を追加してください。</p>");
  $("checks").innerHTML = state.checks
    .map((v) => {
      const c = w.checks.find((c) => c.id === v.check_id);
      const latest = v.evidence.at(-1);
      return `<section class="check" id="check-${esc(c.id)}"><div class="row"><h2>${esc(c.text)}</h2><span class="badge ${esc(v.verdict)}">${esc(labels[v.verdict])}</span></div><p class="muted">条件 ${esc(c.id)}・版 ${c.version}／出所：${originMarkup(c.origin)}</p>${codeMarkup(checkReference(c), c.id)}<p class="reason">${esc(v.reason)}</p><p><strong>次の確認の提案</strong><br>${esc(v.next.text)}<br><span class="muted">理由：${esc(v.next.reason)}。この提案から自動実行しません。</span></p>${latest ? `<p class="evidence-scope">${latest.applicable ? "現在の条件に対応する試行" : "以前の条件の試行。現在の判定には使えません"}：${esc(latest.run_id)}<br><a href="#evidence-${esc(latest.id)}">根拠と実行記録を見る</a><br>${evidenceLinks(latest)}</p>${latest.observed.unconfirmed.length ? `<div class="unconfirmed"><strong>この観測では確認していないこと</strong><ul>${latest.observed.unconfirmed.map((text) => `<li>${esc(text)}</li>`).join("")}</ul></div>` : ""}` : ""}${c.questions.map((q) => `<div><p><strong>${esc(q.text)}</strong>${q.answer ? `<br>${esc(q.answer)}<br><span class="muted">出所：${esc(q.origin)}</span>` : "<br>未回答"}</p><label for="answer-${esc(c.id)}-${esc(q.id)}">回答・修正する内容</label><input id="answer-${esc(c.id)}-${esc(q.id)}"><label for="origin-${esc(c.id)}-${esc(q.id)}">回答の出所</label><input id="origin-${esc(c.id)}-${esc(q.id)}"><button class="secondary answer" data-check="${esc(c.id)}" data-question="${esc(q.id)}">回答と出所を保存</button></div>`).join("")}${
        c.rule === "demo_save"
          ? `<details open><summary>合成データで新しい試行を行う</summary><p class="muted">架空の利用者1件をTCPで送信・ファイル保存します。選んだ故障条件を現在の試験条件として記録します。</p>${[
              ["normal", "正常"],
              ["save-failure", "保存前の人工失敗"],
              ["missing-observation", "保存の観測だけ欠落"],
              ["disconnect", "受信前の遮断"],
            ]
              .map(
                ([fault, label]) =>
                  `<button class="demo secondary" data-check="${esc(c.id)}" data-fault="${fault}">${label}</button>`,
              )
              .join("")}</details>`
          : ""
      }${executionSetup(c)}<details><summary>以前の結果と、現在の条件に使える範囲</summary>${v.evidence.map((e) => `<p>${esc(e.id)}：観測時は${esc(labels[e.observed.verdict])}／${e.applicable ? "現在の条件に対応" : "再確認が必要"}<br>${esc(e.invalidated_by.join("、"))}</p>`).join("") || "<p>根拠はまだありません。</p>"}</details><details><summary>確認事項を追加する</summary><label for="question-text-${esc(c.id)}">確認したいこと</label><input id="question-text-${esc(c.id)}"><button class="secondary add-question" data-check="${esc(c.id)}">未回答の確認事項を保存</button></details><details><summary>条件文・判定方法の更新</summary><p class="muted">既存の判定規則は定義済みの条件文だけに使えます。別の条件は「判定方法未定義」にしてください。版を更新すると、以前の根拠は履歴として残ります。条件文・判定規則を変えた場合は回答を再確認します。</p><label for="text-${esc(c.id)}">条件文</label><textarea id="text-${esc(c.id)}">${esc(c.text)}</textarea><label for="rule-${esc(c.id)}">判定規則</label><select id="rule-${esc(c.id)}">${[
        ["manual_only", "判定方法未定義"],
        ["observed_assertion", "固定版の観測名・対象"],
        ["reported_assertion", "外部から報告された観測名・対象"],
        ["demo_save", "デモのファイル書き込み・同期"],
        ["local_index_uploaded", "固定版アダプターの模擬索引更新"],
      ]
        .map(
          ([rule, label]) =>
            `<option value="${rule}" ${c.rule === rule ? "selected" : ""}>${label}</option>`,
        )
        .join(
          "",
        )}</select><label for="deps-${esc(c.id)}">依存先・カンマ区切り</label><input id="deps-${esc(c.id)}" value="${esc(c.dependencies.join(","))}"><label for="check-origin-${esc(c.id)}">条件の出所</label><input id="check-origin-${esc(c.id)}" value="${esc(c.origin)}"><button class="secondary revise" data-check="${esc(c.id)}">版と変更理由を保存</button></details></section>`;
    })
    .join("");
  $("statements").innerHTML = w.statements
    .map(
      (s) =>
        `<div class="history-item"><span class="badge">${esc(labels[s.kind])}</span> ${s.role ? esc(labels[s.role]) : "立場の申告なし"}<p>${esc(s.text)}</p><p class="muted">${esc(s.origin)}</p><p>関連する条件：${s.check_ids.map((id) => `<a href="#check-${esc(id)}">${esc(id)}</a>`).join("、") || "指定なし"}</p></div>`,
    )
    .join("");
  $("statement-checks").innerHTML = w.checks
    .map(
      (c) =>
        `<label><input type="checkbox" value="${esc(c.id)}">${esc(c.id)}</label>`,
    )
    .join("");
  $("decision-checks").innerHTML = w.checks
    .map(
      (c) =>
        `<label><input type="checkbox" value="${esc(c.id)}" checked>${esc(c.id)}</label>`,
    )
    .join("");
  $("decisions").innerHTML =
    [...w.decisions]
      .reverse()
      .map(
        (d) =>
          `<div class="history-item"><strong>${esc(labels[d.role])}・${esc(d.actor)}：${esc(labels[d.action])}</strong><p>${esc(d.reason)}</p><p class="muted">判断時の事例：版${d.based_on_revision}／確認事項：${esc(d.check_ids.join(", "))}／安心感：${d.confidence ?? "記録なし"}／時間：${d.elapsed_ms == null ? "記録なし" : d.elapsed_ms / 1000 + "秒"}</p>${d.change_reference ? `<p>改修への参照：${esc(d.change_reference)}</p>` : ""}<details><summary>判断時の根拠と未確認事項</summary><pre>${esc(JSON.stringify(d.basis, null, 2))}</pre></details><p class="muted">この記録は判断当時のものです。現在の適用範囲は上の確認事項で確認してください。</p></div>`,
      )
      .join("") || "<p>人による判断はまだ記録されていません。</p>";
  $("context").textContent = JSON.stringify(w.context, null, 2);
  $("context-keys").innerHTML = [
    ...new Set([
      ...Object.keys(w.context),
      "engine",
      "input",
      "environment",
      "fault",
      "target",
      "adapter",
      "command",
      "authorization",
      "configuration",
    ]),
  ]
    .map((k) => `<option value="${esc(k)}"></option>`)
    .join("");
  $("audit").innerHTML =
    "<table><thead><tr><th>版</th><th>記入者</th><th>操作</th><th>理由</th></tr></thead><tbody>" +
    w.history
      .map(
        (h) =>
          `<tr><td>${h.revision}</td><td>${esc(h.actor)}</td><td>${esc(h.operation)}</td><td>${esc(h.reason)}</td></tr>`,
      )
      .join("") +
    "</tbody></table>";
  $("evidence").innerHTML = w.evidence
    .map(
      (e) =>
        `<section id="evidence-${esc(e.id)}"><h3>${esc(e.id)}／試行 ${esc(e.manifest.correlation.run_id)}</h3><p>条件 ${esc(e.check_id)}・取り込み時の版 ${e.check_version}</p>${codeMarkup(e.source, e.check_id, "取り込み当時の参照コードを読む")}${e.reported ? `<p class="callout">外部からの報告観測：${esc(e.reported.producer)}／出所：${esc(e.reported.origin)}。対象コードの実行・独立検証を基盤が行った記録ではありません。</p>` : ""}${browserEvidence(e)}${e.execution ? `<div class="execution-target"><p><strong>固定版の実行記録</strong><br>${esc(e.execution.recipe.source.repository)}・${esc(e.execution.recipe.source.commit.slice(0,12))}<br>起動：${esc(e.execution.recipe.entrypoint)}／${esc(e.execution.runtime_version)}<br>方式：${esc(e.execution.mode)}／終了コード：${esc(e.execution.exit_code ?? "取得できない")}／固定版の終了時照合：${e.execution.snapshot_unchanged ? '一致' : '不一致'}</p><p class="muted">起動ファイルと観測の参照先が固定版に含まれることを照合しました。該当行の到達は対象プログラムの申告で、独立した計測ではありません。local方式の外部状態は隔離していません。bubblewrapはLinuxの隔離方式を要求し、起動できない場合はローカルへ切り替えません。</p><details><summary>対象プログラムの出力・エラー</summary><h4>標準出力・先頭8KiB</h4><pre>${esc(e.execution.stdout_excerpt)}</pre><h4>標準エラー・先頭8KiB</h4><pre>${esc(e.execution.stderr_excerpt)}</pre></details>${e.execution.diagnostics.length ? `<ul>${e.execution.diagnostics.map(d => `<li>${esc(d)}</li>`).join('')}</ul>` : ''}</div>` : ''}<table><thead><tr><th>根拠</th><th>観測した内容</th></tr></thead><tbody>${e.events.map((event) => `<tr id="evidence-${esc(e.id)}-events-${event.sequence}"><td>events.jsonl#${event.sequence}<br>${esc(event.source)}／${esc(event.kind)}</td><td>${esc(event.detail)}</td></tr>`).join("")}${(e.execution?.observations ?? []).map(o => `<tr id="evidence-${esc(e.id)}-observation-${o.sequence}"><td>observations.jsonl#${o.sequence}<br>${esc(o.assertion)}／${esc(o.subject)}<br>${esc(o.path)}：${o.line}行<br><a href="/#source=${encodeURIComponent(JSON.stringify({ source: { ...e.execution.recipe.source, path:o.path, line:o.line, end_line:o.line }, check_id:e.check_id }))}">観測の参照コードを読む</a></td><td>${esc(o.outcome === 'pass' ? '成功の観測' : '失敗の観測')}<br>${esc(o.detail)}</td></tr>`).join('')}${(e.reported?.observations ?? []).map(o => `<tr id="evidence-${esc(e.id)}-reported-${o.sequence}"><td>reported-observations#${o.sequence}<br>${esc(o.assertion)}／${esc(o.subject)}</td><td>${esc(o.outcome === "pass" ? "成功の報告" : "失敗の報告")}<br>${esc(o.detail)}${codeMarkup(o.source, e.check_id, "報告に付けた参照コードを読む", "報告者が対応づけた参照先です。到達の証明ではありません。")}</td></tr>`).join("")}${(e.adapter?.events ?? []).map((event) => `<tr id="evidence-${esc(e.id)}-adapter-${event.sequence}"><td>adapter-events.jsonl#${event.sequence}<br>${esc(event.kind)}</td><td>${esc(typeof event.detail === "string" ? event.detail : JSON.stringify(event.detail))}</td></tr>`).join("")}</tbody></table><details><summary>条件・コード・入力・環境の記録</summary><pre>${esc(JSON.stringify({ manifest: e.manifest, dependencies: e.dependencies, target: e.adapter?.target, execution: e.execution, reported: e.reported }, null, 2))}</pre></details></section>`,
    )
    .join("");
  for (const [id, value] of drafts) {
    const el = $(id);
    if (el) el.value = value;
  }
  for (const [id, selected] of choices)
    for (const el of $(id).querySelectorAll("input"))
      if (selected.has(el.value)) el.checked = selected.get(el.value);
  for (const [id, summary, open] of details)
    for (const el of $(id)?.querySelectorAll("details") ?? [])
      if (el.querySelector("summary").textContent === summary) el.open = open;
  for (const [checkId, evidenceId] of recipeBases) {
    const check = w.checks.find(c => c.id === checkId);
    const evidence = w.evidence.find(e => e.id === evidenceId);
    if (check?.source && evidence?.execution && evidence.execution.recipe.source.repository === check.source.repository)
      showRecipeBasis(check, evidence);
    else recipeBases.delete(checkId);
  }
  renderedChecks = w.checks;
  document.querySelectorAll('a[href^="#"]').forEach((a) => {
    a.onclick = () => {
      const target = document.getElementById(a.getAttribute("href").slice(1));
      for (
        let parent = target?.parentElement;
        parent;
        parent = parent.parentElement
      ) {
        if (parent.tagName === "DETAILS") parent.open = true;
      }
    };
  });
  document.querySelectorAll('.configure-observation').forEach(b => {
    b.onclick = () => guard(b, () => act({ op: 'configure_observation', check_id: b.dataset.check, criterion: { assertion: $('assertion-' + b.dataset.check).value.trim(), subject: $('subject-' + b.dataset.check).value.trim() } }));
  });
  document.querySelectorAll('.load-prior-recipe').forEach(b => {
    b.onclick = () => guard(b, () => loadPriorRecipe(b.dataset.check));
  });
  document.querySelectorAll('.download-recipe').forEach(b => {
    b.onclick = () => guard(b, async () => {
      const recipe = recipeFromForm(b.dataset.check);
      downloadJSON(`trial-${state.workspace.id}-${recipe.check_id}.json`, recipe);
      message('試験設定を保存しました。CLIから実行すると、新しい試行と根拠をこの条件へ取り込めます。');
    });
  });
  document.querySelectorAll('.execute-recipe').forEach(b => {
    b.onclick = async () => { await guard(b, () => startExternal(b.dataset.check)); renderExecution(); };
  });
  renderExecution();
  document.querySelectorAll(".add-question").forEach((b) => {
    b.onclick = () =>
      guard(b, () =>
        act({
          op: "add_question",
          check_id: b.dataset.check,
          question: {
            id: "question-" + state.workspace.revision,
            text: $("question-text-" + b.dataset.check).value,
            answer: null,
            origin: null,
          },
        }),
      );
  });
  document.querySelectorAll(".answer").forEach(
    (b) =>
      (b.onclick = () =>
        guard(b, () =>
          act({
            op: "answer",
            check_id: b.dataset.check,
            question_id: b.dataset.question,
            answer: $("answer-" + b.dataset.check + "-" + b.dataset.question)
              .value,
            origin: $("origin-" + b.dataset.check + "-" + b.dataset.question)
              .value,
          }),
        )),
  );
  document.querySelectorAll(".revise").forEach(
    (b) =>
      (b.onclick = () =>
        guard(b, () =>
          act({
            op: "revise_check",
            check_id: b.dataset.check,
            text: $("text-" + b.dataset.check).value,
            origin: $("check-origin-" + b.dataset.check).value,
            rule: $("rule-" + b.dataset.check).value,
            dependencies: $("deps-" + b.dataset.check)
              .value.split(",")
              .map((s) => s.trim())
              .filter(Boolean),
          }),
        )),
  );
  const faults = {
    normal: "normal",
    "save-failure": "fail_before_save",
    "missing-observation": "omit_save_observation",
    disconnect: "disconnect_before_receive",
  };
  document.querySelectorAll(".demo").forEach(
    (b) =>
      (b.onclick = () =>
        guard(b, () =>
          post("/api/demo", {
            ...common(),
            check_id: b.dataset.check,
            fault: faults[b.dataset.fault],
          }),
        )),
  );
}
async function guard(b, fn) {
  b.disabled = true;
  try {
    await fn();
  } catch (e) {
    message(e.message, true);
  } finally {
    b.disabled = false;
  }
}
button("reload", load);
button("add-statement", () =>
  act({
    op: "add_statement",
    statement: {
      id: "note-" + state.workspace.revision,
      kind: $("statement-kind").value,
      role: $("role").value,
      text: $("statement-text").value,
      origin: $("statement-origin").value,
      check_ids: [
        ...$("statement-checks").querySelectorAll("input:checked"),
      ].map((el) => el.value),
    },
  }),
);
button("decide", () =>
  act({
    op: "decide",
    role: $("role").value,
    action: $("decision-action").value,
    check_ids: [...$("decision-checks").querySelectorAll("input:checked")].map(
      (el) => el.value,
    ),
    change_reference: $("change-reference").value.trim() || null,
    confidence:
      $("confidence").value === "" ? null : Number($("confidence").value),
    elapsed_ms:
      $("elapsed").value === "" ? null : Number($("elapsed").value) * 1000,
  }),
);
button("context-update", () =>
  act({
    op: "set_context",
    key: $("context-key").value.trim(),
    value: $("context-value").value,
  }),
);
button("add-check", () =>
  act({
    op: "add_check",
    check: {
      id: $("new-id").value.trim(),
      version: 1,
      text: $("new-text").value,
      origin: $("new-origin").value,
      source: pendingSource && $("new-origin").value === pendingSource.url ? pendingSource.reference : null,
      rule: "manual_only",
      dependencies: [],
      questions: [],
    },
  }),
);
load().then(async () => {
  if (!location.hash.startsWith("#code=")) return;
  const input = JSON.parse(decodeURIComponent(location.hash.slice(6)));
  if (typeof input.source !== "string" || typeof input.subject !== "string") return;
  const source = new URL(input.source);
  if (source.protocol !== "https:" || source.hostname !== "github.com" || source.username || source.password) return;
  const tools = await codeTools;
  let reference;
  try { reference = tools.validateCodeReference(input.reference || tools.referenceFromURL(source.href)); }
  catch { return; }
  if (tools.sourceURL(reference) !== source.href) return;
  pendingSource = { reference, url: source.href };
  $('new-origin').value = source.href;
  $("new-id").value = "code-check-" + state.workspace.revision;
  $("new-text").placeholder = input.subject + "で確かめたい動作を入力してください";
  for (let parent = $("new-text").parentElement; parent; parent = parent.parentElement)
    if (parent.tagName === "DETAILS") parent.open = true;
  $("new-text").focus();
  message("コードの出所を引き継ぎました。確かめたい動作を記入してください。試験方法はまだ未定義です。");
}).catch((e) => message(e.message, true));
