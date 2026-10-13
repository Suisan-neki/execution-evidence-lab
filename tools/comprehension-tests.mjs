import assert from 'node:assert/strict';
import { analyze, maskCode } from '../ui/scanner.js';
import { explainFunction, compareSnapshots, lineChanges, referenceFromURL, sourceURL, validateCodeReference, cachedSnapshot, cacheSnapshot } from '../ui/comprehension.js';
const make = (files, commit = 'a', truncated = false) => ({ schema: 1, fullName: 'example/project', reference: 'main', commit: commit.repeat(40), truncated, inventory: Object.entries(files).map(([path, text]) => ({ path, size: Buffer.byteLength(text), mode: '100644' })), files, omitted: [] });
const before = make({
  'src/main.rs': `use crate::storage;\nfn main(input: &str) -> Result<(), Error> {\n  if input.is_empty() { return Err(Error); }\n  storage::save(input)?;\n  Ok(())\n}\nfn unrelated() { unsafe_call(); }`,
  'src/storage.rs': `pub fn save(data: &str) {\n  let fake = r#"fn bad() { dangerous(); }"#;\n  /* outer /* nested() */ ignored(); */\n  write(data);\n}`,
  'app.py': 'def first(data):\n    """\n    def fake():\n        dangerous()\n    """\n    if data:\n        return send(data)\n\ndef later():\n    unrelated()'
});
const model = analyze(before), guide = explainFunction(model, 'src/main.rs', 2);
assert.equal(guide.name, 'main'); assert.equal(guide.range.end, 6);
assert.ok(guide.signature.includes('input: &str'));
assert.ok(guide.steps.some(s => s.kind === 'branch'));
assert.ok(guide.steps.some(s => s.kind === 'propagate'));
const save = guide.calls.find(c => c.name === 'storage::save');
assert.deepEqual(save.candidates, [{ path: 'src/storage.rs', line: 1, name: 'save' }]);
assert.ok(!guide.calls.some(c => c.name === 'unsafe_call'));
const storage = explainFunction(model, 'src/storage.rs', 1);
assert.deepEqual(storage.calls.map(c => c.name), ['write']);
const py = explainFunction(model, 'app.py', 1);
assert.deepEqual(py.calls.map(c => c.name), ['send']);
assert.equal(explainFunction(model, 'src/main.rs', 1), null);
assert.ok(!maskCode('const r = /function fake() { evil(); }/;').includes('fake'));
const js = make({ 'app.js': 'function run({data}) {\n  return save(data);\n}\nfunction other() { wrong(); }' });
assert.equal(explainFunction(analyze(js), 'app.js', 1).range.end, 3);
const after = make({ 'src/main.rs': before.files['src/main.rs'].replace('storage::save(input)?;', 'storage::save(input).await?;'), 'new.js': 'function main() {}' }, 'b');
const diff = compareSnapshots(before, after);
assert.equal(diff.entries.find(e => e.path === 'src/main.rs').status, 'changed');
assert.equal(diff.entries.find(e => e.path === 'src/storage.rs').status, 'removed');
assert.equal(diff.entries.find(e => e.path === 'new.js').status, 'added');
assert.equal(compareSnapshots(before, { ...after, truncated: true }).entries.find(e => e.path === 'app.py').status, 'unknown');
const unread = { ...before, files: {} };
assert.ok(compareSnapshots(unread, unread).entries.every(e => e.status === 'unknown'));
const hashed = { ...unread, inventory: unread.inventory.map(f => ({ ...f, sha: 'c'.repeat(40) })) };
assert.ok(compareSnapshots(hashed, hashed).entries.every(e => e.status === 'unchanged'));
assert.throws(() => compareSnapshots(before, { ...after, fullName: 'other/project' }));
assert.equal(lineChanges('same\nold\nend', 'same\nnew\nend').before[0].line, 2);
assert.equal(lineChanges('same\nold\nend', 'same\nnew\nend').after[0].text, 'new');
assert.equal(lineChanges(null, 'x'), null);
const source = referenceFromURL(`https://github.com/example/project/blob/${before.commit}/src/main.rs#L2-L6`);
assert.equal(source.line, 2); assert.equal(source.end_line, 6);
assert.equal(sourceURL(source), `https://github.com/example/project/blob/${before.commit}/src/main.rs#L2`);
for (const path of ['../x', '/x', 'a//b', 'a\\b', 'a\0b']) assert.throws(() => validateCodeReference({ ...source, path }));
assert.throws(() => referenceFromURL('https://github.com/example/project/blob/main/x#L1'));
const storageMap = new Map(); const session = { setItem: (k,v) => storageMap.set(k,v), getItem: k => storageMap.get(k) };
assert.ok(cacheSnapshot(before, session)); assert.equal(cachedSnapshot(source, session).commit, before.commit);
assert.equal(cachedSnapshot({ ...source, commit: after.commit }, session), null);
assert.equal(cacheSnapshot(before, { setItem() { throw Error('quota'); } }), false);
console.log('code comprehension: function boundaries, misleading literals, candidate calls, version changes, incomplete trees, source links and cache checks passed');
