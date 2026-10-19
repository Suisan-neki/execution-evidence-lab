// 切り替えの記録はRustに保存する。実際の記入者・承認者を推測しない。
export async function mountModeSwitch(root, { token, prefix = '', onChange = async () => {} }) {
  const label = document.createElement('label');
  label.textContent = '確認モード ';
  const select = document.createElement('select');
  select.setAttribute('aria-label', '確認モードを切り替える');
  const loading = new Option('読み込み中…', ''); loading.disabled = true; loading.hidden = true;
  select.add(loading); select.add(new Option('汎用', 'general')); select.add(new Option('医療', 'medical')); select.value = '';
  label.append(select);
  const status = document.createElement('span'); status.setAttribute('role', 'status');
  const help = document.createElement('p'); help.className = 'note';
  root.replaceChildren(label, status, help);
  let view, pending = false;
  async function request(path, data) {
    const response = await fetch(prefix + '/api/v1/' + path, { method: data ? 'POST' : 'GET', headers: { 'X-Review-Token': token, ...(data ? { 'Content-Type': 'application/json' } : {}) }, ...(data ? { body: JSON.stringify(data) } : {}) });
    const result = await response.json();
    if (!response.ok) throw Error(result.error || `HTTP ${response.status}`);
    return result;
  }
  function render() {
    select.value = view.workspace.profile_history?.at(-1)?.domain || 'general';
    help.textContent = select.value === 'medical' ? '医療の表示・閲覧権限・保存期間も確認する観点です。切り替えだけで安全性は判定しません。' : '扱うデータ・外への通信・復旧を確認する観点です。';
  }
  async function reload() { view = await request('case'); render(); }
  select.disabled = true;
  try { await reload(); select.disabled = false; } catch (e) { status.textContent = e.message; }
  select.addEventListener('change', async () => {
    if (pending) return;
    pending = true; select.disabled = true; status.textContent = '保存しています…';
    const domain = select.value;
    try {
      // 最新版を照合する。失敗時は保存済みの表示に戻し、未保存の選択を現在値にしない。
      view = await request('case');
      view = await request('change', { expected_revision: view.workspace.revision, actor: 'ブラウザ操作（本人確認なし）', reason: `画面で確認モードを${domain === 'medical' ? '医療' : '汎用'}に切り替えた`, action: { op: 'select_profile', domain } });
      status.textContent = `${domain === 'medical' ? '医療' : '汎用'}に切り替えました。結果と履歴は保持しています。`;
      render(); await onChange();
      window.dispatchEvent(new CustomEvent('eel-case-updated', { detail: { prefix } }));
    } catch (e) { render(); status.textContent = `切り替えできませんでした。${e.message}`; }
    finally { pending = false; select.disabled = false; }
  });
  window.addEventListener('eel-case-updated', async e => {
    if (e.detail?.prefix !== prefix || pending) return;
    try { await reload(); } catch (error) { status.textContent = error.message; }
  });
}
