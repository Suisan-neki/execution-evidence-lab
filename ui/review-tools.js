// モードは事例に保存し、説明の詳しさはこのブラウザだけで変更する。
// 条件・結果の判定と確認資料の生成はRustが行う。
const domainLabels = { general: '汎用', medical: '医療' };
const verdictLabels = { satisfied: '条件を満たす', not_satisfied: '条件を満たさない', unknown: '判定できない' };
function el(tag, text, attrs = {}) {
  const n = document.createElement(tag);
  if (text) n.textContent = text;
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  return n;
}
let fieldId = 0;
function control(label, input) {
  input.id = 'review-tools-field-' + (++fieldId);
  input.dataset.size = 'md';
  const wrapper = el('div', '', { class: 'dads-form-control-label', 'data-size': 'md' });
  const title = el('label', label, { class: 'dads-form-control-label__label', for: input.id });
  const outer = el('span', '', { class: input.tagName === 'SELECT' ? 'dads-select' : 'dads-input-text' });
  if (input.tagName === 'SELECT') {
    const inner = el('span', '', { class: 'dads-select__control' });
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'dads-select__chevron'); svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('width', '16'); svg.setAttribute('height', '16'); svg.setAttribute('aria-hidden', 'true');
    const path = document.createElementNS('http://www.w3.org/2000/svg', 'path'); path.setAttribute('d', 'M12 17L3 8L4 7L12 15L20 7L21 8L12 17Z'); path.setAttribute('fill', 'currentcolor'); svg.append(path);
    inner.append(input, svg); outer.append(inner);
  } else { input.type = 'text'; outer.append(input); }
  wrapper.append(title, outer);
  return wrapper;
}
function button(label, action) {
  const b = el('button', label, { type: 'button', class: 'dads-button', 'data-type': 'outline', 'data-size': 'sm' });
  b.addEventListener('click', action);
  return b;
}
function download(content, type, filename) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const link = el('a', '', { href: url, download: filename });
  document.body.append(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
}
export async function mountReviewTools(root, { token, onChange = async () => {} }) {
  if (!root) return;
  let view, catalog, generation = 0, pending = false;
  let detail = 'plain';
  try { detail = localStorage.getItem('eel-explanation-detail') === 'technical' ? 'technical' : 'plain'; } catch { /* 保存できなくても閲覧できる。 */ }
  const title = el('h2', '確認モードと確認資料');
  const status = el('p', '', { role: 'status', 'aria-live': 'polite' });
  const body = el('div');
  root.replaceChildren(title, status, body);
  async function request(path, data, html = false) {
    const r = await fetch('/api/v1/' + path, {
      method: data ? 'POST' : 'GET',
      headers: { 'X-Review-Token': token, ...(data ? { 'Content-Type': 'application/json' } : {}) },
      ...(data ? { body: JSON.stringify(data) } : {}),
    });
    if (!r.ok) { let message; try { message = (await r.json()).error; } catch { /* HTTPの状態を表示する。 */ } throw Error(message || `HTTP ${r.status}`); }
    return html ? r.text() : r.json();
  }
  async function guarded(action) {
    if (pending) return;
    pending = true; status.textContent = '';
    root.querySelectorAll('button').forEach(b => { b.disabled = true; });
    try { await action(); } catch (e) { status.textContent = e.message; }
    finally { pending = false; root.querySelectorAll('button').forEach(b => { b.disabled = false; }); }
  }
  async function reload() {
    const seq = ++generation;
    const [next, nextCatalog] = await Promise.all([request('case'), request('profiles')]);
    if (seq !== generation) return;
    view = next; catalog = nextCatalog; render();
  }
  function render() {
    const history = view.workspace.profile_history || [], active = history.at(-1);
    const domain = active?.domain || 'general', version = active?.version || 0;
    const controls = el('div', '', { class: 'review-tools-fields' });
    const mode = el('select', '', { class: 'dads-select__select', 'aria-label': '確認モード' });
    Object.entries(domainLabels).forEach(([value, label]) => mode.add(new Option(label, value)));
    mode.value = domain;
    const details = el('select', '', { class: 'dads-select__select', 'aria-label': '説明の詳しさ' });
    details.add(new Option('分かりやすい説明', 'plain')); details.add(new Option('技術的な説明', 'technical')); details.value = detail;
    const actor = el('input', '', { class: 'dads-input-text__input', 'aria-label': 'モード設定の記入者', autocomplete: 'name' });
    const reason = el('input', '', { class: 'dads-input-text__input', 'aria-label': 'モード設定の理由' });
    const explanation = el('p');
    const proposals = el('div'), reportPreview = el('div', '', { 'data-review-report': '' });
    async function change(action) {
      if (!actor.value.trim() || !reason.value.trim()) throw Error('モード設定の記入者と理由を入力してください。');
      const name = actor.value, why = reason.value;
      view = await request('change', { expected_revision: view.workspace.revision, actor: name.trim(), reason: why.trim(), action });
      render();
      const inputs = body.querySelectorAll('input'); inputs[0].value = name; inputs[1].value = why;
      await onChange();
    }
    details.addEventListener('change', () => {
      detail = details.value;
      try { localStorage.setItem('eel-explanation-detail', detail); } catch { /* この画面では選択を維持する。 */ }
      renderProposals();
    });
    controls.append(control('確認モード', mode), control('説明の詳しさ', details));
    const inputs = el('div', '', { class: 'review-tools-fields' });
    inputs.append(control('モード設定の記入者', actor), control('モード設定の理由', reason));
    function renderProposals() {
      const description = domain === 'medical'
        ? { plain: '患者や職員に何を見せ、誰が記録を扱うかを確認するための候補を示します。必要な条件は実際の担当者と確認します。', technical: '医療に関する表示・認可・保持の確認候補を共通の条件モデルへ追加できます。規則と業務要件は未確定です。' }
        : { plain: '扱うデータ、外への通信、停止後の復旧について確認する候補を示します。', technical: '入力・永続化・通信・復旧の確認候補を示します。観測方法を設定するまではmanual_onlyとして扱います。' };
      explanation.textContent = description[detail];

      const list = el('ul');
      for (const template of catalog.templates.filter(t => t.domain === 'general' || domain === 'medical')) {
        const item = el('li');
        item.append(el('strong', template.text), el('p', template[detail]));
        if (view.workspace.checks.some(c => c.id === 'profile-' + template.id)) item.append(el('p', '確認事項に追加済み'));
        else item.append(button('候補を追加する', () => guarded(() => change({ op: 'add_profile_check', template_id: template.id, selection_version: version }))));
        list.append(item);
      }
      proposals.replaceChildren(el('h3', '確認項目の候補'), el('p', catalog.notice), list);
    }
    const proposalDetails = el('details'); proposalDetails.append(el('summary', '確認項目の候補を見る'), proposals);
    const historyDetails = el('details');
    historyDetails.append(el('summary', 'モードの変更履歴'), el('p', history.length ? history.map(s => `事例版${s.recorded_revision}：${domainLabels[s.domain]}／選択版${s.version}／カタログ${s.catalog_version}`).join('\n') : 'モードの選択記録はまだありません。汎用の候補を表示しています。'));
    const report = el('details');
    report.append(el('summary', '他の人と確認する資料'), el('p', '条件文や判断理由に記入した情報も含まれます。共有する前に保存内容を確認してください。'),
      button('現在の結果を確認する', () => guarded(async () => {
        const r = await request('report');
        const list = el('ul');
        r.checks.forEach(c => list.append(el('li', `${c.definition.text}：${verdictLabels[c.current.verdict]}。${c.current.reason}`)));
        reportPreview.replaceChildren(el('h3', `${r.title}／事例版${r.revision}`), list, el('p', r.limits[0]),
          button('確認資料のJSONを保存する', () => download(JSON.stringify(r, null, 2), 'application/json', `${r.case_id}-revision-${r.revision}.json`)),
          button('確認資料のHTMLを保存する', () => guarded(async () => {
            // HTMLも単独の事例版からサーバーで作る。JSONのプレビューより更新されている場合は保存前に知らせる。
            const current = await request('case');
            if (current.workspace.revision !== r.revision) throw Error('事例が更新されています。「現在の結果を確認する」で内容を読み直してください。');
            const response = await request('report-html', { expected_revision: r.revision }, true);
            download(response, 'text/html', `${r.case_id}-revision-${r.revision}.html`);
          })));
      })), reportPreview);
    const actions = el('div', '', { class: 'review-tools-actions' });
    actions.append(button('確認モードを記録する', () => guarded(() => change({ op: 'select_profile', domain: mode.value }))), button('設定を読み直す', () => guarded(reload)));
    body.replaceChildren(explanation, el('p', `現在は${domainLabels[domain]}モードです。モードを選んでも試験の結果や権限は変わりません。`), controls, inputs, actions, proposalDetails, historyDetails, report);
    renderProposals();
    if (pending) root.querySelectorAll('button').forEach(b => { b.disabled = true; });
  }
  try { await reload(); } catch (e) { status.textContent = e.message; }
}
