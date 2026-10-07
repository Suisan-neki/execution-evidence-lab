// 同じ元情報に対する限定した手順の再現。人の理解・作業時間は測定しない。
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";

const root = resolve(import.meta.dirname, "..");
const binary =
  process.env.REVIEW_BINARY ||
  join(root, "target/debug/execution-evidence-lab");
const output = resolve(
  process.argv[2] || join(root, "evaluations", `methods-${Date.now()}`),
);
mkdirSync(output, { recursive: true });
const read = (path) => JSON.parse(readFileSync(path, "utf8"));
const write = (path, value) =>
  writeFileSync(path, JSON.stringify(value, null, 2) + "\n", { flag: "wx" });
const command = (...args) => execFileSync(binary, args, { encoding: "utf8" });

// workspace.view、生成した判定理由・行動提案・答え合わせは参照しない。
// 通常のチェックリストをコードで再現した比較候補で、新たな手法ではない。
function checklist(input, checkId) {
  const c = input.checks.find((c) => c.id === checkId);
  const mapping = input.declared_mapping
    .filter((m) => m.check_id === checkId)
    .at(-1);
  if (c.questions.some((q) => q.answer == null)) return "unknown";
  if (!mapping || mapping.check_version !== c.version) return "unknown";
  if (
    c.dependencies.some(
      (k) =>
        !mapping.dependencies[k] ||
        mapping.dependencies[k] !== input.context[k],
    )
  )
    return "unknown";
  const run = input.runs.find((r) => r.id === mapping.evidence_id);
  if (
    c.rule !== "demo_save" ||
    run.manifest.mode !== "demo" ||
    run.manifest.condition.kind !== "save_completed" ||
    c.text !== run.manifest.condition.text
  )
    return "unknown";
  const relevant = run.events.filter((e) =>
    ["run_id", "record_id", "attempt_id"].every(
      (k) => e.correlation[k] === run.manifest.correlation[k],
    ),
  );
  const positive = relevant.some(
    (e) => e.source === "server" && e.kind === "save_committed",
  );
  const negative = relevant.some(
    (e) =>
      (e.source === "server" &&
        ["save_failed", "input_rejected"].includes(e.kind)) ||
      (e.source === "fault_gate" && e.kind === "request_blocked"),
  );
  return positive && negative
    ? "unknown"
    : positive
      ? "satisfied"
      : negative
        ? "not_satisfied"
        : "unknown";
}

// 単純なラベル検索の限界を確認する追加条件。有力な比較相手とは呼ばない。
function keyword(input, checkId) {
  const mapping = input.declared_mapping
    .filter((m) => m.check_id === checkId)
    .at(-1);
  const events =
    input.runs.find((r) => r.id === mapping?.evidence_id)?.events || [];
  return events.some((e) => e.kind === "save_committed")
    ? "satisfied"
    : events.some((e) =>
          ["save_failed", "input_rejected", "request_blocked"].includes(e.kind),
        )
      ? "not_satisfied"
      : "unknown";
}

const specs = [
  { id: "normal", fault: "normal", expected: "satisfied" },
  { id: "save-failure", fault: "save-failure", expected: "not_satisfied" },
  { id: "disconnect", fault: "disconnect", expected: "not_satisfied" },
  {
    id: "missing-observation",
    fault: "missing-observation",
    expected: "unknown",
  },
  {
    id: "foreign-attempt",
    expected: "unknown",
    mutate(events) {
      for (const e of events) e.correlation.attempt_id = "foreign-attempt";
    },
  },
  {
    id: "wrong-source",
    expected: "unknown",
    mutate(events) {
      for (const e of events)
        if (e.kind === "save_committed") e.source = "client";
    },
  },
  {
    id: "conflicting-events",
    expected: "unknown",
    mutate(events) {
      events.push({
        ...events.at(-1),
        sequence: events.length + 1,
        source: "server",
        kind: "save_failed",
        detail: "合成の食い違い。実際の処理失敗ではない",
      });
    },
  },
  {
    id: "changed-code",
    expected: "unknown",
    action: { op: "set_context", key: "engine", value: "synthetic-new-code" },
  },
  { id: "changed-condition", expected: "unknown", revise: true },
  {
    id: "unanswered-scope",
    expected: "unknown",
    action: {
      op: "add_question",
      check_id: "save",
      question: {
        id: "scope",
        text: "誰のどの条件か",
        answer: null,
        origin: null,
      },
    },
  },
  { id: "later-view", expected: "unknown", checkId: "later-view" },
  {
    id: "undeclared-dependency",
    expected: "satisfied",
    action: {
      op: "set_context",
      key: "undeclared-setting",
      value: "synthetic-v2",
    },
    limitation:
      "宣言していない依存先の変更は検出できない。成立は宣言された保存条件の範囲だけ",
  },
  { id: "historical-decision", expected: "satisfied", decision: true },
];
const results = [];
for (const spec of specs) {
  const directory = join(output, spec.id);
  mkdirSync(directory);
  const casePath = join(directory, "case");
  command("case", "init", casePath);
  const logs = command(
    "demo",
    "--fault",
    spec.fault || "normal",
    "--output",
    join(directory, "runs"),
  );
  const runPath = /^試行: (.+)$/m.exec(logs)[1];
  // 合成の反例は元の実TCP試行と別のコピーにする。
  let importPath = runPath;
  if (spec.mutate) {
    importPath = join(directory, "synthetic-trace");
    mkdirSync(importPath);
    for (const name of ["manifest.json", "complete.json"])
      write(join(importPath, name), read(join(runPath, name)));
    const events = readFileSync(join(runPath, "events.jsonl"), "utf8")
      .trim()
      .split("\n")
      .map(JSON.parse);
    spec.mutate(events);
    writeFileSync(
      join(importPath, "events.jsonl"),
      events.map((e) => JSON.stringify(e)).join("\n") + "\n",
      { flag: "wx" },
    );
  }
  const checkId = spec.checkId || "save";
  command(
    "case",
    "import",
    casePath,
    importPath,
    "--condition",
    checkId,
    "--actor",
    "Codex合成試験",
    "--reason",
    "比較手順の再現。実参加者ではない",
    "--use-run-context",
    "true",
  );
  const apply = (action) => {
    const request = join(directory, "change.json");
    write(request, {
      expected_revision: JSON.parse(command("case", "show", casePath)).workspace
        .revision,
      actor: "Codex合成試験",
      reason: "反例の再現。実際の条件の変更ではない",
      action,
    });
    command("case", "apply", casePath, request);
  };
  if (spec.action) apply(spec.action);
  if (spec.revise) {
    const c = JSON.parse(command("case", "show", casePath)).workspace.checks[0];
    apply({
      op: "revise_check",
      check_id: c.id,
      text: c.text,
      origin: "合成の条件版更新",
      rule: c.rule,
      dependencies: c.dependencies,
    });
  }
  if (spec.decision)
    apply({
      op: "decide",
      role: "developer",
      action: "hold",
      check_ids: ["save"],
      change_reference: null,
      confidence: null,
      elapsed_ms: null,
    });
  command("case", "export", casePath, "--output", join(directory, "materials"));
  const input = read(join(directory, "materials/input.json"));
  for (const decision of input.decisions)
    assert.ok(
      !("basis" in decision),
      "derived decision basis leaked into common input",
    );
  const view = JSON.parse(command("case", "show", casePath));
  const proposed = view.checks.find((c) => c.check_id === checkId).verdict;
  const reference = checklist(input, checkId);
  assert.equal(proposed, spec.expected, spec.id);
  assert.equal(reference, spec.expected, spec.id);
  results.push({
    id: spec.id,
    input: `${spec.id}/materials/input.json`,
    input_fingerprint: read(join(directory, "materials/export.json"))
      .common_input_fingerprint,
    trace_type: spec.mutate
      ? "synthetic_mutation_of_real_tcp_trace"
      : "real_tcp_with_synthetic_case",
    expected: spec.expected,
    checklist: reference,
    proposed,
    keyword: keyword(input, checkId),
    limitation: spec.limitation || null,
  });
}
write(join(output, "results.json"), {
  schema_version: 1,
  evaluation: "developer_regression_not_human_evaluation",
  reference: "developer_defined_not_independent",
  human_evaluation: "not_conducted",
  results,
});
writeFileSync(
  join(output, "results.md"),
  `# 比較手順の再現\n\n通常のチェックリストと今回の規則に差があるとは主張しない。既知の限定条件で両者の結果を照合した。人の時間・理解・安心感と、外部研究手法は未評価。\n\n| 事例 | 試作 | チェックリスト | 単純なラベル検索 |\n| --- | --- | --- | --- |\n${results.map((r) => `| ${r.id} | ${r.proposed} | ${r.checklist} | ${r.keyword} |`).join("\n")}\n\n未宣言の依存先、虚偽の記録、実環境との対応は別の確認が必要。答え合わせ・保存物は各方法の入力に含めない。\n`,
  { flag: "wx" },
);
console.log(
  JSON.stringify({
    output,
    scenarios: results.length,
    checks: "passed",
    human_evaluation: "not_conducted",
  }),
);
