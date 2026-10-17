// 一般の保存・対応づけは実API、観測の表示の反例は合成データ。--realではCIの実OpenEMR事例を使う。
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, rm, readFile, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { chromium } from "playwright";
const root = resolve(import.meta.dirname, ".."),
  binary =
    process.env.REVIEW_BINARY ||
    join(root, "target/debug/execution-evidence-lab");
const temp = await mkdtemp(join(tmpdir(), "eel-architecture-ui-")),
  real = process.argv.includes("--real");
let browser;
async function withServer(casePath, fn) {
  const s = spawn(binary, ["review", casePath, "--port", "0"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let errors = "";
  s.stderr.on("data", (d) => (errors += d));
  try {
    const url = await new Promise((accept, reject) => {
      let stdout = "";
      const timer = setTimeout(
        () => reject(Error(errors || "server timeout")),
        10000,
      );
      s.stdout.on("data", (d) => {
        stdout += d;
        const m = /http:\/\/127\.0\.0\.1:\d+/.exec(stdout);
        if (m) {
          clearTimeout(timer);
          accept(m[0]);
        }
      });
      s.once("exit", () => {
        clearTimeout(timer);
        reject(Error(errors || "server exited"));
      });
    });
    await fn(url);
  } finally {
    s.kill("SIGTERM");
  }
}
try {
  const launch = () =>
    chromium.launch({
      headless: true,
      executablePath: process.env.REVIEW_BROWSER_EXECUTABLE || undefined,
      args: [
        "--no-sandbox",
        ...JSON.parse(process.env.REVIEW_BROWSER_ARGS || "[]"),
      ],
    });
  browser = await launch();
  await mkdir(join(root, "test-results"), { recursive: true });
  if (real) {
    for (const fault of ["normal", "database-stopped"])
      await withServer(
        join(
          process.env.EEL_ARCHITECTURE_CASE_ROOT ||
            join(root, "evaluations/ci-openemr"),
          fault,
          "case",
        ),
        async (url) => {
          const context = await browser.newContext({
              viewport: { width: 1440, height: 1100 },
            }),
            page = await context.newPage();
          const errors = [];
          page.on("pageerror", (e) => errors.push(e.message));
          page.setDefaultTimeout(15000);
          await page.goto(url + "/architecture");
          await page
            .getByLabel("対象と構成の版")
            .selectOption("openemr-trial:1");
          await page.getByLabel("図の詳しさ").selectOption("containers");
          await page
            .locator('likec4-view .react-flow__node[data-id="system.db"]')
            .waitFor();
          const token = await page
              .locator("meta[name=review-token]")
              .getAttribute("content"),
            headers = { "X-Review-Token": token };
          const w = (
            await (await fetch(url + "/api/v1/case", { headers })).json()
          ).workspace;
          assert.equal(w.evidence.length, 1);
          await page
            .getByLabel("図に対応づける試行")
            .selectOption(w.evidence[0].id);
          await page.locator("[data-object=db]").click();
          assert.match(
            await page.locator("#details-body").innerText(),
            fault === "normal" ? /○ 期待どおり/ : /？ 観測できていない/,
          );
          await page
            .locator("#details-body")
            .getByRole("button", { name: "試験結果と証拠を見る" })
            .first()
            .click();
          await page.getByRole("dialog").waitFor();
          await page
            .getByRole("dialog")
            .getByRole("button", { name: "元の構成図へ戻る" })
            .click();
          const db = page.locator(
            'likec4-view .react-flow__node[data-id="system.db"]',
          );
          assert.equal(
            await db.getAttribute("data-eel-state"),
            fault === "normal" ? "pass" : "unknown",
          );
          const sql = page.locator("[data-object=sql]");
          assert.match(await sql.innerText(), /対応する観測なし/);
          await page.screenshot({
            path: join(
              root,
              `test-results/architecture-real-${fault}-1440.png`,
            ),
            fullPage: true,
          });
          await page.setViewportSize({ width: 390, height: 1000 });
          await page.screenshot({
            path: join(root, `test-results/architecture-real-${fault}-390.png`),
            fullPage: true,
          });
          assert.deepEqual(errors, []);
          await context.close();
          if (fault === "normal") {
            await browser.close();
            browser = await launch();
          }
        },
      );
  } else {
    const casePath = join(temp, "case");
    execFileSync(binary, [
      "case",
      "init",
      casePath,
      "--template",
      "general",
      "--id",
      "architecture-ui",
      "--title",
      "合成の構成確認",
      "--actor",
      "合成",
      "--reason",
      "画面の確認",
    ]);
    await withServer(casePath, async (url) => {
      const context = await browser.newContext({
          viewport: { width: 1440, height: 1100 },
        }),
        page = await context.newPage(),
        errors = [],
        external = [];
      page.on("pageerror", (e) => errors.push(e.message));
      page.on("request", (r) => {
        if (!r.url().startsWith(url)) external.push(r.url());
      });
      page.setDefaultTimeout(15000);
      await page.goto(url + "/architecture");
      await page
        .getByRole("button", { name: "この試験用構成を事例に追加する" })
        .waitFor();
      await page
        .locator('likec4-view .react-flow__node[data-id="system"]')
        .waitFor();
      assert.equal((await fetch(url + "/api/v1/architecture")).status, 403);
      await page.getByLabel("記入者", { exact: true }).fill("合成の確認者");
      await page.getByLabel("記録する理由").fill("構成と根拠の往復を確認");
      await page
        .getByRole("button", { name: "この試験用構成を事例に追加する" })
        .click();
      await page
        .getByText("試験用構成を事例に追加しました。", { exact: true })
        .waitFor();
      const token = await page
          .locator("meta[name=review-token]")
          .getAttribute("content"),
        headers = {
          "X-Review-Token": token,
          "Content-Type": "application/json",
        };
      let w = (await (await fetch(url + "/api/v1/case", { headers })).json())
        .workspace;
      assert.equal(w.architecture_models.length, 1);
      const change = async (action) => {
        const r = await fetch(url + "/api/v1/change", {
          method: "POST",
          headers,
          body: JSON.stringify({
            expected_revision: w.revision,
            actor: "合成",
            reason: "画面試験の入力",
            action,
          }),
        });
        assert.equal(r.status, 200, await r.clone().text());
        w = (await r.json()).workspace;
      };
      await change({
        op: "add_check",
        check: {
          id: "save",
          version: 1,
          text: "どの担当者が再表示できる必要があるか",
          origin: "合成の未決定条件",
          rule: "manual_only",
          dependencies: [],
          questions: [],
        },
      });
      await page.getByRole("button", { name: "最新版を読み直す" }).click();
      await page.getByLabel("図の詳しさ").selectOption("containers");
      await page
        .locator('likec4-view .react-flow__node[data-id="system.db"]')
        .waitFor();
      const label = page
        .locator("likec4-view")
        .getByText("DBへの接続設定", { exact: true });
      await label.click();
      await page.waitForFunction(
        () =>
          document.querySelector("#details-heading")?.textContent ===
          "DBへの接続設定",
      );
      await page
        .locator('likec4-view .react-flow__node[data-id="system.db"]')
        .click();
      await page.waitForFunction(
        () =>
          document.querySelector("#details-heading")?.textContent === "MariaDB",
      );
      await page
        .getByRole("button", { name: "この確認事項を対応づける" })
        .click();
      await page
        .getByText("対応づけを記録しました。", { exact: true })
        .waitFor();
      await page
        .getByLabel("自分の言葉で記入")
        .fill("DB停止時に誰が対応する？ <img src=x onerror=alert(1)>");
      await page
        .getByRole("button", { name: "疑問・期待を記録して対応づける" })
        .click();
      await page
        .locator("#details-body")
        .getByText("DB停止時に誰が対応する？ <img src=x onerror=alert(1)>", {
          exact: true,
        })
        .waitFor();
      assert.equal(await page.locator("#details-body img").count(), 0);
      const saved = (
        await (await fetch(url + "/api/v1/case", { headers })).json()
      ).workspace;
      assert.equal(saved.architecture_links.length, 2);
      assert.equal(saved.statements.length, 1);
      w = saved;
      const next = structuredClone(saved.architecture_models[0]);
      next.version = 2;
      next.elements[0].title = "新版の補足";
      await change({ op: "add_architecture_model", model: next });
      await page.getByRole("button", { name: "最新版を読み直す" }).click();
      await page.getByLabel("対象と構成の版").selectOption("openemr-trial:2");
      assert.equal(await page.locator("likec4-view").count(), 0);
      assert.match(
        await page.locator("#diagram-status").innerText(),
        /この構成の版の図は未生成/,
      );
      await page.getByLabel("対象と構成の版").selectOption("openemr-trial:1");
      await page.locator("[data-object=db]").focus();
      await page.keyboard.press("Enter");
      assert.equal(
        await page.locator("#details-heading").textContent(),
        "MariaDB",
      );
      // HTTP失敗をDBの未観測へ広げない。実際の保存APIとは分けた合成応答。
      await page.route("**/api/v1/case", async (route) => {
        const response = await route.fetch(),
          v = await response.json();
        const recipe = JSON.parse(
          await readFile(
            join(root, "examples/openemr-browser/recipe.json"),
            "utf8",
          ),
        );
        v.workspace.evidence = [
          {
            id: "synthetic-evidence",
            check_id: "save",
            manifest: {
              correlation: { run_id: "synthetic-db-stopped" },
              mode: "synthetic",
            },
            execution: {
              recipe,
              containers: {
                browser: {
                  version: "synthetic",
                  prepared: true,
                  completed: false,
                  detail: "合成の途中停止",
                  steps: [],
                  responses: [],
                },
                database_reads: [
                  { id: "visit", performed: false, detail: "DB停止の合成記録" },
                ],
              },
            },
          },
        ];
        v.checks[0].evidence = [
          {
            id: "synthetic-evidence",
            summary: {
              tried: "合成のDB停止試験",
              fault: "DB停止",
              fault_applied: "合成",
              items: [
                {
                  id: "visit-row",
                  scope: "受診理由の保存",
                  observed_label: "観測できていない",
                  source: "DBの直接読取",
                  expected: "試験値を含む",
                  detail: "DBは読取不能",
                },
              ],
            },
          },
        ];
        await route.fulfill({ response, json: v });
      });
      await page.route("**/api/v1/architecture", async (route) => {
        const response = await route.fetch(),
          v = await response.json();
        for (const [object_id, observation_id, method, observed] of [
          ["db", "visit-row", "database_rows", "not_observed"],
          ["https", "visit-http", "browser_response", "fail"],
        ])
          v.links.push({
            link: {
              id: "synthetic-" + object_id,
              model_id: "openemr-trial",
              model_version: 1,
              object_id,
              target: {
                kind: "evidence",
                evidence_id: "synthetic-evidence",
                observation_id,
              },
            },
            current_model: true,
            applicable: true,
            method,
            observed,
            detail: "合成の表示確認",
          });
        await route.fulfill({ response, json: v });
      });
      await page.getByRole("button", { name: "最新版を読み直す" }).click();
      await page
        .getByLabel("図に対応づける試行")
        .selectOption("synthetic-evidence");
      await page.locator("[data-object=db]").click();
      assert.match(
        await page.locator("#details-body").innerText(),
        /？ 観測できていない/,
      );
      assert.match(
        await page.locator("[data-object=https]").innerText(),
        /期待と異なる/,
      );
      assert.match(
        await page.locator("[data-object=sql]").innerText(),
        /対応する観測なし/,
      );
      await page
        .locator("#details-body")
        .getByRole("button", { name: "試験結果と証拠を見る" })
        .click();
      await page.getByRole("dialog").waitFor();
      assert.match(await page.getByRole("dialog").innerText(), /DBは読取不能/);
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "元の構成図へ戻る" })
        .click();
      assert.equal(
        await page.locator("#details-heading").textContent(),
        "MariaDB",
      );
      await page
        .locator('likec4-view .react-flow__node[data-id="system.db"]')
        .waitFor();
      assert.equal(
        await page
          .locator('likec4-view .react-flow__node[data-id="system.db"]')
          .getAttribute("data-eel-state"),
        "unknown",
      );
      await page.screenshot({
        path: join(root, "test-results/architecture-1440.png"),
        fullPage: true,
      });
      for (const width of [390, 320]) {
        await page.setViewportSize({ width, height: 1000 });
        await page.screenshot({
          path: join(root, `test-results/architecture-${width}.png`),
          fullPage: true,
        });
        assert.ok(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= innerWidth,
          ),
          "page overflow",
        );
      }
      assert.deepEqual(errors, []);
      assert.deepEqual(external, []);
      await context.close();
    });
  }
  console.log(
    JSON.stringify({
      real_openemr: real,
      checks: real
        ? [
            "normal and DB-stopped real evidence",
            "nested result scope",
            "unknown SQL path",
            "diagram to evidence and back",
            "1440px and 390px",
          ]
        : [
            "LikeC4 HTML embed",
            "same-model context and containers",
            "node selection",
            "API token",
            "persisted model/check/question links",
            "escaped question",
            "HTTP fail versus DB unknown",
            "diagram/evidence round trip",
            "no external asset requests",
            "1440px/390px/320px",
          ],
    }),
  );
} finally {
  if (browser) await browser.close();
  await rm(temp, { recursive: true, force: true });
}
