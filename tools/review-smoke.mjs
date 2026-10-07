// 実ブラウザ、HTTP、Rustの試行、永続記録を通す自動確認。人による評価ではない。
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { chromium } from 'playwright';

const root = resolve(import.meta.dirname, '..');
const binary = process.env.REVIEW_BINARY || join(root, 'target/debug/execution-evidence-lab');
const temporary = await mkdtemp(join(tmpdir(), 'review-browser-'));
const casePath = join(temporary, 'case');
const output = join(root, 'test-results');
await mkdir(output, { recursive: true });
execFileSync(binary, ['case', 'init', casePath, '--actor', 'Codex自動試験', '--reason', '合成事例のブラウザ動作確認']);
const server = spawn(binary, ['review', casePath, '--port', '0'], { stdio: ['ignore', 'pipe', 'pipe'] });
let browser;
let stderr = '';
server.stderr.on('data', data => { stderr += data; });
try {
  const url = await new Promise((accept, reject) => {
    const timer = setTimeout(() => reject(Error('server did not start: ' + stderr)), 10000);
    let stdout = '';
    server.stdout.on('data', data => {
      stdout += data;
      const match = /http:\/\/127\.0\.0\.1:\d+/.exec(stdout);
      if (match) { clearTimeout(timer); accept(match[0]); }
    });
    server.once('exit', code => { clearTimeout(timer); reject(Error('server exited: ' + code + stderr)); });
  });
  browser = await chromium.launch({ headless: true, executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 } });
  const page = await context.newPage();
  const failures = [];
  page.on('pageerror', error => failures.push(error.message));
  await page.goto(url);
  await page.locator('#check-save').waitFor();
  await page.locator('#actor').fill('合成・開発者役');
  await page.locator('#reason').fill('ブラウザ試験：現在の保存範囲を確認する');
  const state = async () => (await context.request.get(url + '/api/case')).json();
  const submit = async selector => {
    const response = page.waitForResponse(r => r.request().method() === 'POST');
    await page.locator(selector).click();
    const result = await response;
    assert.equal(result.status(), 200, await result.text());
    await page.waitForFunction(() => document.getElementById('message').textContent.startsWith('記録しました'));
    await page.locator(selector).waitFor({ state: 'visible' });
  };
  for (const [fault, verdict] of [['normal', 'satisfied'], ['save-failure', 'not_satisfied'], ['missing-observation', 'unknown'], ['disconnect', 'not_satisfied']]) {
    await submit(`.demo[data-fault="${fault}"]`);
    assert.equal((await state()).checks[0].verdict, verdict);
    assert.equal((await state()).checks[1].verdict, 'unknown');
  }
  await submit('.demo[data-fault="normal"]');
  await page.locator('#check-save a[href*="-events-"]').first().click();
  assert.ok(await page.locator('#evidence').isVisible(), 'evidence link must open its details container');
  await page.locator('#statement-text').fill('<img src=x onerror="window.injected=true">合成の認識');
  await page.locator('#statement-origin').fill('Codex自動試験・実参加者の発言ではない');
  await page.locator('#statement-checks input[value="save"]').check();
  await submit('#add-statement');
  assert.equal(await page.evaluate(() => window.injected), undefined);
  assert.equal(await page.locator('#statements img').count(), 0);
  assert.deepEqual((await state()).workspace.statements.at(-1).check_ids, ['save']);

  const stale = await context.newPage();
  await stale.goto(url);
  await stale.locator('#check-save').waitFor();
  await stale.locator('#actor').fill('合成・別の画面');
  await stale.locator('#reason').fill('競合の検証');
  await page.locator('#decision-action').selectOption('hold');
  await submit('#decide');
  const rejected = stale.waitForResponse(r => r.request().method() === 'POST');
  await stale.locator('#decide').click();
  assert.equal((await rejected).status(), 409);
  await stale.locator('#message.error').waitFor();
  assert.equal((await state()).workspace.decisions.length, 1);
  await stale.close();

  await page.locator('#role').selectOption('operator');
  await page.locator('#actor').fill('合成・運用担当役');
  await page.locator('#reason').fill('実参加者ではない。誰が何を後で見るかは未回答なので追加確認する');
  await page.locator('#decision-action').selectOption('ask');
  await submit('#decide');
  assert.deepEqual((await state()).workspace.decisions.map(d => d.role), ['developer', 'operator']);
  const later = page.locator('#check-later-view');
  await later.locator('summary').filter({ hasText: '確認事項を追加する' }).click();
  await page.locator('#question-text-later-view').fill('実際の閲覧先はどこか');
  await submit('.add-question[data-check="later-view"]');
  assert.equal((await state()).workspace.checks[1].questions.at(-1).answer, null);
  await page.locator('#answer-later-view-who').fill('試験用の仮回答');
  await page.locator('#origin-later-view-who').fill('合成・運用担当役の回答');
  await submit('.answer[data-check="later-view"][data-question="who"]');
  assert.equal((await state()).workspace.checks[1].questions[0].answer, '試験用の仮回答');

  await page.locator('#context-key').locator('..').evaluate(el => { for (let p = el; p; p = p.parentElement) if (p.tagName === 'DETAILS') p.open = true; });
  await page.locator('#context-key').fill('engine');
  await page.locator('#context-value').fill('合成・コード変更後の版');
  await submit('#context-update');
  assert.equal((await state()).checks[0].verdict, 'unknown');
  assert.ok((await state()).checks[0].evidence.at(-1).invalidated_by.some(v => v.includes('engine')));
  await page.screenshot({ path: join(output, 'review-desktop.png'), fullPage: true });
  await submit('.demo[data-fault="normal"]');
  assert.equal((await state()).checks[0].verdict, 'satisfied');
  assert.equal((await state()).workspace.decisions[0].basis[0].verdict, 'satisfied');
  await page.reload();
  await page.locator('#check-save').waitFor();
  assert.equal((await state()).workspace.decisions.length, 2);
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'mobile overflow');
  await page.screenshot({ path: join(output, 'review-mobile.png'), fullPage: true });
  assert.deepEqual(failures, []);
  console.log(JSON.stringify({ browser: await browser.version(), checks: ['four faults', 'same source event links', 'escaped statements', 'stale writes', 'two synthetic roles', 'question and answer', 'selective invalidation and retest', 'reload persistence', 'mobile layout'], human_evaluation: 'not_conducted' }));
} finally {
  await browser?.close();
  server.kill('SIGTERM');
  await rm(temporary, { recursive: true, force: true });
}
