// 公開GitHubの応答を固定し、実ブラウザ・コード読取・確認事項への引継ぎを確認する。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { chromium } from "playwright";

const root = resolve(import.meta.dirname, ".."), binary = process.env.REVIEW_BINARY || join(root, "target/debug/execution-evidence-lab");
const temp = await mkdtemp(join(tmpdir(), "repository-browser-")), casePath = join(temp, "case"), output = join(root, "test-results");
await mkdir(output, { recursive: true });
execFileSync(binary, ["case", "init", casePath]);
const server = spawn(binary, ["review", casePath, "--port", "0"], { stdio: ["ignore", "pipe", "pipe"] });
let browser;
try {
  const url = await new Promise((accept, reject) => {
    const timer = setTimeout(() => reject(Error("server did not start")), 10000);
    let text = "";
    server.stdout.on("data", data => { text += data; const m = /http:\/\/127\.0\.0\.1:\d+/.exec(text); if (m) { clearTimeout(timer); accept(m[0]); } });
    server.once("exit", code => { clearTimeout(timer); reject(Error("server exited " + code)); });
  });
  browser = await chromium.launch({ headless: true, executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined, args: ["--no-sandbox"] });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, acceptDownloads: true });
  const commit = "a".repeat(40), tree = "b".repeat(40), files = {
    "README.md": "# Example\n\n担当者にデータを送る架空のアプリです。<img src=x onerror=window.injected=true>\n\n起動手順は設定を参照。",
    "Cargo.toml": '[package]\nname = "example-app"',
    "src/main.rs": '//! 操作を受け付けて保存処理を呼ぶ。\nuse example_app::{storage};\nfn main() { storage::save(); }',
    "src/storage.rs": '//! ファイルの書き込みを行う。\npub fn save() {}',
    "package.json": '{"scripts":{"test":"cargo test"}}',
  };
  const newerCommit = "c".repeat(40), newerTree = "d".repeat(40);
  const newerFiles = { ...files, "src/main.rs": '//! 失敗した場合は呼び出し元へ戻る。\nuse example_app::{storage};\nfn main() { if ready() { storage::save(); } }', "src/new.rs": 'fn added() {}' };
  const requests = [];
  const headers = { "access-control-allow-origin": "*", "content-type": "application/json" };
  await context.route("https://api.github.com/**", async route => {
    const target = route.request().url(); requests.push(target);
    let data;
    if (target.endsWith("/example/project")) data = { private: false, default_branch: "main", description: "合成のリポジトリ。実参加者の事例ではない。" };
    else if (target.endsWith("/commits/feature%2Fread")) data = { sha: commit, commit: { tree: { sha: tree } } };
    else if (target.endsWith("/commits/updated")) data = { sha: newerCommit, commit: { tree: { sha: newerTree } } };
    else if (target.includes(`/git/trees/${newerTree}?recursive=1`)) data = { tree: Object.entries(newerFiles).map(([path, text]) => ({ path, size: Buffer.byteLength(text), mode: "100644", type: "blob", sha: createHash("sha1").update(text).digest("hex") })), truncated: false };
    else if (target.includes(`/git/trees/${tree}?recursive=1`)) data = { tree: Object.entries(files).map(([path, text]) => ({ path, size: Buffer.byteLength(text), mode: "100644", type: "blob", sha: createHash("sha1").update(text).digest("hex") })), truncated: false };
    else throw Error("unexpected GitHub URL " + target);
    await route.fulfill({ status: 200, headers, body: JSON.stringify(data) });
  });
  await context.route("https://raw.githubusercontent.com/**", async route => {
    const target = route.request().url(); requests.push(target);
    const selectedCommit = target.includes('/' + newerCommit + '/') ? newerCommit : commit;
    assert.ok(target.includes('/' + selectedCommit + '/'));
    const path = target.split(selectedCommit + '/')[1];
    await route.fulfill({ status: 200, headers: { "access-control-allow-origin": "*", "content-type": "text/plain; charset=utf-8" }, body: (selectedCommit === newerCommit ? newerFiles : files)[path] });
  });
  const page = await context.newPage(), errors = [];
  page.on("pageerror", e => errors.push(e.message));
  await page.goto(url);
  await page.locator("#scan").waitFor();
  await page.screenshot({ path: join(output, "understand-start-desktop.png"), fullPage: true });
  await page.locator("#repository-url").fill("https://github.com/example/project");
  await page.locator("#scan-form summary").click();
  await page.locator("#repository-ref").fill("feature/read");
  await page.locator("#scan").click();
  await page.locator("#workspace").waitFor({ state: "visible" });
  assert.ok((await page.locator("#scan-scope").innerText()).includes(commit.slice(0, 12)));
  assert.equal(await page.locator("#readme-summary img").count(), 0);
  assert.equal(await page.evaluate(() => window.injected), undefined);
  await page.screenshot({ path: join(output, "understand-overview-desktop.png"), fullPage: true });
  await page.locator('#entry-list button[data-path="src/main.rs"]').click();
  assert.ok((await page.locator("#file-purpose").innerText()).includes("操作を受け付けて"));
  await page.locator('#symbol-list button[data-line="3"]').click();
  assert.equal(await page.locator(".code-line.target .line-number").innerText(), "3");
  assert.ok((await page.locator("#file-links a").getAttribute("href")).endsWith(`/src/main.rs#L3`));
  assert.ok((await page.locator('#function-guide').innerText()).includes('storage::save'));
  await page.locator('#outgoing button[data-path="src/storage.rs"]').click();
  assert.ok((await page.locator("#incoming").innerText()).includes("src/main.rs"));
  await page.screenshot({ path: join(output, "understand-code-desktop.png"), fullPage: true });
  const download = page.waitForEvent("download");
  await page.locator("#download").click();
  const saved = await download;
  const path = await saved.path();
  await page.locator("#snapshot-file").setInputFiles(path);
  await page.waitForFunction(() => document.getElementById("notice").textContent.includes("保存された読取記録"));
  await page.locator("#tab-code").click();
  assert.equal(await page.locator("#source").innerText(), "");
  assert.ok(await page.locator("#to-test").isDisabled());
  await page.locator("#tab-overview").click();
  await page.locator('#entry-list button[data-path="src/main.rs"]').click();
  await page.locator('#symbol-list button[data-line="3"]').click();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), "mobile overflow");
  await page.screenshot({ path: join(output, "understand-code-mobile.png"), fullPage: true });
  await page.locator("#to-test").click();
  await page.locator("#new-origin").waitFor({ state: "visible" });
  assert.equal(await page.locator("#new-origin").inputValue(), `https://github.com/example/project/blob/${commit}/src/main.rs#L3`);
  assert.ok((await page.locator("#new-text").getAttribute("placeholder")).includes("main"));
  await page.locator("#actor").fill("合成・コードを読む人");
  await page.locator("#reason").fill("選んだコードで確認したい処理を記録する");
  await page.locator("#new-text").fill("送信したデータの保存結果を読み出して一致を確認できる");
  const posted = page.waitForResponse(r => r.request().method() === "POST");
  await page.locator("#add-check").click();
  const response = await posted; assert.equal(response.status(), 200);
  const state = await response.json(), check = state.workspace.checks.at(-1);
  assert.equal(check.rule, "manual_only");
  assert.equal(check.origin, `https://github.com/example/project/blob/${commit}/src/main.rs#L3`);
  assert.equal(state.checks.at(-1).verdict, "unknown");
  await page.locator(`#check-${check.id} a[href^="https://github.com/"][href*="${commit}"]`).waitFor();
  assert.equal(check.source.commit, commit);
  // 保存済みの固定版へ戻り、別の版を読み、変更箇所と参照先更新を確認する。
  await page.locator(`#check-${check.id} .code-reference a`).click();
  await page.waitForFunction(() => document.getElementById('notice').textContent.includes('固定版のコードを開きました'));
  assert.equal(await page.locator('#file-name').innerText(), 'src/main.rs');
  assert.equal(await page.locator('.code-line.target .line-number').innerText(), '3');
  await page.locator('#compare-ref').fill('updated');
  await page.locator('#compare-version').click();
  await page.waitForFunction(() => document.getElementById('comparison-result').textContent.includes('変更あり'));
  assert.ok((await page.locator('#comparison-result').innerText()).includes('追加'));
  await page.locator('button[data-version-path="src/main.rs"]').click();
  await page.locator('#symbol-list button[data-line="3"]').click();
  assert.ok((await page.locator('#function-guide').innerText()).includes('条件に応じて'));
  await page.locator('#binding-actor').fill('合成・コードを読む人');
  await page.locator('#binding-reason').fill('比較した版に条件を対応づけて再確認する');
  const bindingResponse = page.waitForResponse(r => r.request().method() === 'POST');
  await page.locator('#bind-source').click();
  const bound = await bindingResponse;
  assert.equal(bound.status(), 200);
  const boundState = await bound.json(), boundCheck = boundState.workspace.checks.find(c => c.id === check.id);
  assert.equal(boundCheck.source.commit, newerCommit);
  assert.equal(boundCheck.version, 2);
  assert.equal(boundState.checks.find(c => c.check_id === check.id).verdict, 'unknown');
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.screenshot({ path: join(output, 'understand-version-desktop.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'version comparison mobile overflow');
  await page.screenshot({ path: join(output, 'understand-version-mobile.png'), fullPage: true });
  await page.locator('#back-check').click();
  await page.locator(`#check-${check.id} .code-reference a`).waitFor();
  assert.ok((await page.locator(`#check-${check.id}`).innerText()).includes(newerCommit.slice(0,12)));
  await page.goto(url);
  await page.locator("#repository-url").fill("https://github.com/example/project/tree/main");
  await page.locator("#scan").click();
  await page.locator("#notice.error").waitFor();
  assert.ok((await page.locator("#notice").innerText()).includes("ファイルやブランチ"));
  assert.deepEqual(errors, []);
  // 固定応答の画面試験と実GitHubの読取は別の結果として記録する。
  const liveContext = await browser.newContext({ viewport: { width: 1440, height: 1100 } });
  const livePage = await liveContext.newPage();
  let liveResult;
  try {
    await livePage.goto(url);
    await livePage.locator("#repository-url").fill("https://github.com/Suisan-neki/execution-evidence-lab");
    await livePage.locator("#scan").click();
    await livePage.waitForFunction(() => !document.getElementById("scan").disabled, null, { timeout: 90000 });
    if (await livePage.locator("#notice.error").count()) throw Error(await livePage.locator("#notice").innerText());
    assert.equal(await livePage.locator("#repo-name").innerText(), "Suisan-neki/execution-evidence-lab");
    await livePage.locator('#entry-list button[data-path="src/main.rs"]').click();
    assert.ok((await livePage.locator("#source").innerText()).includes("fn main"));
    liveResult = { status: "passed", repository: "Suisan-neki/execution-evidence-lab", scope: await livePage.locator("#scan-scope").innerText() };
    await livePage.screenshot({ path: join(output, "understand-live-github-desktop.png"), fullPage: true });
  } catch (error) { liveResult = { status: "not_verified", reason: error.message }; }
  await liveContext.close();
  console.log(JSON.stringify({ browser: await browser.version(), public_api: "fixed responses", checks: ["commit-pinned GitHub requests", "README and declarations", "source and references", "escaped code", "record export and import", "code to unverified condition", "function guide and candidate calls", "return to pinned code", "version comparison and reference update", "invalid URLs", "mobile layout"], human_evaluation: "not_conducted" }));
  console.log(JSON.stringify({ live_github: liveResult }));
} finally {
  await browser?.close(); server.kill("SIGTERM"); await rm(temp, { recursive: true, force: true });
}
