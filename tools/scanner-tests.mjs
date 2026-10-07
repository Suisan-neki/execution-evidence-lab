import assert from "node:assert/strict";
import { analyze, repositoryAddress, maskCode, validateSnapshot, codeLink, scanPublicRepository } from "../ui/scanner.js";

const files = {
  "Cargo.toml": '[package]\nname = "example-app"\n',
  "src/main.rs": 'use example_app::{storage::{save, load}, network};\n// fn imaginary() {}\nfn main() { save(); }\nfn borrow(x: &\'static str) { }\nfn later() { }',
  "src/storage.rs": 'pub fn save() {}\npub fn load() {}',
  "src/network.rs": 'pub fn send() {}',
  "ui/main.js": '// import x from "./fake.js";\nimport { save } from "./storage.js";\nconst example = "function imaginary() {}";\nfunction begin() { save(); }',
  "ui/storage.js": "export function save() {}",
  "service/main.py": "from .storage import save\n# def imaginary(): pass\ndef main():\n    save()",
  "service/storage.py": "def save():\n    pass",
  "package.json": '{"scripts":{"start":"node ui/main.js"}}',
};
const fixture = { schema: 1, fullName: "example/project", description: "Fixture", reference: "main", commit: "a".repeat(40), inventory: Object.entries(files).map(([path, text]) => ({ path, mode: "100644", size: Buffer.byteLength(text) })), files, omitted: [], truncated: false };
const analysis = analyze(fixture);
assert.deepEqual(analysis.files.find(f => f.path === "src/main.rs").symbols.map(s => s.name), ["main", "borrow", "later"]);
assert.deepEqual(analysis.files.find(f => f.path === "src/main.rs").imports.map(e => e.to), ["src/storage.rs", "src/network.rs"]);
assert.deepEqual(analysis.files.find(f => f.path === "ui/main.js").symbols.map(s => s.name), ["begin"]);
assert.deepEqual(analysis.files.find(f => f.path === "ui/main.js").imports.map(e => e.to), ["ui/storage.js"]);
assert.deepEqual(analysis.files.find(f => f.path === "service/main.py").imports.map(e => e.to), ["service/storage.py"]);
assert.equal(maskCode('"fn fake() {}"\nfn real() {}', true).split("\n").length, 2);
assert.equal(analysis.scripts[0].command, "node ui/main.js");
assert.equal(repositoryAddress("https://github.com/example/project.git").fullName, "example/project");
for (const url of ["http://github.com/a/b", "https://evil.com/a/b", "https://github.com/a/b/tree/main", "https://user:secret@github.com/a/b", "https://github.com/a/b?token=x"]) assert.throws(() => repositoryAddress(url));
assert.equal(codeLink(fixture, "src/main.rs", 3), `https://github.com/example/project/blob/${fixture.commit}/src/main.rs#L3`);
assert.equal(validateSnapshot(fixture), fixture);
assert.throws(() => validateSnapshot({ ...fixture, inventory: [{ path: "../escape", mode: "100644" }] }));
assert.throws(() => validateSnapshot({ ...fixture, files: { "src/main.rs": "x".repeat(131073) } }));
const originalFetch = globalThis.fetch;
const urls = [];
globalThis.fetch = async (url, options) => {
  urls.push(url);
  assert.equal(options.credentials, "omit");
  if (url === "https://api.github.com/repos/example/project") return Response.json({ private: false, default_branch: "main" });
  if (url.endsWith("/commits/feature%2Fread")) return Response.json({ sha: fixture.commit, commit: { tree: { sha: "b".repeat(40) } } });
  if (url.includes("/git/trees/")) return Response.json({ tree: fixture.inventory.map(f => ({ ...f, type: "blob" })), truncated: true });
  if (url.startsWith(`https://raw.githubusercontent.com/example/project/${fixture.commit}/`)) return new Response(files[url.split(fixture.commit + "/")[1]]);
  throw Error("unexpected URL: " + url);
};
try {
  const scanned = await scanPublicRepository("https://github.com/example/project", "feature/read");
  assert.equal(scanned.commit, fixture.commit);
  assert.equal(scanned.truncated, true);
  assert.equal(scanned.files["src/main.rs"], files["src/main.rs"]);
  assert.ok(urls.some(url => url.includes("/git/trees/" + "b".repeat(40))));
  globalThis.fetch = async () => new Response("", { status: 403 });
  await assert.rejects(scanPublicRepository("https://github.com/example/project"), /利用上限/);
} finally { globalThis.fetch = originalFetch; }
console.log("repository reader: parser evidence, lifetimes, excluded comments, pinned ref, public-only URLs, limits and API errors passed");
