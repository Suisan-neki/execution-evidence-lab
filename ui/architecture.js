// 構成・観測の判定はRust APIを使う。LikeC4の固定版に対するDOMの対応は生成したID表で照合する。
import {
  renderBrowserEvidence,
  installBrowserScreenshots,
} from "/ui/browser-evidence.js";
const token = document.querySelector('meta[name="review-token"]').content;
installBrowserScreenshots(token);
const $ = (id) => document.getElementById(id),
  esc = (v) =>
    String(v ?? "").replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[c],
    );
let state,
  data,
  ids,
  selected = "system",
  model,
  diagramObserver,
  overlaySheet,
  observedRoots = new Set();
const basisLabels = {
    configuration: "設定から確認",
    human: "人による補足",
    inference: "解析による推定",
  },
  environmentLabels = {
    trial: "試験用",
    deployed: "実運用",
    simulated: "模擬",
    unknown: "環境は未確認",
  };
const methods = {
  browser_text: "画面表示",
  browser_canvas: "canvasの画素",
  server_http: "サーバーの直接読取",
  reported_assertion: "対象内の段階報告（報告された範囲）",
  browser_response: "HTTP応答",
  database_rows: "DBの直接読取",
  database_query: "DBの直接読取",
  stored_file: "保存先の直接読取",
};
const outcome = {
  pass: "○ 期待どおり",
  fail: "× 期待と異なる",
  not_observed: "？ 観測できていない",
};
const button = (text, fn, type = "outline") => {
  const b = document.createElement("button");
  b.className = "dads-button";
  b.dataset.type = type;
  b.dataset.size = "sm";
  b.textContent = text;
  b.addEventListener("click", () => guard(fn));
  return b;
};
const p = (text) => {
  const n = document.createElement("p");
  n.textContent = text;
  return n;
};
function status(text) {
  $("status").textContent = text;
}
async function api(path, body) {
  const r = await fetch("/api/v1/" + path, {
    method: body ? "POST" : "GET",
    headers: {
      "X-Review-Token": token,
      ...(body ? { "Content-Type": "application/json" } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const v = await r.json();
  if (!r.ok) throw Error(v.error || "記録を取得できません");
  return v;
}
async function guard(fn) {
  status("");
  try {
    await fn();
  } catch (e) {
    status(e.message);
  }
}
async function change(action) {
  if (!$("actor").value.trim() || !$("reason").value.trim())
    throw Error("記入者と記録する理由を入力してください");
  state = await api("change", {
    expected_revision: state.workspace.revision,
    actor: $("actor").value.trim(),
    reason: $("reason").value.trim(),
    action,
  });
}
function modelKey(m) {
  return m.id + ":" + m.version;
}
function presets() { return data.builtins || [data.builtin]; }
function availableModels() {
  return [...data.models, ...presets().filter(p => !data.models.some(m => modelKey(m) === modelKey(p)))];
}
function diagramIds() { return ids.models?.[model.id] || ids; }
function evidenceTarget(t) { return ["evidence", "reported_evidence"].includes(t.kind); }
function observationLabel(t) { return t.kind === "reported_evidence" ? t.assertion + "／" + t.subject : t.observation_id; }
function selectedRun() { return state.workspace.evidence.find(e => e.id === $("trial").value)?.manifest.correlation.run_id; }
async function load() {
  const key = model && modelKey(model),
    trial = $("trial").value;
  let loadedState, loadedData;
  for (let attempt = 0; attempt < 3; attempt++) {
    loadedState = await api("case");
    loadedData = await api("architecture");
    if (loadedState.workspace.revision === loadedData.revision) break;
    if (attempt === 2)
      throw Error("読み込み中に更新が続いています。最新版を読み直してください");
  }
  ids ??= await fetch("/ui/architecture-ids.json").then((r) => r.json());
  state = loadedState;
  data = loadedData;
  $("model").replaceChildren();
  for (const m of availableModels()) {
    const o = new Option(
      `${m.title}／版${m.version}${data.models.some(saved => modelKey(saved) === modelKey(m)) ? "" : "（未保存の見本）"}`,
      modelKey(m),
    );
    $("model").append(o);
  }
  if ([...$("model").options].some((o) => o.value === key))
    $("model").value = key;
  else {
    const preferred = data.models.at(-1) || presets().find(m => state.workspace.checks.some(c => c.source?.repository === m.source.repository && c.source.commit === m.source.commit)) || presets()[0];
    $("model").value = modelKey(preferred);
  }
  $("trial").replaceChildren(new Option("試行を選択しない", ""));
  const runs = new Set();
  for (const e of state.workspace.evidence) {
    if (runs.has(e.manifest.correlation.run_id)) continue;
    runs.add(e.manifest.correlation.run_id);
    $("trial").append(
      new Option(
        `${e.manifest.correlation.run_id}／${e.execution?.recipe.trial?.fault || e.reported?.context.fault || e.manifest.mode}`,
        e.id,
      ),
    );
  }
  if ([...$("trial").options].some((o) => o.value === trial))
    $("trial").value = trial;
  render();
}
function saved() {
  return data.models.some((m) => modelKey(m) === modelKey(model));
}
function object() {
  return [...model.elements, ...model.relations].find((o) => o.id === selected);
}
function matchingLinks(objectId) {
  return data.links.filter(
    (v) =>
      v.link.model_id === model.id &&
      v.link.model_version === model.version &&
      v.link.object_id === objectId,
  );
}
function relevant(objectId) {
  const run = selectedRun();
  return matchingLinks(objectId).filter(v => evidenceTarget(v.link.target) && state.workspace.evidence.some(e => e.id === v.link.target.evidence_id && e.manifest.correlation.run_id === run));
}
function observationStatus(objectId) {
  const links = relevant(objectId);
  if (!links.length) return { kind: "unknown", label: "？ 対応する観測なし" };
  if (links.some((l) => !l.applicable)) {
    const historical = links.some(l => l.observed === "fail") ? "× 期待と異なる" : links.some(l => l.observed == null || l.observed === "not_observed") ? "？ 未観測を含む" : "○ 期待どおり";
    return { kind: "unknown", label: "↻ 現在の条件には再確認が必要／観測時：" + historical };
  }
  if (links.some((l) => l.observed === "fail"))
    return { kind: "fail", label: "関連観測：× 期待と異なる" };
  if (links.some((l) => l.observed == null || l.observed === "not_observed"))
    return { kind: "unknown", label: "関連観測：？ 未観測を含む" };
  return { kind: "pass", label: "関連観測：○ 期待どおり" };
}
function sourceLink(s) {
  if (!s) return p("固定版のコード参照なし");
  const a = document.createElement("a");
  a.href = `https://github.com/${encodeURIComponent(s.repository.split("/")[0])}/${encodeURIComponent(s.repository.split("/")[1])}/blob/${encodeURIComponent(s.commit)}/${s.path.split("/").map(encodeURIComponent).join("/")}#L${s.line}-L${s.end_line}`;
  a.target = "_blank";
  a.rel = "noopener noreferrer";
  a.className = "source";
  a.textContent = `${s.repository}@${s.commit.slice(0, 12)}／${s.path}:${s.line}–${s.end_line}`;
  return a;
}
function render() {
  model =
    availableModels().find((m) => modelKey(m) === $("model").value) || presets()[0];
  if (!object()) selected = "system";
  if (!object()) selected = model.elements[0].id;
  $("model-description").replaceChildren(
    p(
      `${environmentLabels[model.environment]}／構成の版${model.version}／事例の版${state.workspace.revision}`,
    ),
    sourceLink(model.source),
    p("このGit参照とイメージ内のコードが同一だとは証明していません。"),
  );
  const current = Math.max(
    ...data.models.filter((m) => m.id === model.id).map((m) => m.version),
    model.version,
  );
  if (model.version < current)
    $("model-description").append(
      p(
        `過去の構成です。現在は版${current}があります。過去の試行の参照先は変更しません。`,
      ),
    );
  const limits = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = "構成の根拠と未確認事項";
  limits.append(summary, ...model.limits.map(p));
  const pins = document.createElement("pre");
  pins.textContent = JSON.stringify(
    { images: model.images, browser_version: model.browser_version },
    null,
    2,
  );
  limits.append(pins);
  $("model-description").append(limits);
  $("save-model").hidden = saved();
  renderDiagram();
  renderObjects();
  renderDetails();
  renderControls();
}
function renderObjects() {
  $("objects").replaceChildren();
  for (const o of [...model.elements, ...model.relations]) {
    const s = observationStatus(o.id),
      b = button(o.title, () => choose(o.id));
    b.className = "observed-" + s.kind;
    b.setAttribute("aria-pressed", String(o.id === selected));
    b.dataset.object = o.id;
    const small = document.createElement("small");
    small.textContent = (o.from ? "接続／" : "要素／") + s.label;
    b.append(small);
    $("objects").append(b);
  }
}
function choose(id, focus = true) {
  if (
    !model.hasOwnProperty("elements") ||
    ![...model.elements, ...model.relations].some((o) => o.id === id)
  )
    return;
  selected = id;
  renderObjects();
  renderDetails();
  renderControls();
  markDiagram();
  if (focus) $("details").focus();
}
function renderDiagram() {
  diagramObserver?.disconnect();
  observedRoots = new Set();
  const box = $("diagram");
  box.replaceChildren();
  // 生成済み図と保存モデルが完全一致する場合だけ埋め込む。未知の版へ旧図を流用しない。
  if (!presets().some(p => JSON.stringify(model) === JSON.stringify(p))) {
    box.hidden = true;
    $("diagram-status").textContent =
      "この構成の版の図は未生成です。下の構成一覧から説明と記録を確認できます。";
    return;
  }
  box.hidden = false;
  $("diagram-status").textContent =
    "図を読み込んでいます。下の一覧からも同じ構成要素・接続を選べます。";
  const view = document.createElement("likec4-view");
  view.setAttribute("view-id", diagramIds().views?.[$("view").value] || $("view").value);
  view.setAttribute("browser", "false");
  view.setAttribute("color-scheme", "light");
  const canvas = document.createElement("div");
  canvas.className = "diagram-canvas";
  canvas.append(view);
  box.append(canvas);
  customElements.whenDefined("likec4-view").then(() => {
    if (!view.isConnected) return;
    const shadow = view.shadowRoot;
    if (!shadow) return;
    const style = document.createElement("style");
    style.textContent = `:host{font-family:system-ui,sans-serif}.react-flow__node[data-eel-selected=true]{outline:4px solid #0055b8;outline-offset:3px}.react-flow__node[data-eel-state]::after{content:attr(data-eel-label);position:absolute;left:0;bottom:-26px;white-space:nowrap;background:white;color:#26364a;font-size:13px;padding:2px 6px;border:1px solid #63748a;border-radius:3px;pointer-events:none}.react-flow__node[data-eel-state=pass]::after{border:2px solid #157347}.react-flow__node[data-eel-state=fail]::after{border:2px solid #b3261e}.react-flow__node[data-eel-state=unknown]::after{border-style:dashed}.react-flow__edge[data-eel-state=pass] path{stroke:#157347!important}.react-flow__edge[data-eel-state=fail] path{stroke:#b3261e!important}.react-flow__edge[data-eel-state=unknown] path{stroke-dasharray:7 5!important}.react-flow__edge[data-eel-selected=true] path{stroke:#0055b8!important;stroke-width:4!important}`;
    overlaySheet = new CSSStyleSheet();
    overlaySheet.replaceSync(style.textContent);
    const selectFromDiagram = (event) => {
      if (!view.isConnected) return;
      if (
        event.type === "keydown" &&
        event.key !== "Enter" &&
        event.key !== " "
      )
        return;
      const path = event.composedPath();
      const label = path.find((n) => n.matches?.(".likec4-edge-label__text"));
      const node = path.find((n) => n.matches?.(".react-flow__node[data-id]"));
      const edge = path.find((n) => n.matches?.(".react-flow__edge[data-id]"));
      const candidates = label
        ? model.relations
            .filter((r) => r.title === label.textContent.trim())
            .map((r) => r.id)
        : edge
          ? diagramIds().edges[edge.dataset.id] || []
          : node
            ? [diagramIds().nodes[node.dataset.id]].filter(Boolean)
            : [];
      if (!candidates.length) return;
      event.preventDefault();
      event.stopPropagation();
      if (candidates.length === 1) choose(candidates[0], false);
      else
        status(
          "この線の接続を一つに特定できません。内部の構成か、下の一覧から接続を選んでください。",
        );
    };
    shadow.addEventListener("click", selectFromDiagram, true);
    shadow.addEventListener("keydown", selectFromDiagram, true);
    diagramObserver = new MutationObserver(() => markDiagram());
    const poll = () => {
      if (!view.isConnected) return;
      markDiagram();
      if (!deepAll(".react-flow__node").length && Date.now() < deadline)
        setTimeout(poll, 100);
    };
    const deadline = Date.now() + 15000;
    poll();
  });
}
function diagramRoots() {
  const root = $("diagram").querySelector("likec4-view")?.shadowRoot;
  if (!root) return [];
  const roots = [root];
  for (let i = 0; i < roots.length; i++)
    for (const el of roots[i].querySelectorAll("*"))
      if (el.shadowRoot) roots.push(el.shadowRoot);
  return roots;
}
function deepAll(selector) {
  return diagramRoots().flatMap((r) => [...r.querySelectorAll(selector)]);
}
function markDiagram() {
  const shadow = $("diagram").querySelector("likec4-view")?.shadowRoot;
  if (!shadow) return;
  for (const root of diagramRoots()) {
    if (overlaySheet && !root.adoptedStyleSheets.includes(overlaySheet))
      root.adoptedStyleSheets = [...root.adoptedStyleSheets, overlaySheet];
    if (diagramObserver && !observedRoots.has(root)) {
      diagramObserver.observe(root, { childList: true, subtree: true });
      observedRoots.add(root);
    }
  }
  for (const n of deepAll(".react-flow__node[data-id]")) {
    const id = diagramIds().nodes[n.dataset.id];
    if (!id) continue;
    const s = observationStatus(id);
    n.dataset.eelObject = id;
    n.dataset.eelState = s.kind;
    n.dataset.eelLabel = s.label;
    n.dataset.eelSelected = String(id === selected);
  }
  for (const label of deepAll(".likec4-edge-label__text")) {
    label.tabIndex = 0;
    label.setAttribute("role", "button");
    label.setAttribute("aria-label", "接続：" + label.textContent.trim());
  }
  for (const n of deepAll(".react-flow__edge[data-id]")) {
    const objects = diagramIds().edges[n.dataset.id] || [];
    const states = objects.map(observationStatus);
    n.dataset.eelState = states.some((s) => s.kind === "fail")
      ? "fail"
      : states.length && states.every((s) => s.kind === "pass")
        ? "pass"
        : "unknown";
    n.dataset.eelSelected = String(objects.includes(selected));
  }
  if (deepAll(".react-flow__node").length)
    $("diagram-status").textContent =
      "図と下の一覧のどちらからも選択できます。図が横幅に収まらない場合は左右にスクロールできます。観測のない接続は未確認です。";
}
function renderDetails() {
  const o = object();
  $("details-heading").textContent = o.title;
  const body = $("details-body");
  body.replaceChildren(p(o.description));
  if (o.from)
    body.append(
      p(
        `接続元：${model.elements.find((e) => e.id === o.from).title}／接続先：${model.elements.find((e) => e.id === o.to).title}`,
      ),
      p(
        "構成上の接続です。以下の関連観測から、通信経路全体の成立は判定しません。",
      ),
    );
  body.append(
    p(`${basisLabels[o.basis.kind]}：${o.basis.note}`),
    sourceLink(o.basis.source),
  );
  const h = document.createElement("h3");
  h.textContent = "関連する疑問・確認事項・試験結果";
  body.append(h);
  const list = matchingLinks(o.id);
  if (!list.length) body.append(p("対応づけた記録はまだありません。"));
  for (const v of list) {
    const l = v.link,
      t = l.target,
      section = document.createElement("section");
    section.className = "record-action";
    if (t.kind === "check") {
      const c = state.workspace.checks.find((c) => c.id === t.check_id);
      section.append(
        p(
          `確認事項 ${t.check_id}／参照した版${t.check_version}${v.applicable ? "" : "／現在の条件には再確認が必要"}`,
        ),
        p(
          v.applicable
            ? c.text
            : "参照時の版と現在の条件の版が異なります。旧版は事例の履歴で確認してください。",
        ),
      );
      if (v.applicable)
        section.append(
          ...c.questions.map((q) => p(`${q.text}／${q.answer || "未回答"}`)),
        );
    }
    if (t.kind === "statement") {
      const s = state.workspace.statements.find((s) => s.id === t.statement_id);
      section.append(
        p(s.text),
        p(`出所：${s.origin}／記入者：${s.recorded_by || "未記録"}`),
      );
    }
    if (evidenceTarget(t)) {
      const e = state.workspace.evidence.find((e) => e.id === t.evidence_id);
      const run = selectedRun();
      section.hidden = !!run && run !== e.manifest.correlation.run_id;
      section.classList.add(
        "observed-" +
          (v.observed === "fail"
            ? "fail"
            : v.observed === "pass"
              ? "pass"
              : "unknown"),
      );
      section.append(
        p(
          `${methods[v.method] || v.method || "観測方法を未確認"}／${observationLabel(t)}／${outcome[v.observed] || "？ 観測結果の記録なし"}`,
        ),
        p(e.manifest.correlation.run_id),
        p(v.detail),
        p(
          v.applicable
            ? "この構成・現在の条件への照合済み。要素全体の正常性は判定しません。"
            : "↻ 現在の条件には再確認が必要。観測時の結果を表示しています。",
        ),
        button("試験結果と証拠を見る", () => showEvidence(e, observationLabel(t), t)),
      );
    }
    body.append(section);
  }
}
function showEvidence(e, obs, target) {
  const d = document.createElement("dialog"),
    back = button("元の構成図へ戻る", () => d.close());
  back.classList.add("evidence-return");
  d.append(back, p(`構成の版${model.version}／${object().title}／${obs}`));
  const summary = state.checks
    .flatMap((c) => c.evidence)
    .find((v) => v.id === e.id)?.summary;
  if (summary && !e.reported) {
    d.append(
      p(summary.tried),
      p(`故障条件：${summary.fault}／${summary.fault_applied}`),
    );
    for (const item of summary.items) {
      const sec = document.createElement("section");
      sec.className = "record-action";
      sec.append(
        p(`${item.id}／${item.scope}／${item.observed_label}`),
        p(item.source),
        p(`期待：${item.expected}`),
        p(item.detail),
      );
      if (item.id === obs) sec.setAttribute("aria-label", "選択した観測条件");
      d.append(sec);
    }
  }
  if (e.reported) {
    d.append(p(`報告者：${e.reported.producer}／${e.reported.origin}`), p(`試行条件：${e.reported.context.fault || "報告に記載なし"}`), p("段階の報告と、基盤によるHTTP・DBの直接読取は別です。対象の実行経路の独立した証明ではありません。"));
    const matching = e.reported.observations.filter(o => o.assertion === target.assertion && o.subject === target.subject);
    if (!matching.length) d.append(p("この段階の成功・失敗の報告はありません。処理の失敗とは断定しません。"));
    for (const o of matching) {
      const item = document.createElement("section"); item.className = "record-action"; item.setAttribute("aria-label", "選択した観測条件");
      item.append(p(`${o.assertion}／${o.subject}／${outcome[o.outcome]}`), sourceLink(o.source), p(o.detail)); d.append(item);
    }
  }
  const browser = document.createElement("div");
  browser.innerHTML = renderBrowserEvidence(e);
  d.append(browser);
  const raw = document.createElement("details"),
    s = document.createElement("summary"),
    pre = document.createElement("pre");
  s.textContent = e.reported ? "固定版と報告の詳しい記録" : "固定版と実行の詳しい記録";
  pre.textContent = JSON.stringify(e.execution || e.reported, null, 2);
  raw.append(s, pre);
  d.append(raw);
  document.body.append(d);
  d.addEventListener(
    "close",
    () => {
      d.remove();
      $("details").focus();
    },
    { once: true },
  );
  d.showModal();
}
function form(title, label, control, submit, action) {
  const section = document.createElement("div");
  section.className = "record-action";
  const h = document.createElement("h3");
  h.textContent = title;
  const l = document.createElement("label");
  l.textContent = label;
  l.htmlFor = control.id;
  section.append(h, l, control, button(submit, action));
  return section;
}
function makeSelect(id, options) {
  const s = document.createElement("select");
  s.id = id;
  for (const [value, text] of options) s.append(new Option(text, value));
  return s;
}
async function addLink(
  target,
  context = {
    model_id: model.id,
    model_version: model.version,
    object_id: selected,
  },
) {
  await change({
    op: "add_architecture_link",
    link: {
      id: "link-" + crypto.randomUUID(),
      ...context,
      target,
    },
  });
  await load();
  status("対応づけを記録しました。");
}
function renderControls() {
  const box = $("record-controls");
  box.replaceChildren();
  if (!saved()) {
    box.append(
      p("見本の構成を事例に追加すると、疑問や試験結果を対応づけられます。"),
    );
    return;
  }
  box.append(p(`対象：${object().title}／構成の版${model.version}`));
  const check = makeSelect(
    "link-check",
    state.workspace.checks.map((c) => [c.id, c.text]),
  );
  if (check.options.length)
    box.append(
      form(
        "確認事項を対応づける",
        "確認事項",
        check,
        "この確認事項を対応づける",
        () =>
          addLink({
            kind: "check",
            check_id: check.value,
            check_version: state.workspace.checks.find(
              (c) => c.id === check.value,
            ).version,
          }),
      ),
    );
  const kind = makeSelect("note-kind", [
    ["unanswered", "疑問（未回答）"],
    ["statement", "期待（自分の言葉）"],
    ["implementation", "実装と確認方法の説明"],
  ]);
  const kindLabel = document.createElement("label");
  kindLabel.htmlFor = "note-kind";
  kindLabel.textContent = "記録の種類";
  box.append(kindLabel, kind);
  const note = document.createElement("textarea");
  note.id = "architecture-note";
  note.rows = 3;
  box.append(
    form(
      "疑問・期待を残す",
      "自分の言葉で記入",
      note,
      "疑問・期待を記録して対応づける",
      async () => {
        if (!note.value.trim())
          throw Error("疑問や期待する動作を入力してください");
        const id = "statement-" + crypto.randomUUID();
        const context = {
          model_id: model.id,
          model_version: model.version,
          object_id: selected,
        };
        await change({
          op: "add_statement",
          statement: {
            id,
            kind: kind.value,
            role: $("role").value,
            text: note.value.trim(),
            origin: $("reason").value.trim(),
            check_ids: [],
          },
        });
        await addLink({ kind: "statement", statement_id: id }, context);
      },
    ),
  );
  const options = state.workspace.evidence.flatMap(e => {
    if (e.reported && e.observation) return [[JSON.stringify({kind:"reported_evidence",evidence_id:e.id,assertion:e.observation.assertion,subject:e.observation.subject}), `${e.manifest.correlation.run_id}／段階報告：${e.observation.subject}`]];
    return (e.execution?.recipe.trial?.checks || []).map(c => [JSON.stringify({kind:"evidence",evidence_id:e.id,observation_id:c.id}), `${e.manifest.correlation.run_id}／${c.scope}（${c.id}）`]);
  });
  const observation = makeSelect("link-observation", options);
  if (options.length)
    box.append(
      form(
        "試行の一つの観測を対応づける",
        "試行と観測条件",
        observation,
        "この観測を対応づける",
        () => {
          return addLink(JSON.parse(observation.value));
        },
      ),
    );
}
$("reload").addEventListener("click", () =>
  guard(async () => {
    await load();
    status("最新版を読み直しました。");
  }),
);
$("model").addEventListener("change", () => guard(render));
$("view").addEventListener("change", () => guard(renderDiagram));
$("trial").addEventListener("change", () =>
  guard(() => {
    renderObjects();
    renderDetails();
    markDiagram();
  }),
);
$("save-model").addEventListener("click", () =>
  guard(async () => {
    const selectedModel = structuredClone(model);
    await change({ op: "add_architecture_model", model: selectedModel });
    await load();
    status("試験用構成を事例に追加しました。");
  }),
);
await guard(load);
