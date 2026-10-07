"use strict";
const token = "__REVIEW_TOKEN__";
let state;
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
  const response = await fetch("/api/case");
  const body = await response.json();
  if (!response.ok) throw Error(body.error);
  state = body;
  render();
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
      const target = match
        ? `evidence-${evidence.id}-${match[1] ? "adapter" : "events"}-${match[2]}`
        : `evidence-${evidence.id}`;
      return `<a href="#${esc(target)}">${esc(ref)}</a>`;
    })
    .join("、");
}
function render() {
  const w = state.workspace;
  $("case-title").textContent = w.title;
  $("revision").textContent = "事例 " + w.id + "・版 " + w.revision;
  $("checks").innerHTML = state.checks
    .map((v) => {
      const c = w.checks.find((c) => c.id === v.check_id);
      const latest = v.evidence.at(-1);
      return `<section class="check" id="check-${esc(c.id)}"><div class="row"><h2>${esc(c.text)}</h2><span class="badge ${esc(v.verdict)}">${esc(labels[v.verdict])}</span></div><p class="muted">条件 ${esc(c.id)}・版 ${c.version}／出所：${esc(c.origin)}</p><p class="reason">${esc(v.reason)}</p><p><strong>次の確認の提案</strong><br>${esc(v.next.text)}<br><span class="muted">理由：${esc(v.next.reason)}。この提案から自動実行しません。</span></p>${latest ? `<p><a href="#evidence-${esc(latest.id)}">根拠と実行記録を見る</a><br>${evidenceLinks(latest)}</p>` : ""}${c.questions.map((q) => `<div><p><strong>${esc(q.text)}</strong>${q.answer ? `<br>${esc(q.answer)}<br><span class="muted">出所：${esc(q.origin)}</span>` : "<br>未回答"}</p><label for="answer-${esc(c.id)}-${esc(q.id)}">回答・修正する内容</label><input id="answer-${esc(c.id)}-${esc(q.id)}"><label for="origin-${esc(c.id)}-${esc(q.id)}">回答の出所</label><input id="origin-${esc(c.id)}-${esc(q.id)}"><button class="secondary answer" data-check="${esc(c.id)}" data-question="${esc(q.id)}">回答と出所を保存</button></div>`).join("")}${
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
      }<details><summary>以前の結果と、現在の条件に使える範囲</summary>${v.evidence.map((e) => `<p>${esc(e.id)}：観測時は${esc(labels[e.observed.verdict])}／${e.applicable ? "現在の条件に対応" : "再確認が必要"}<br>${esc(e.invalidated_by.join("、"))}</p>`).join("") || "<p>根拠はまだありません。</p>"}</details><details><summary>確認事項を追加する</summary><label for="question-text-${esc(c.id)}">確認したいこと</label><input id="question-text-${esc(c.id)}"><button class="secondary add-question" data-check="${esc(c.id)}">未回答の確認事項を保存</button></details><details><summary>条件文・判定方法の更新</summary><p class="muted">既存の判定規則は定義済みの条件文だけに使えます。別の条件は「判定方法未定義」にしてください。版を更新すると、以前の根拠は履歴として残ります。条件文・判定規則を変えた場合は回答を再確認します。</p><label for="text-${esc(c.id)}">条件文</label><textarea id="text-${esc(c.id)}">${esc(c.text)}</textarea><label for="rule-${esc(c.id)}">判定規則</label><select id="rule-${esc(c.id)}">${[
        ["manual_only", "判定方法未定義"],
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
        `<section id="evidence-${esc(e.id)}"><h3>${esc(e.id)}／試行 ${esc(e.manifest.correlation.run_id)}</h3><p>条件 ${esc(e.check_id)}・取り込み時の版 ${e.check_version}</p><table><thead><tr><th>根拠</th><th>観測した内容</th></tr></thead><tbody>${e.events.map((event) => `<tr id="evidence-${esc(e.id)}-events-${event.sequence}"><td>events.jsonl#${event.sequence}<br>${esc(event.source)}／${esc(event.kind)}</td><td>${esc(event.detail)}</td></tr>`).join("")}${(e.adapter?.events ?? []).map((event) => `<tr id="evidence-${esc(e.id)}-adapter-${event.sequence}"><td>adapter-events.jsonl#${event.sequence}<br>${esc(event.kind)}</td><td>${esc(typeof event.detail === "string" ? event.detail : JSON.stringify(event.detail))}</td></tr>`).join("")}</tbody></table><details><summary>条件・コード・入力・環境の記録</summary><pre>${esc(JSON.stringify({ manifest: e.manifest, dependencies: e.dependencies, target: e.adapter?.target }, null, 2))}</pre></details></section>`,
    )
    .join("");
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
      rule: "manual_only",
      dependencies: [],
      questions: [],
    },
  }),
);
load().catch((e) => message(e.message, true));
