// 実Rust APIと合成の読取記録を使う。公開GitHubへの接続に依存せず画面遷移を確認する。
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
const root = resolve(import.meta.dirname, '..'), temp = await mkdtemp(join(tmpdir(), 'eel-journey-'));
const data = join(temp, 'app'), output = join(root, 'test-results');
const { snapshot } = JSON.parse(await readFile(join(root, 'tests/fixtures/reader/navigation.json'), 'utf8'));
snapshot.files['app.py'] = 'import sys\nif __name__ == "__main__":\n    print(sys.argv[1:])\n';
snapshot.inventory.push({ path: 'app.py', mode: '100644', size: snapshot.files['app.py'].length });
await mkdir(join(data, 'snapshots', snapshot.fullName), { recursive: true });
await writeFile(join(data, 'snapshots', snapshot.fullName, snapshot.commit + '.json'), JSON.stringify(snapshot));
await mkdir(output, { recursive: true });
const child = spawn(process.env.REVIEW_BINARY || join(root, 'target/debug/execution-evidence-lab'), ['app', data, '--port', '0'], { stdio: ['ignore', 'pipe', 'pipe'] });
let stderr = ''; child.stderr.on('data', data => { stderr += data; });
let browser, page;
try {
  const url = await new Promise((accept, reject) => {
    let stdout = ''; const timeout = setTimeout(() => reject(Error(stderr || 'startup timeout')), 15000);
    child.stdout.on('data', data => { stdout += data; const m = /http:\/\/127\.0\.0\.1:\d+/.exec(stdout); if (m) { clearTimeout(timeout); accept(m[0]); } });
    child.once('exit', () => { clearTimeout(timeout); reject(Error(stderr || 'server exited')); });
  });
  browser = await chromium.launch({ headless: true, executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  page = await context.newPage(); page.setDefaultTimeout(10000);
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.goto(url);
  const token = await page.locator('meta[name=review-token]').getAttribute('content');
  const headers = { 'X-Review-Token': token, 'Content-Type': 'application/json' };
  const state = async (prefix = '') => (await context.request.get(url + prefix + '/api/v1/case', { headers })).json();
  const selectMode = () => page.getByLabel('確認モードを切り替える', { exact: true });
  const shot = name => page.screenshot({ path: join(output, 'journey-' + name + '.png'), fullPage: true });
  assert.equal(await page.locator('#review-tools').isVisible(), false);
  assert.equal(await page.locator('#person').isVisible(), false);
  assert.equal(await page.locator('#repo-url').isVisible(), false, '対象の選択とURL入力を別の画面にする');
  await context.route('**/api/v1/change', route => route.fulfill({ status: 409, contentType: 'application/json', body: JSON.stringify({ error: '合成の更新競合' }) }));
  await selectMode().selectOption('medical');
  await page.getByRole('status').filter({ hasText: '切り替えできませんでした' }).waitFor();
  assert.equal(await selectMode().inputValue(), 'general');
  assert.equal((await state()).workspace.profile_history.length, 0);
  await context.unroute('**/api/v1/change');
  await selectMode().selectOption('medical');
  await page.getByRole('status').filter({ hasText: '医療に切り替えました' }).waitFor();
  const original = (await state()).workspace;
  assert.equal(original.profile_history.at(-1).domain, 'medical');
  await shot('start');

  let releaseExecution, executionArrived;
  const heldExecution = new Promise(resolve => { releaseExecution = resolve; });
  const executionRequested = new Promise(resolve => { executionArrived = resolve; });
  const holdExecution = async route => { const response = await route.fetch(); executionArrived(); await heldExecution; await route.fulfill({ response }); };
  await context.route('**/api/v1/execution', holdExecution);
  // 読み込みではコードを実行せず、ページを順に進める。戻る・再読込で同じ固定版へ戻る。
  await page.getByRole('link', { name: 'GitHubのURLを入力する', exact: true }).click();
  await page.locator('#repo-url').fill('https://github.com/' + snapshot.fullName);
  await page.locator('#repo-ref').fill(snapshot.commit);
  await page.getByRole('button', { name: '読み込む', exact: true }).click();
  await page.locator('#understand:not([hidden])').waitFor();
  assert.equal(await page.locator('#open').isVisible(), false);
  assert.equal(await page.locator('#person').isVisible(), false);
  await page.getByRole('button', { name: '期待する動作を確認する', exact: true }).click();
  await page.locator('#person').getByLabel('名前').fill('合成の作成者');
  await page.locator('#expect-text').fill('入力した値を後から読み出せる');
  await executionRequested; releaseExecution();
  await page.locator('#status', { hasText: /固定して読み込みました/ }).waitFor();
  assert.equal(await page.locator('#expect-text').inputValue(), '入力した値を後から読み出せる', '遅い読取応答で書きかけの期待を消さない');
  await context.unroute('**/api/v1/execution', holdExecution);
  await page.getByRole('button', { name: 'この動作を確かめる', exact: true }).click();
  await page.locator('[data-check="example-project-1"]').waitFor();
  assert.equal(await page.locator('[data-check] .verdict').isVisible(), false, '期待の記入直後に未判定の警告を前面へ出さない');
  await page.goBack(); await page.locator('#expect-text').waitFor();
  await page.goBack(); await page.locator('#understand:not([hidden])').waitFor();
  await page.goForward();
  await page.goForward(); await page.locator('#expect:not([hidden])').waitFor();
  await page.reload(); await page.locator('[data-check="example-project-1"]').waitFor();
  const card = page.locator('[data-check="example-project-1"]');
  await card.getByText('実装と確認方法を対応づける（技術者）', { exact: true }).click();
  await card.getByLabel('コードの行', { exact: true }).fill(`https://github.com/${snapshot.fullName}/blob/${snapshot.commit}/app.py#L3`);
  await card.getByLabel('この期待は、どのコードがどう動くことで満たされるか').fill('入力を標準出力へ返す。保存処理は未実装。');
  await card.getByLabel('どう確かめるか（確認の方法）').fill('合成の引数が出力に含まれるかを試す。');
  await card.getByRole('button', { name: '対応づけを記録', exact: true }).click();
  await shot('expect');
  await card.getByRole('button', { name: 'この期待を試す', exact: true }).click();
  await shot('trial-setup');
  await page.getByRole('button', { name: '次へ：試す状況を選ぶ', exact: true }).click();
  await page.getByRole('button', { name: '次へ：判定の条件を確認する', exact: true }).click();
  await page.getByRole('button', { name: '実行前に確認する', exact: true }).click();
  await page.getByRole('button', { name: '試験を始める', exact: true }).waitFor();
  assert.equal((await state()).workspace.evidence.length, 0);
  await page.goBack(); await page.locator('#trial-body form').waitFor();
  await page.getByRole('button', { name: '実行前に確認する', exact: true }).click();
  await page.reload(); await page.locator('#trial-body form').waitFor();
  assert.equal((await state()).workspace.evidence.length, 0, '確認画面の再読込では実行しない');
  await page.locator('#steps a[href="#expect"]').click();
  await page.locator('#expect-text').fill('別の動作を確かめる');
  await page.getByRole('button', { name: 'この動作を確かめる', exact: true }).click();
  await page.locator('[data-check="example-project-2"]').getByRole('button', { name: '結果を見る', exact: true }).click();
  await page.getByLabel('結果を見る期待', { exact: true }).selectOption('example-project-1');
  assert.equal(new URL(page.url()).searchParams.get('check'), 'example-project-1');
  await page.reload(); await page.locator('#result:not([hidden])').waitFor();
  assert.match(await page.locator('#current-check').innerText(), /入力した値を後から読み出せる/);
  assert.equal(await page.getByLabel('結果を見る期待', { exact: true }).inputValue(), 'example-project-1');
  await page.goBack(); await page.locator('#result:not([hidden])').waitFor();
  assert.equal(await page.getByLabel('結果を見る期待', { exact: true }).inputValue(), 'example-project-2');
  const main = (await state()).workspace;
  assert.equal(main.checks.length, 2); assert.equal(main.evidence.length, 0);
  await shot('selected-result');

  // 同じサーバーから独立したデモへ移動する。戻っても元の条件とモードを保持する。
  await page.getByRole('link', { name: '対象を選び直す', exact: true }).click();
  await page.getByRole('link', { name: 'デモを始める', exact: true }).click();
  await page.getByRole('heading', { name: '仕組みを知る', exact: true }).waitFor();
  assert.equal((await state('/demo')).workspace.evidence.length, 0);
  await selectMode().selectOption('medical');
  await page.getByRole('status').filter({ hasText: '医療に切り替えました' }).waitFor();
  await page.getByRole('button', { name: '次へ：試す内容を選ぶ', exact: true }).click();
  await page.goBack(); await page.getByRole('heading', { name: '仕組みを知る', exact: true }).waitFor();
  await page.goForward(); await page.getByRole('heading', { name: '試す内容を選ぶ', exact: true }).waitFor();
  const scenarios = [['normal', 'satisfied'], ['fail_before_save', 'not_satisfied'], ['omit_save_observation', 'unknown'], ['disconnect_before_receive', 'not_satisfied']];
  let count = 0;
  for (const [fault, verdict] of scenarios) {
    await page.locator(`input[value="${fault}"]`).check();
    await page.getByRole('button', { name: '次へ：実行内容を確認する', exact: true }).click();
    assert.equal((await state('/demo')).workspace.evidence.length, count, '選択・確認画面への移動だけでは実行しない');
    await page.reload();
    await page.getByLabel('試験する人の名前', { exact: true }).fill('合成の実演者');
    await page.getByRole('button', { name: '試験を始める', exact: true }).click();
    await page.getByRole('heading', { name: '結果を見る', exact: true }).waitFor();
    const current = await state('/demo'); count++;
    assert.equal(current.workspace.evidence.length, count);
    assert.equal(current.checks.find(c => c.check_id === 'save').verdict, verdict);
    assert.equal(current.workspace.evidence.at(-1).review_profile.domain, 'medical');
    assert.equal(current.workspace.evidence.at(-1).manifest.fault, fault);
    assert.equal(current.checks.find(c => c.check_id === 'later-view').verdict, 'unknown');
    await page.reload(); await page.getByRole('heading', { name: '結果を見る', exact: true }).waitFor();
    assert.equal((await state('/demo')).workspace.evidence.length, count, '結果の再読込では再試行しない');
    await shot('result-' + fault);
    await page.getByRole('button', { name: '別の状況で試す', exact: true }).click();
  }
  const demoBefore = await state('/demo');
  await selectMode().selectOption('general');
  await page.getByRole('status').filter({ hasText: '汎用に切り替えました' }).waitFor();
  assert.deepEqual((await state('/demo')).checks, demoBefore.checks);
  assert.deepEqual((await state()).workspace, main, 'デモの実行・モード変更が元の事例を変更しない');

  // デモの名前空間でも通常のトークン・Origin検証を維持する。
  assert.equal((await context.request.get(url + '/demo/api/v1/case')).status(), 403);
  const stale = { expected_revision: 1, actor: '合成', reason: '競合確認', action: { op: 'select_profile', domain: 'medical' } };
  assert.equal((await context.request.post(url + '/demo/api/v1/change', { headers, data: stale })).status(), 409);
  assert.equal((await context.request.post(url + '/demo/api/v1/change', { headers: { ...headers, Origin: 'https://example.invalid' }, data: stale })).status(), 403);
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    await shot('choices-' + width);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
    await page.getByRole('link', { name: '対象を選ぶ画面に戻る', exact: true }).click();
    await page.getByRole('heading', { name: '何を確かめますか', exact: true }).waitFor();
    await page.waitForFunction(() => { const s = document.querySelector('[aria-label="確認モードを切り替える"]'); return !s.disabled && s.value === 'medical'; });
    await shot('start-' + width);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
    await page.getByRole('link', { name: 'デモを始める', exact: true }).click();
    await page.getByRole('button', { name: '次へ：試す内容を選ぶ', exact: true }).click();
  }
  assert.deepEqual(errors, []);
  assert.deepEqual((await readdir(join(data, 'case'))).sort(), ['revisions']);
  console.log(JSON.stringify({ checks: ['guided pages', 'back/forward/reload', 'no execution before explicit start', 'four real trials', 'unknown distinct from failure', 'isolated demo case', 'browser domain switching preserves results', 'token/origin/revision checks', '1440/390/320px'], human_evaluation: 'not_conducted' }));
} catch (e) {
  if (page && !page.isClosed()) { console.error(await page.locator('body').innerText()); await page.screenshot({ path: join(output, 'journey-failure.png'), fullPage: true }); }
  throw e;
} finally {
  child.kill('SIGTERM'); await browser?.close(); await rm(temp, { recursive: true, force: true });
}
