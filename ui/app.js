import { mountDiagram, objectTitle, selectDiagramObject } from '/ui/repository-diagram.js';
const $ = id => document.getElementById(id);
const token = document.querySelector('meta[name="review-token"]').content;
const state = { audience: null, source: null, overview: null, graph: null, selected: null, view: null, sequence: 0, loading: false, saving: false, draftKey: null, preparation: null };
const AUDIENCE = { operator: '非技術者', developer: '技術者' };
function el(tag, attrs = {}, ...children) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'onClick') n.addEventListener('click', v);
    else if (k === 'className') n.className = v;
    else n.setAttribute(k, v);
  }
  for (const c of children.flat()) if (c !== null && c !== undefined) n.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return n;
}
function button(text, onClick, type = 'outline') { return el('button', { type: 'button', className: 'dads-button', 'data-type': type, 'data-size': 'sm', onClick }, text); }
function remembered(key) { try { return sessionStorage.getItem(key); } catch { return null; } }
function remember(key, value) { try { sessionStorage.setItem(key, value); } catch { /* 保存できなくても現在の画面は使う。 */ } }
function status(text) { $('status').textContent = text; }
function error(e) { $('alert').replaceChildren(el('p', { className: 'input-error' }, e.message)); }
async function api(path, body) {
  const r = await fetch('/api/v1/' + path, { method: body ? 'POST' : 'GET', headers: { 'X-Review-Token': token, ...(body ? { 'Content-Type': 'application/json' } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) });
  const data = await r.json();
  if (!r.ok) throw Error(data.error || `HTTP ${r.status}`);
  return data;
}
async function run(task, current = () => true) { $('alert').replaceChildren(); try { return await task(); } catch (e) { if (current()) error(e); return null; } }
function screen(name) {
  for (const id of ['audience', 'repository', 'system']) $(id + '-screen').hidden = id !== name;
  $('change-audience').hidden = !state.audience;
  $('change-audience').textContent = state.audience ? `立場：${AUDIENCE[state.audience]}（変更）` : '';
}
function saveDraft() { if (state.draftKey) remember(state.draftKey, $('concern-text').value); }
function draftKey() { return state.source ? `eel-concern:${state.source.repository}@${state.source.commit}:${state.selected?.id || 'whole'}` : null; }
function findObject(id) {
  const node = state.graph?.nodes.find(n => n.id === id); if (node) return { ...node, object_type: 'element' };
  const edge = state.graph?.edges.find(e => e.id === id); return edge ? { ...edge, object_type: 'relation' } : null;
}
function concernLabel(object) {
  if (!object) return 'システム全体';
  return object.object_type === 'relation' ? `${state.graph.nodes.find(n => n.id === object.from)?.title} → ${state.graph.nodes.find(n => n.id === object.to)?.title}：${object.title}` : object.title;
}
function selectNode(node) {
  saveDraft(); state.selected = node; state.draftKey = draftKey();
  $('concern-text').value = remembered(state.draftKey) || '';
  $('selected-heading').textContent = node?.object_type === 'relation' ? `${state.graph.nodes.find(n => n.id === node.from)?.title} → ${state.graph.nodes.find(n => n.id === node.to)?.title}` : objectTitle(node, false);
  $('selected-description').textContent = node?.description || '全体への不安や疑問も、そのまま書けます。';
  $('select-whole').hidden = !node;
  selectDiagramObject($('repository-diagram'), node?.id);
  const details = $('selected-details'); details.replaceChildren();
  const technical = state.audience === 'developer';
  if (node) {
    if (node.scope_note) details.append(el('p', { className: 'note' }, node.scope_note));
    details.append(el('p', { className: 'note' }, node.basis?.kind === 'configuration' ? '設定に定義された構成です。動作はまだ検証していません。' : 'コードの記述から整理した推定です。動作はまだ検証していません。'));
    if (node.object_type === 'relation') details.append(el('p', {}, `やり取り：${node.title}`));
    if (technical && (node.technology?.length || node.protocol)) details.append(el('p', { className: 'component-technology' }, `技術・接続：${node.protocol || node.technology.join(' / ')}`));
    if (technical && node.details?.length) {
      const internal = el('details', { className: 'component-internal', open: '' }, el('summary', {}, '内部の処理と起動設定'));
      const groups = new Map();
      for (const d of node.details) { if (!groups.has(d.title)) groups.set(d.title, []); groups.get(d.title).push(d); }
      for (const [title, items] of groups) internal.append(el('h3', {}, title), el('ul', {}, items.map(d => el('li', {}, d.text))));
      details.append(internal);
    }
  }
  if (state.graph) {
    const connections = node ? state.graph.edges.filter(e => e.from === node.id || e.to === node.id) : state.graph.edges;
    if (connections.length && node?.object_type !== 'relation') {
      const list = el('ul', { className: 'component-connections' });
      for (const e of connections) {
        const from = state.graph.nodes.find(n => n.id === e.from), to = state.graph.nodes.find(n => n.id === e.to);
        list.append(el('li', {}, button(`${from.title} → ${to.title}：${e.title}`, () => selectNode({ ...e, object_type: 'relation' }), 'text')));
      }
      if (node) details.append(el('h3', {}, 'この部分とのやり取り'), list);
      else details.append(el('details', {}, el('summary', {}, '図の接続を文章で読む'), list));
    }
  }
  const evidence = $('selected-evidence'); evidence.replaceChildren();
  if (node) {
    const d = el('details', {}, el('summary', {}, '説明の根拠となる設定・コード'));
    for (const ref of node.refs || []) {
      const href = `https://github.com/${state.source.repository}/blob/${state.source.commit}/${ref.path.split('/').map(encodeURIComponent).join('/')}#L${ref.line}`;
      d.append(el('p', {}, el('a', { href, target: '_blank', rel: 'noopener noreferrer' }, `${ref.path} ${ref.line}行目`), technical && ref.excerpt ? el('code', {}, ref.excerpt) : null));
    }
    evidence.append(d);
  }
}
$('concern-text').addEventListener('input', saveDraft);
async function renderGraph() {
  await mountDiagram($('repository-diagram'), state.graph, { technical: state.audience === 'developer', selected: state.selected?.id, onSelect: selectNode });
}
function setAudience(audience) {
  state.audience = audience; remember('eel-audience', audience);
  // 名前や権限を推測しない。詳細な検証画面にも自己申告の立場だけを引き継ぐ。
  remember('eel-person', JSON.stringify({ name: 'ブラウザ利用者（本人確認なし）', role: audience }));
  if (state.source) { screen('system'); run(renderGraph); selectNode(state.selected); renderConcerns(); }
  else {
    screen('repository');
    const params = new URLSearchParams(location.search);
    if (params.get('url')) { $('repository-url').value = params.get('url'); $('repository-ref').value = params.get('ref') || ''; openRepository(params.get('url'), params.get('ref') || '', false); }
  }
}
for (const b of document.querySelectorAll('[data-audience]')) b.addEventListener('click', () => setAudience(b.dataset.audience));
$('change-audience').addEventListener('click', () => { saveDraft(); screen('audience'); });
$('change-repository').addEventListener('click', () => { saveDraft(); state.sequence++; state.loading = false; $('load-repository').disabled = false; status(''); state.source = null; state.overview = null; state.graph = null; state.selected = null; state.draftKey = null; state.preparation = null; $('repository-url').value = ''; $('repository-ref').value = ''; document.querySelector('.repository-options').open = false; screen('repository'); history.pushState(null, '', '/'); });
$('select-whole').addEventListener('click', () => selectNode(null));
let diagramZoom = 1;
function zoomDiagram(value) { diagramZoom = Math.min(3, Math.max(.5, value)); const chart = $('repository-diagram').querySelector('svg'); if (chart) chart.style.width = diagramZoom === 1 ? '100%' : `${diagramZoom * 100}%`; }
$('diagram-fit').addEventListener('click', () => zoomDiagram(1));
$('diagram-zoom-in').addEventListener('click', () => zoomDiagram(diagramZoom * 1.3));
$('diagram-zoom-out').addEventListener('click', () => zoomDiagram(diagramZoom / 1.3));
$('repository-form').addEventListener('submit', e => { e.preventDefault(); openRepository($('repository-url').value.trim(), $('repository-ref').value.trim()); });
async function openRepository(url, reference, record = true) {
  if (!/^https:\/\/github\.com\/[A-Za-z0-9-]+\/[A-Za-z0-9_.-]+\/?$/.test(url)) {
    $('repository-error').hidden = false; $('repository-error').textContent = 'https://github.com/所有者/リポジトリ の形で入力してください。'; $('repository-url').setAttribute('aria-invalid', 'true'); $('repository-url').focus(); return;
  }
  const seq = ++state.sequence; state.loading = true; saveDraft();
  $('repository-error').hidden = true; $('repository-url').removeAttribute('aria-invalid'); $('load-repository').disabled = true;
  status('リポジトリを読み込み、構成図を作成しています。');
  await run(async () => {
    const read = await api('understand', { operation: 'open', url, reference });
    const [map, view] = await Promise.all([
      api('understand', { operation: 'architecture', repository: read.source.repository, commit: read.source.commit }), api('case')
    ]);
    if (seq !== state.sequence) return;
    state.source = read.source; state.overview = read.result; state.graph = map.result; state.view = view; state.selected = null; state.draftKey = null; state.preparation = null;
    $('system-heading').textContent = read.source.repository;
    $('repository-version').textContent = `読み込んだ版：${read.source.commit.slice(0, 12)}`;
    $('map-scope').textContent = `${map.result.scope}。実際の配置や動作は未確認です。`;
    $('diagram-summary').textContent = map.result.summary;
    $('map-basis').replaceChildren(el('p', {}, map.result.basis), el('p', {}, map.result.unknown), ...(map.result.notices || []).map(n => el('p', {}, n)), el('p', {}, `本文を読んだファイル：${map.result.read_files}件。構成に関係する候補の未読：${map.result.unread_files}件。${map.result.inventory_truncated ? 'ファイル一覧自体も途中で切れています。' : ''}`));
    const params = new URLSearchParams(location.search);
    const focus = params.get('url') === url ? findObject(params.get('focus')) : null;
    selectNode(focus); renderConcerns(); screen('system'); diagramZoom = 1;
    await renderGraph();
    if (seq !== state.sequence) return;
    if (record) history.pushState(null, '', `/?url=${encodeURIComponent(url)}&ref=${encodeURIComponent(read.source.commit)}${focus ? `&focus=${encodeURIComponent(focus.id)}` : ''}`);
    status('構成図を表示しました。気になる部分を選んでください。');
  }, () => seq === state.sequence);
  if (seq === state.sequence) { state.loading = false; $('load-repository').disabled = false; }
}
function id(prefix) { return `${prefix}-${crypto.randomUUID()}`; }
function meta(statement) { try { return JSON.parse(statement.origin); } catch { return null; } }
function concerns() {
  return (state.view?.workspace.statements || []).filter(s => {
    const m = meta(s); return m?.kind === 'repository_concern' && m.repository === state.source.repository;
  });
}
async function change(action, reason) {
  // 保存時に最新版を読む。競合を無視して古い状態で上書きしない。
  const view = await api('case');
  const next = await api('change', { expected_revision: view.workspace.revision, actor: `ブラウザ利用者（${AUDIENCE[state.audience]}・本人確認なし）`, reason, action });
  if (!state.view || next.workspace.revision >= state.view.workspace.revision) state.view = next;
  return next;
}
$('concern-form').addEventListener('submit', e => {
  e.preventDefault();
  if (state.saving || !state.source || !$('concern-text').value.trim()) return;
  const source = state.source, node = state.selected, model = state.graph, label = concernLabel(node), audience = state.audience, text = $('concern-text').value.trim(), savedDraftKey = state.draftKey;
  const ref = node?.refs?.[0];
  state.saving = true; $('save-concern').disabled = true;
  run(async () => {
    const origin = JSON.stringify({ kind: 'repository_concern', repository: source.repository, commit: source.commit, node: node?.id || null, object_type: node?.object_type || 'whole', model_id: model.model_id, model_revision: model.model_revision, label });
    await change({ op: 'add_statement', statement: { id: id('concern'), kind: 'unanswered', role: audience, text, origin, source: ref ? { repository: source.repository, commit: source.commit, path: ref.path, line: ref.line, end_line: ref.end_line || ref.line } : null, check_ids: [] } }, '構成図から不安・疑問を記録した');
    remember(savedDraftKey, '');
    if (state.source?.repository === source.repository && state.source.commit === source.commit) {
      if (state.draftKey === savedDraftKey && $('concern-text').value.trim() === text) $('concern-text').value = '';
      renderConcerns(); status('疑問を記録しました。検証したい内容を具体化できます。');
    }
  }).finally(() => { state.saving = false; $('save-concern').disabled = false; });
});
function checkFor(statement) {
  return state.view.workspace.checks.find(c => c.origin === `repository-concern:${statement.id}`);
}
function renderConcerns() {
  const root = $('concerns'); root.replaceChildren();
  const list = concerns();
  if (!list.length) { root.append(el('p', { className: 'note' }, 'まだ疑問はありません。構成図の部分を選ぶか、システム全体への疑問を書いてください。')); return; }
  for (const statement of list) {
    const m = meta(statement), check = checkFor(statement);
    const card = el('article', { className: 'concern-card', 'data-concern': statement.id }, el('p', { className: 'concern-location' }, `${m.label}${m.commit === state.source.commit ? '' : '（以前の版への疑問）'}`), el('h3', {}, statement.text));
    const target = m.commit === state.source.commit ? findObject(m.node) : null;
    if (target) card.append(button('図のこの部分を見る', () => { selectNode(target); $('repository-diagram').scrollIntoView({ block: 'center' }); }));
    if (m.commit === state.source.commit && m.model_revision && m.model_revision !== state.graph.model_revision) card.append(el('p', { className: 'note' }, '疑問を残した後に、図の読み取り範囲や説明が変わっています。'));
    if (m.commit === state.source.commit && m.node && !target) card.append(el('p', { className: 'note' }, '疑問を残した部分は、現在の図では特定できていません。元の出所を保持しています。'));
    if (check) {
      const cv = state.view.checks.find(c => c.check_id === check.id);
      card.append(el('p', {}, `確かめたい動作：${check.text}`));
      card.append(el('p', { className: 'note' }, cv?.evidence.length ? `検証結果：${({ satisfied: '条件を満たす', not_satisfied: '条件を満たさない', unknown: '判定できない' })[cv.verdict]}。${cv.reason}` : '検証内容を記録しました。まだ実行していません。'));
      card.append(workbenchLink('検証の設定へ進む', check, m, 'trial'));
      if (cv?.evidence.length) card.append(workbenchLink('結果を見て改善を検討する', check, m, 'result'));
    } else {
      card.append(button('検証したい内容を決める', () => prepareConcern(card, statement), 'solid-fill'));
    }
    root.append(card);
    if (!check && state.preparation?.statementId === statement.id) prepareConcern(card, statement);
  }
}
function prepareConcern(card, statement) {
  if (card.querySelector('form')) return;
  if (state.preparation?.statementId !== statement.id) state.preparation = { statementId: statement.id, text: '' };
  const label = el('label', { for: `expect-${statement.id}`, className: 'system-label' }, 'どんな動作を確かめたいですか');
  const text = el('textarea', { id: `expect-${statement.id}`, rows: 3, className: 'system-input', required: '', maxlength: 2000 });
  text.value = state.preparation.text; text.addEventListener('input', () => { state.preparation = { statementId: statement.id, text: text.value }; });
  const form = el('form', {}, label, text, el('p', { className: 'note' }, '期待する動作を具体的に書いてください。起動方法や確認方法は、次の画面で技術者が設定します。'));
  const submit = el('button', { type: 'submit', className: 'dads-button', 'data-type': 'solid-fill', 'data-size': 'sm' }, '検証内容を記録する');
  form.append(submit);
  form.addEventListener('submit', e => {
    e.preventDefault(); if (!text.value.trim() || submit.disabled) return; submit.disabled = true;
    const m = meta(statement);
    run(async () => {
      // 二重保存や再読込後の再操作では既存の条件を再利用する。
      state.view = await api('case');
      if (!checkFor(statement)) {
        const slug = m.repository.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 80);
        const prefix = `${slug}-`, checks = state.view.workspace.checks;
        let n = 1; while (checks.some(c => c.id === prefix + n)) n++;
        const source = statement.source;
        await change({ op: 'add_check', check: { id: prefix + n, version: 1, text: text.value.trim(), origin: `repository-concern:${statement.id}`, source: source || null, observation: null, rule: 'manual_only', dependencies: [], questions: [] } }, `疑問「${statement.text}」から検証したい動作を記録した`);
      }
      state.preparation = null; renderConcerns(); status('検証内容を記録しました。設定へ進めます。');
    }).finally(() => { submit.disabled = false; });
  });
  card.append(form); text.focus();
}
function workbenchLink(label, check, m, step) {
  const href = `/workbench?url=${encodeURIComponent('https://github.com/' + m.repository)}&ref=${encodeURIComponent(check.source?.commit || m.commit)}&check=${encodeURIComponent(check.id)}${m.node ? `&focus=${encodeURIComponent(m.node)}` : ''}&from=system#${step}`;
  return el('a', { href, className: 'dads-button', 'data-type': 'outline', 'data-size': 'sm' }, label);
}
async function restore() {
  const audience = remembered('eel-audience'); state.audience = AUDIENCE[audience] ? audience : null;
  const params = new URLSearchParams(location.search);
  if (!state.audience) { screen('audience'); return; }
  screen('repository');
  if (params.get('url')) { $('repository-url').value = params.get('url'); $('repository-ref').value = params.get('ref') || ''; await openRepository(params.get('url'), params.get('ref') || '', false); }
}
window.addEventListener('popstate', () => { saveDraft(); state.sequence++; state.loading = false; $('load-repository').disabled = false; restore(); });
restore();
