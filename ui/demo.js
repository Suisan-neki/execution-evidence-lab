import { mountModeSwitch } from '/ui/mode-switch.js';
import { mountReviewTools } from '/ui/review-tools.js';
const token = document.querySelector('meta[name=review-token]').content;
const screen = document.getElementById('demo-screen'), status = document.getElementById('demo-status'), alert = document.getElementById('demo-alert');
const steps = ['仕組みを知る', '試す内容を選ぶ', '実行前に確認する', '結果を見る'];
const faults = {
  normal: ['通常どおり保存する', '架空データ1件を送信し、ファイルへの書き込みを試します。'],
  fail_before_save: ['保存できない場合を試す', '受信した後、書き込む直前に人工的な失敗を起こします。'],
  omit_save_observation: ['記録を取得できない場合を試す', '保存処理は動かし、保存の観測記録だけを取得しません。'],
  disconnect_before_receive: ['通信が届かない場合を試す', '受信前に通信を切ります。'],
};
const verdicts = { satisfied: '○ 条件を満たす', not_satisfied: '× 条件を満たさない', unknown: '？ 判定できない' };
let view, busy = false;
let fault = 'normal';
try { const saved = sessionStorage.getItem('eel-demo-fault'); if (faults[saved]) fault = saved; } catch { /* 保存なしでも操作できる。 */ }
function node(tag, text, attrs = {}) { const n = document.createElement(tag); if (text) n.textContent = text; for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v); return n; }
function button(text, action, primary = false) { const b = node('button', text, { type: 'button', class: 'dads-button', 'data-type': primary ? 'solid-fill' : 'outline', 'data-size': 'md' }); b.addEventListener('click', action); return b; }
function route() { const n = Number(location.hash.slice(1)); return n >= 1 && n <= 4 ? n : 1; }
function go(n) { if (busy) return; location.hash = String(n); }
async function api(path, data) {
  const r = await fetch('/demo/api/v1/' + path, { method: data ? 'POST' : 'GET', headers: { 'X-Review-Token': token, ...(data ? { 'Content-Type': 'application/json' } : {}) }, ...(data ? { body: JSON.stringify(data) } : {}) });
  const result = await r.json(); if (!r.ok) throw Error(result.error || `HTTP ${r.status}`); return result;
}
async function reload() { view = await api('case'); }
function paragraph(text) { return node('p', text); }
function navigation(back, next, label) { const row = node('div', '', { class: 'journey-actions' }); if (back) row.append(button('戻る', () => go(back))); if (next) row.append(button(label, () => go(next), true)); screen.append(row); }
function render(focus = false) {
  if (!view) return;
  const step = route(); alert.replaceChildren();
  const nav = document.getElementById('demo-steps'); nav.replaceChildren();
  steps.forEach((text, i) => { const li = node('li', '', i + 1 === step ? { 'aria-current': 'step' } : {}); const a = node('a', `${i + 1}. ${text}`, { href: '#' + (i + 1) }); li.append(a); nav.append(li); });
  screen.replaceChildren(node('p', `ステップ ${step} / 4`, { class: 'eyebrow' }), node('h1', steps[step - 1]));
  if (step === 1) {
    screen.append(paragraph('「保存できた」という表示だけで、何が分かるでしょうか。小さな送受信アプリを実際に動かして確かめます。'));
    const flow = node('ol', '', { class: 'process-list' });
    ['架空の利用者のデータを送る', 'サーバーが受け取り、ファイルに保存する', '保存できた場合に応答を返す'].forEach(t => flow.append(node('li', t)));
    screen.append(flow, paragraph('このデモは保存処理を試します。誰が後で閲覧できるかや医療業務で使えるかは未確認です。'));
    navigation(null, 2, '次へ：試す内容を選ぶ');
  } else if (step === 2) {
    screen.append(paragraph('まず通常時を試し、その後に失敗や記録の欠落を比べられます。ここではまだ実行しません。'));
    const choices = node('fieldset'); choices.append(node('legend', 'どの状況を試しますか'));
    Object.entries(faults).forEach(([id, [title, description]]) => {
      const label = node('label', '', { class: 'choice' }), input = node('input', '', { type: 'radio', name: 'demo-fault', value: id }); input.checked = fault === id;
      input.addEventListener('change', () => { fault = id; try { sessionStorage.setItem('eel-demo-fault', fault); } catch { /* 保存なしでも操作できる。 */ } });
      const text = node('span'); text.append(node('strong', title), node('span', description, { class: 'choice-description' })); label.append(input, text); choices.append(label);
    });
    screen.append(choices); navigation(1, 3, '次へ：実行内容を確認する');
  } else if (step === 3) {
    screen.append(node('h2', faults[fault][0]), paragraph(faults[fault][1]), paragraph('「試験を始める」を押すと、localhostの送受信とファイル保存を1回実行します。毎回新しい試行用の保存先を使い、過去の試行は残します。'), paragraph('入力：架空の利用者・30歳。実在する人の情報は使いません。'));
    const name = node('input', '', { id: 'demo-actor', type: 'text', class: 'dads-input-text__input', 'data-size': 'md', autocomplete: 'name' });
    try { name.value = sessionStorage.getItem('eel-demo-actor') || ''; } catch { /* 名前は毎回入力できる。 */ }
    const label = node('label', '試験する人の名前', { for: 'demo-actor' }); screen.append(label, name, paragraph('自己申告の記入者として試行に残します。'));
    const actions = node('div', '', { class: 'journey-actions' });
    actions.append(button('戻る', () => go(2)), button('試験を始める', async () => {
      if (busy) return;
      if (!name.value.trim()) { alert.textContent = '試験する人の名前を入力してください。'; name.focus(); return; }
      busy = true; alert.replaceChildren(); status.textContent = '送信・保存・記録の取得を実行しています…';
      document.querySelectorAll('button, select, input, a').forEach(n => { if ('disabled' in n) n.disabled = true; else n.setAttribute('aria-disabled', 'true'); });
      try {
        try { sessionStorage.setItem('eel-demo-actor', name.value.trim()); } catch { /* 保存なしでも試験できる。 */ }
        await reload();
        view = await api('demo', { expected_revision: view.workspace.revision, actor: name.value.trim(), reason: `ブラウザの練習用デモ：${faults[fault][0]}`, check_id: 'save', fault });
        status.textContent = '試験が終わりました。保存の観測記録から結果を表示します。';
        window.dispatchEvent(new CustomEvent('eel-case-updated', { detail: { prefix: '/demo' } }));
        busy = false; go(4);
      } catch (e) { alert.textContent = `試験を完了できませんでした。${e.message}`; status.textContent = ''; }
      finally { busy = false; document.querySelectorAll('[aria-disabled]').forEach(n => n.removeAttribute('aria-disabled')); document.querySelectorAll('button, select, input').forEach(n => { n.disabled = false; }); }
    }, true)); screen.append(actions);
  } else {
    const check = view.checks.find(c => c.check_id === 'save'), evidence = check.evidence.at(-1);
    if (!evidence) { screen.append(paragraph('まだ試験していません。試す内容を選んでから実行してください。')); navigation(2, 2, '試す内容を選ぶ'); }
    else {
      const raw = view.workspace.evidence.find(r => r.id === evidence.id);
      screen.append(paragraph('今回試したこと：' + (faults[raw.manifest.fault]?.[0] || raw.manifest.fault)));
      screen.append(node('div', '', { class: 'verdict', 'data-verdict': check.verdict }));
      screen.lastChild.append(node('strong', verdicts[check.verdict]), paragraph(check.reason));
      const timeline = node('ol', '', { class: 'process-list', 'aria-label': '今回取得した記録' });
      for (const [title, pass, fail] of [['送信', 'request_sent', 'transport_failed'], ['受信', 'request_received', 'request_blocked'], ['保存', 'save_committed', 'save_failed'], ['利用者への表示', 'display_saved', 'display_failed']]) {
        const event = raw.events.find(e => e.kind === pass || e.kind === fail);
        const text = event ? event.kind === pass ? '完了の記録あり' : '失敗・遮断の記録あり' : '未観測（失敗とは断定できません）';
        const item = node('li', title + '：' + text);
        if (event) item.append(node('span', `／記録${event.sequence}（${event.source}）`, { class: 'note' }));
        timeline.append(item);
      }
      screen.append(timeline);
      screen.append(node('h2', '何が分かったか'), paragraph(evidence.observed.reason), paragraph('確認した条件：' + view.workspace.checks.find(c => c.id === 'save').text), paragraph('次に確認すること：' + check.next.text), node('h2', 'まだ分からないこと'), paragraph('担当者が後で記録を閲覧できるかは、この試験だけでは分かりません。保存の成功と閲覧できることは別の条件です。'));
      const details = node('details'), list = node('ul'); details.append(node('summary', `根拠と試行履歴（${check.evidence.length}件）`));
      [...check.evidence].reverse().forEach(e => { const raw = view.workspace.evidence.find(r => r.id === e.id); list.append(node('li', `${e.run_id}：${verdicts[e.observed.verdict]}／${raw?.manifest?.fault || ''}。${e.observed.reason}`)); }); details.append(list); screen.append(details);
      navigation(3, 2, '別の状況で試す');
    }
  }
  if (focus) { screen.focus(); screen.scrollIntoView({ block: 'start' }); }
}
window.addEventListener('hashchange', () => { if (!busy) render(true); });
try {
  await reload(); render();
  await mountModeSwitch(document.getElementById('mode-switch'), { token, prefix: '/demo', onChange: reload });
  await mountReviewTools(document.getElementById('review-tools'), { token, prefix: '/demo', onChange: async () => { await reload(); render(); } });
} catch (e) { alert.textContent = `デモを開けませんでした。${e.message}。ページを読み直してください。`; }
