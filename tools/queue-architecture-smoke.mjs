// 実際の番号表示の試行を図で確認する。古いコードの失敗と新しいコードの結果を分ける。
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
import { request as httpRequest } from 'node:http';
const root = resolve(import.meta.dirname, '..'), binary = process.env.REVIEW_BINARY || join(root, 'target/debug/execution-evidence-lab');
const temp = await mkdtemp(join(tmpdir(), 'eel-queue-diagram-'));
const output = join(temp, 'demo');
execFileSync(process.execPath, [join(root, 'tools/queue-demo.mjs'), output], { stdio: 'pipe' });
const server = spawn(binary, ['review', join(output, 'case'), '--port', '0'], { stdio: ['ignore', 'pipe', 'pipe'] });
let browser;
try {
  const url = await new Promise((accept, reject) => {
    let text = ''; const timer = setTimeout(() => reject(Error('server timeout')), 10000);
    server.stdout.on('data', data => { text += data; const match = /http:\/\/127\.0\.0\.1:\d+/.exec(text); if (match) { clearTimeout(timer); accept(match[0]); } });
    server.once('exit', () => { clearTimeout(timer); reject(Error('server exited')); });
  });
  browser = await chromium.launch({ headless: true, executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 } }), page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message)); page.setDefaultTimeout(15000);
  await page.goto(url + '/architecture');
  const token = await page.locator('meta[name=review-token]').getAttribute('content'), headers = { 'X-Review-Token': token, 'Content-Type': 'application/json' };
  function api(path, body) {
    return new Promise((resolve, reject) => {
      const input = body ? JSON.stringify(body) : null;
      const request = httpRequest(url + '/api/v1/' + path, { method: input ? 'POST' : 'GET', agent: false, headers: { ...headers, ...(input ? { 'Content-Length': Buffer.byteLength(input) } : {}) } }, response => {
        let data = ''; response.setEncoding('utf8'); response.on('data', chunk => { data += chunk; }); response.on('end', () => { try { resolve({ status: response.statusCode, body: JSON.parse(data) }); } catch (error) { reject(error); } }); response.on('error', reject);
      });
      request.on('error', reject); request.end(input);
    });
  }
  const state = (await api('case')).body;
  const architecture = (await api('architecture')).body;
  assert.equal(architecture.layouts.length, 2);
  const trials = state.workspace.evidence;
  await page.getByLabel('対象と構成の版').selectOption('queue-reference:2');
  await page.getByLabel('図の詳しさ').selectOption('containers');
  await page.locator('likec4-view [data-eel-object="storage"]').waitFor();
  for (const [index, expected] of [[1, 'pass'], [2, 'unknown'], [3, 'unknown'], [4, 'pass']]) {
    await page.getByLabel('図に対応づける試行').selectOption(trials[index].id);
    const node = page.locator('likec4-view [data-eel-object="storage"]');
    await page.waitForFunction(expected => {
      function find(root) {
        const match = root.querySelector('[data-eel-object="storage"]');
        if (match) return match;
        for (const item of root.querySelectorAll('*')) { if (item.shadowRoot) { const found = find(item.shadowRoot); if (found) return found; } }
      }
      return find(document)?.getAttribute('data-eel-state') === expected;
    }, expected);
    assert.equal(await node.getAttribute('data-eel-state'), expected);
  }
  await page.locator('likec4-view [data-eel-object="storage"]').click();
  await page.locator('#details-body').getByRole('button', { name: '試験結果と証拠を見る' }).click();
  await page.getByRole('dialog').getByRole('button', { name: '元の構成図へ戻る' }).click();
  assert.equal(await page.locator('likec4-view [data-eel-object="storage"]').getAttribute('data-eel-selected'), 'true');
  await page.getByLabel('対象と構成の版').selectOption('queue-reference:1');
  await page.getByLabel('図に対応づける試行').selectOption(trials[0].id);
  await page.locator('likec4-view [data-eel-object="app"]').click();
  await page.locator('#details-body').getByText('↻ 現在の条件には再確認が必要。観測時の結果を表示しています。', { exact: true }).waitFor();
  assert.match(await page.locator('#details-body').innerText(), /期待と異なる/);
  assert.match(await page.locator('#model-description').innerText(), new RegExp(trials[0].source.commit.slice(0, 12)));
  const question = architecture.links.find(l => l.link.target.kind === 'statement').link;
  await page.locator(`[data-object="${question.object_id}"]`).click();
  await page.locator('#details-body').getByText('保存した番号と表示した番号が違う場合に気づけるか', { exact: true }).waitFor();
  // 新版でも描く構成が変われば、古い図を表示しない。
  const changed = structuredClone(state.workspace.architecture_models.at(-1)); changed.version = 3; changed.elements[0].title = '構成を変えた試験クライアント';
  const response = await api('change', { expected_revision: state.workspace.revision, actor: '合成', reason: '描画する構成の変更', action: { op: 'add_architecture_model', model: changed } });
  assert.equal(response.status, 200);
  await page.getByRole('button', { name: '最新版を読み直す', exact: true }).click();
  await page.getByLabel('対象と構成の版').selectOption('queue-reference:3');
  await page.getByText('この構成の版の図は未生成です。', { exact: false }).waitFor();
  assert.equal(await page.locator('likec4-view').count(), 0);
  await page.getByLabel('対象と構成の版').selectOption('queue-reference:2');
  await page.getByLabel('図に対応づける試行').selectOption(trials[4].id);
  await page.locator('likec4-view [data-eel-object="storage"]').waitFor();
  await mkdir(join(root, 'test-results'), { recursive: true });
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: 1100 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.screenshot({ path: join(root, 'test-results', `queue-architecture-${width}.png`), fullPage: true });
  }
  assert.deepEqual(errors, []);
  await context.close();
  console.log(JSON.stringify({ browser_version: browser.version(), checks: ['queue diagram', 'fixed code versions remain distinct', 'actual pass/fail/unknown/retest outcomes', 'old failure stays historical', 'question and evidence round trip', 'changed structure rejects old layout', '1440/390/320px'], human_evaluation: 'not_conducted' }));
} finally { server.kill('SIGTERM'); await browser?.close(); await rm(temp, { recursive: true, force: true }); }
