// 公開リポジトリの読取だけを行う任意の確認。対象コードやセットアップは実行しない。
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
import { verifyConnections } from './diagram-geometry.mjs';
const [repositoryUrl, reference = ''] = process.argv.slice(2);
if (!/^https:\/\/github\.com\/[\w.-]+\/[\w.-]+$/.test(repositoryUrl || '')) throw Error('公開GitHubのURLと任意のコミットを指定してください');
const root = resolve(import.meta.dirname, '..'), temp = await mkdtemp(join(tmpdir(), 'eel-architecture-live-')), out = join(root, 'test-results');
await mkdir(out, { recursive: true });
const child = spawn(join(root, 'target/debug/execution-evidence-lab'), ['app', join(temp, 'app'), '--port', '0']);
let browser, stderr = ''; child.stderr.on('data', d => stderr += d);
try {
  const base = await new Promise((accept, reject) => {
    const timer = setTimeout(() => reject(Error('startup timeout')), 15000); let text = '';
    child.stdout.on('data', d => { text += d; const m = /http:\/\/127\.0\.0\.1:\d+/.exec(text); if (m) { clearTimeout(timer); accept(m[0]); } });
    child.once('exit', () => { clearTimeout(timer); reject(Error(stderr)); });
  });
  browser = await chromium.launch({ executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.goto(base); await page.getByRole('button', { name: /^非技術者/ }).click();
  await page.locator('#repository-url').fill(repositoryUrl);
  if (reference) { await page.getByText('ブランチ・コミットを指定する', { exact: true }).click(); await page.locator('#repository-ref').fill(reference); }
  const response = page.waitForResponse(r => r.url().endsWith('/api/v1/system-diagram'), { timeout: 90000 });
  await page.getByRole('button', { name: '構成図を作成する', exact: true }).click();
  const result = await (await response).json();
  if (!result.result) throw Error(JSON.stringify(result));
  await page.locator('#repository-diagram[aria-busy="false"] svg').waitFor({ timeout: 90000 });
  await verifyConnections(page, result.result);
  const name = repositoryUrl.split('/').at(-1).replace(/[^\w.-]/g, '-');
  await page.screenshot({ path: join(out, name + '-operator.png'), fullPage: true });
  const before = await page.locator('[data-node]').evaluateAll(ns => ns.map(n => [n.dataset.node, n.getAttribute('transform')]));
  await page.getByRole('button', { name: /立場：非技術者/ }).click(); await page.getByRole('button', { name: /^技術者/ }).click();
  await page.locator('#repository-diagram[aria-busy="false"] svg').waitFor();
  assert.deepEqual(await page.locator('[data-node]').evaluateAll(ns => ns.map(n => [n.dataset.node, n.getAttribute('transform')])), before);
  await page.screenshot({ path: join(out, name + '-developer.png'), fullPage: true });
  const graph = result.result;
  await writeFile(join(out, name + '-architecture.json'), JSON.stringify(result, null, 2));
  assert.equal(result.references.unverified.length, 0); assert.deepEqual(errors, []);
  console.log(JSON.stringify({ repository: graph.repository, commit: graph.commit, nodes: graph.nodes.map(n => ({ title: n.title, technology: n.technology })), edges: graph.edges.map(e => ({ title: e.title, type: e.relation_type })), read: graph.read_files, unread: graph.unread_files, notices: graph.notices, human_evaluation: 'not_conducted' }));
} finally { child.kill('SIGTERM'); await browser?.close(); await rm(temp, { recursive: true, force: true }); }
