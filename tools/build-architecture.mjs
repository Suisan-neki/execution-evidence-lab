// OpenEMRの一つの構成モデルから全体・内部の図を生成する。実行時にNode.jsは使わない。
import { readFile, writeFile, mkdtemp, mkdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { execFileSync } from "node:child_process";
import { gzipSync } from "node:zlib";
const root = resolve(import.meta.dirname, ".."),
  out = join(root, "ui/architecture-generated");
const m = JSON.parse(
  await readFile(
    join(root, "examples/openemr-browser/architecture.json"),
    "utf8",
  ),
);
const temp = await mkdtemp(join(tmpdir(), "eel-likec4-"));
const q = JSON.stringify,
  byId = new Map(m.elements.map((e) => [e.id, e]));
function fqn(id) {
  const e = byId.get(id);
  return e.parent ? `${fqn(e.parent)}.${id}` : id;
}
function element(e, indent = "  ") {
  return `${indent}${e.id} = ${e.kind} ${q(e.title)} {\n${indent}  description ${q(e.description)}\n${indent}  style { size xs\n${indent}    textSize lg }\n${m.elements
    .filter((c) => c.parent === e.id)
    .map((c) => element(c, indent + "  "))
    .join("")} ${indent}}\n`;
}
const source = `specification {\n  element person { style { shape person } }\n  element system\n  element app\n  element database { style { shape storage } }\n  element storage { style { shape storage } }\n  element boundary\n}\nmodel {\n${m.elements
  .filter((e) => !e.parent)
  .map((e) => element(e))
  .join(
    "",
  )}${m.relations.map((r) => `  ${fqn(r.from)} -> ${fqn(r.to)} ${q(r.title)} {\n    description ${q(r.description)}\n    metadata { eelId ${q(r.id)} }\n  }`).join("\n")}\n}\nviews {\n view context {\n  title "全体の構成"\n  include operator, browser, system\n  autoLayout TopBottom\n }\n view containers {\n  title "内部の構成"\n  include operator, browser, system.*\n  autoLayout LeftRight\n }\n}\n`;
try {
  await mkdir(out, { recursive: true });
  await writeFile(join(temp, "model.c4"), source);
  const cli = join(root, "node_modules/likec4/bin/likec4.mjs");
  execFileSync(process.execPath, [cli, "validate", temp], { stdio: "inherit" });
  execFileSync(
    process.execPath,
    [cli, "export", "json", temp, "-o", join(temp, "model.json")],
    { stdio: "inherit" },
  );
  execFileSync(
    process.execPath,
    [cli, "codegen", "webcomponent", temp, "-o", join(temp, "bundle.js")],
    { stdio: "inherit" },
  );
  const layout = JSON.parse(await readFile(join(temp, "model.json"), "utf8"));
  for (const view of ["context", "containers"])
    if (!layout.views[view]) throw Error("Missing view " + view);
  if (!layout.views.containers.nodes.some((n) => n.id === "system.db"))
    throw Error("内部のDBを展開できていません");
  const nodes = Object.fromEntries(m.elements.map((e) => [fqn(e.id), e.id]));
  const relations = Object.fromEntries(
    Object.entries(layout.relations).map(([id, r]) => [
      id,
      Array.isArray(r.metadata.eelId) ? r.metadata.eelId[0] : r.metadata.eelId,
    ]),
  );
  const edges = Object.fromEntries(
    Object.values(layout.views).flatMap((v) =>
      v.edges.map((e) => [
        e.id,
        e.relations.map((r) => relations[r]).filter(Boolean),
      ]),
    ),
  );
  await writeFile(join(out, "model.c4"), source);
  await writeFile(join(out, "ids.json"), JSON.stringify({ nodes, edges }));
  const original = await readFile(join(temp, "bundle.js"), "utf8");
  // 固定版の生成物に含まれる外部Webフォントを除く。図にも端末の書体を使う。
  const fonts = original.match(/@font-face\{[^}]*\}/g) || [];
  if (fonts.length !== 6)
    throw Error("LikeC4のフォント定義を再確認してください");
  const bundle = Buffer.from(original.replace(/@font-face\{[^}]*\}/g, ""));
  await writeFile(
    join(out, "likec4.js.gz"),
    gzipSync(bundle, { level: 9, mtime: 0 }),
  );
  for (const [pkg, name] of [
    ["likec4", "LikeC4"],
    ["react", "React"],
    ["react-dom", "ReactDOM"],
    ["@likec4/core", "LikeC4Core"],
  ])
    await writeFile(
      join(out, "LICENSE-" + name),
      await readFile(join(root, "node_modules", pkg, "LICENSE")),
    );
  const published = await readFile(
    join(root, "node_modules/likec4/react/index.mjs"),
    "utf8",
  );
  const notices = [
    ...new Set(
      (published.match(/\/\*+[\s\S]*?\*\//g) || []).filter((c) =>
        /@license|copyright/i.test(c),
      ),
    ),
  ];
  await writeFile(
    join(out, "THIRD-PARTY-NOTICES.txt"),
    "License notices retained from likec4@1.60.0/react/index.mjs.\n\n" +
      notices.join("\n\n") +
      "\n",
  );
  console.log(
    JSON.stringify({
      likec4: "1.60.0",
      views: ["context", "containers"],
      bytes: bundle.length,
      gzip_bytes: gzipSync(bundle, { level: 9 }).length,
    }),
  );
} finally {
  await rm(temp, { recursive: true, force: true });
}
