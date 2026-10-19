// 実APIを使い、モード・説明表示・候補・確認資料と参照アプリの画面を確認する。
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { chromium } from 'playwright';
const root = resolve(import.meta.dirname, '..'), binary = process.env.REVIEW_BINARY || join(root, 'target/debug/execution-evidence-lab');
const temp = await mkdtemp(join(tmpdir(), 'eel-modes-ui-'));
const processes = new Set();
let browser, page;
async function server(program, args) {
  const child = spawn(program, args, { stdio: ['ignore', 'pipe', 'pipe'] }); processes.add(child);
  let stderr = ''; child.stderr.on('data', data => { stderr += data; });
  const url = await new Promise((accept, reject) => {
    let text = ''; const timer = setTimeout(() => reject(Error(stderr || 'server timeout')), 10000);
    child.stdout.on('data', data => { text += data; const match = /http:\/\/127\.0\.0\.1:\d+/.exec(text); if (match) { clearTimeout(timer); accept(match[0]); } });
    child.once('exit', () => { clearTimeout(timer); reject(Error(stderr || 'server exited')); });
  });
  return { url, child };
}
async function stop(child) {
  const exited = new Promise(resolve => child.once('exit', resolve)); child.kill('SIGTERM'); await exited; processes.delete(child);
}
try {
  browser = await chromium.launch({ headless: true, executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  await mkdir(join(root, 'test-results'), { recursive: true });
  const app = await server(binary, ['app', join(temp, 'app'), '--port', '0']);
  const context = await browser.newContext({ acceptDownloads: true, viewport: { width: 1440, height: 1000 } });
  page = await context.newPage(); const errors = [];
  page.on('pageerror', e => errors.push(e.message)); page.setDefaultTimeout(10000);
  await page.goto(app.url);
  await page.locator('#secondary-tools > summary').click();
  const panel = page.locator('#review-tools');
  await panel.getByText('現在は汎用モードです。', { exact: false }).waitFor();
  const token = await page.locator('meta[name=review-token]').getAttribute('content');
  const headers = { 'X-Review-Token': token, 'Content-Type': 'application/json' };
  async function state() { const r = await fetch(app.url + '/api/v1/case', { headers }); assert.equal(r.status, 200); return r.json(); }
  let releaseCatalog, catalogRequested;
  const heldCatalog = new Promise(resolve => { releaseCatalog = resolve; });
  const arrived = new Promise(resolve => { catalogRequested = resolve; });
  const holdCatalog = async route => { const response = await route.fetch(); catalogRequested(); await heldCatalog; await route.fulfill({ response }); };
  await context.route('**/api/v1/profiles', holdCatalog);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('eel-case-updated', { detail: { prefix: '' } })));
  await arrived;
  await panel.getByLabel('モード設定の記入者', { exact: true }).fill('合成の作成者');
  await panel.getByLabel('モード設定の理由', { exact: true }).fill('試験用の医療の観点を選ぶ');
  await panel.getByLabel('確認モード', { exact: true }).selectOption('medical');
  await panel.getByRole('button', { name: '確認モードを記録する', exact: true }).click();
  await panel.getByText('現在は医療モードです。', { exact: false }).waitFor();
  const returned = page.waitForResponse(response => response.url().endsWith('/api/v1/profiles'));
  releaseCatalog(); await returned;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await panel.getByText('現在は医療モードです。', { exact: false }).waitFor();
  assert.equal(await panel.getByLabel('モード設定の記入者', { exact: true }).inputValue(), '合成の作成者');
  await context.unroute('**/api/v1/profiles', holdCatalog);
  let before = await state(); assert.equal(before.workspace.profile_history[0].domain, 'medical');
  await panel.getByText('確認項目の候補を見る', { exact: true }).click();
  await panel.getByLabel('説明の詳しさ', { exact: true }).selectOption('technical');
  await panel.getByText('認証と職種・利用者別の認可を分離して検証します。', { exact: false }).waitFor();
  assert.deepEqual((await state()).workspace, before.workspace, '説明だけの変更で事例を更新しない');
  const access = panel.getByRole('listitem').filter({ hasText: '必要な担当者の閲覧・変更範囲を確認できる' });
  await access.getByRole('button', { name: '候補を追加する', exact: true }).click();
  await panel.getByText('確認事項に追加済み', { exact: true }).waitFor({ state: 'attached' });
  await panel.getByText('確認項目の候補を見る', { exact: true }).click();
  await panel.getByText('確認事項に追加済み', { exact: true }).waitFor();
  const added = await state(); assert.equal(added.checks[0].verdict, 'unknown');
  assert.match(added.workspace.checks[0].origin, /未採用の確認候補/);
  await panel.getByLabel('確認モード', { exact: true }).selectOption('general');
  await panel.getByRole('button', { name: '確認モードを記録する', exact: true }).click();
  await panel.getByText('現在は汎用モードです。', { exact: false }).waitFor();
  assert.deepEqual((await state()).checks, added.checks, 'モード変更で条件と判定を変えない');
  await panel.getByText('他の人と確認する資料', { exact: true }).click();
  await panel.getByRole('button', { name: '現在の結果を確認する', exact: true }).click();
  await panel.locator('[data-review-report]').getByText(/判定できない/).waitFor();
  let [download] = await Promise.all([page.waitForEvent('download'), panel.getByRole('button', { name: '確認資料のJSONを保存する', exact: true }).click()]);
  const saved = JSON.parse(await readFile(await download.path(), 'utf8'));
  assert.equal(saved.domain, 'general'); assert.equal(saved.checks[0].current.verdict, 'unknown');
  const now = await state();
  const updated = await fetch(app.url + '/api/v1/change', { method: 'POST', headers, body: JSON.stringify({ expected_revision: now.workspace.revision, actor: '合成の別操作', reason: 'プレビュー後の更新', action: { op: 'set_context', key: 'test', value: 'changed' } }) });
  assert.equal(updated.status, 200);
  await panel.getByRole('button', { name: '確認資料のHTMLを保存する', exact: true }).click();
  await panel.getByText('事例が更新されています。', { exact: false }).waitFor();
  await panel.getByRole('button', { name: '現在の結果を確認する', exact: true }).click();
  [download] = await Promise.all([page.waitForEvent('download'), panel.getByRole('button', { name: '確認資料のHTMLを保存する', exact: true }).click()]);
  const html = await readFile(await download.path(), 'utf8'); assert.match(html, /判定できない/); assert.doesNotMatch(html, /<script/);
  // 単独のHTMLでも同じ結果が読める。
  await download.saveAs(join(temp, 'review.html'));
  const offline = await context.newPage(); await offline.goto('file://' + join(temp, 'review.html'));
  await offline.getByRole('heading', { name: '確認事項と結果', exact: true }).waitFor();
  await offline.close();
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.screenshot({ path: join(root, 'test-results', `modes-${width}.png`), fullPage: true });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'モード設定で画面全体が横にはみ出さない');
  }
  await page.goto(app.url + '/architecture');
  await page.locator('#review-tools').getByText('現在は汎用モードです。', { exact: false }).waitFor();
  assert.equal(await page.locator('#review-tools').getByLabel('説明の詳しさ', { exact: true }).inputValue(), 'technical');
  assert.deepEqual((await state()).workspace, (await updated.json()).workspace);
  await context.close();
  // 参照アプリそのものをブラウザで操作する。平台のHTTP試行とは別の確認として記録する。
  const storage = join(temp, 'queue-state.json');
  let queue = await server('python3', [join(root, 'examples/queue-display/app.py'), '--port', '0', '--storage', storage]);
  const queueContext = await browser.newContext(), queuePage = await queueContext.newPage();
  queuePage.on('pageerror', error => errors.push(error.message));
  await queuePage.goto(queue.url); await queuePage.getByLabel('呼び出す番号').fill('42');
  await queuePage.getByRole('button', { name: '呼び出す', exact: true }).click();
  await queuePage.locator('#current-number').getByText('42', { exact: true }).waitFor();
  const savedQueue = await readFile(storage);
  await rm(storage); await mkdir(storage);
  await queuePage.getByLabel('呼び出す番号').fill('43');
  await queuePage.getByRole('button', { name: '呼び出す', exact: true }).click();
  await queuePage.getByRole('alert').getByText('保存できませんでした。', { exact: false }).waitFor();
  assert.equal(await queuePage.locator('#current-number').innerText(), '42', '保存失敗で新しい番号を表示しない');
  await rm(storage, { recursive: true }); await writeFile(storage, savedQueue);
  await stop(queue.child);
  queue = await server('python3', [join(root, 'examples/queue-display/app.py'), '--port', '0', '--storage', storage]);
  await queuePage.goto(queue.url); await queuePage.locator('#current-number').getByText('42', { exact: true }).waitFor();
  assert.equal(JSON.parse(await readFile(storage, 'utf8')).number, 42);
  await queuePage.screenshot({ path: join(root, 'test-results', 'queue-display.png'), fullPage: true });
  await queueContext.close();
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ browser_version: browser.version(), checks: ['domain selection', 'independent detail display', 'manual candidates remain unknown', 'domain switch preserves conditions', 'JSON and standalone HTML', 'stale preview rejected', 'mobile layout', 'architecture uses same settings', 'actual queue browser display', 'storage failure keeps previous display', 'queue process restart retains number'], human_evaluation: 'not_conducted' }));
} catch (error) {
  if (page && !page.isClosed()) { console.error(await page.locator('#review-tools').innerText()); await page.screenshot({ path: join(root, 'test-results/modes-failure.png'), fullPage: true }); }
  throw error;
} finally {
  for (const child of processes) child.kill('SIGTERM');
  await browser?.close(); await rm(temp, { recursive: true, force: true });
}
