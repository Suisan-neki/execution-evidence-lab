import assert from 'node:assert/strict';
import { analyze } from '../ui/scanner.js';
import { explainFunction, callersOf } from '../ui/comprehension.js';
const snapshot=(files,unread=[])=>({schema:1,fullName:'example/project',reference:'main',commit:'a'.repeat(40),truncated:false,files,omitted:unread.map(path=>({path,reason:'未読'})),inventory:[...Object.entries(files).map(([path,text])=>({path,mode:'100644',size:Buffer.byteLength(text)})),...unread.map(path=>({path,mode:'100644',size:100}))]});
const files={
  'Cargo.toml':'[package]\nname="example-app"',
  'src/main.rs':'use example_app::{storage::{save as persist, self as disk}, other};\nfn main(input: &str) {\n  persist(input);\n  disk::save(input);\n  other::save(input);\n}\nfn shadow(persist: fn()) { persist(); }',
  'src/storage.rs':'pub fn save(input: &str) {}',
  'src/other.rs':'pub fn save(input: &str) {}',
  'ui/app.ts':'import {\n save as persist,\n type Value\n} from "./store.js";\nimport * as disk from "./store.js";\nimport decode from "./codec.js";\nimport { save } from "external-package";\nfunction run(input: string) {\n persist(input);\n disk.save(input);\n decode(input);\n save(input);\n unknown.save(input);\n}\nfunction shadow(persist) { persist(); }\nfunction localShadow() { const persist = input => input; persist(1); }\nfunction destructured({persist}) { persist(1); }',
  'ui/store.js':'export function save(data) { return data; }',
  'ui/other.js':'export function persist(data) { return data; }',
  'ui/codec.js':'export default function parse(data) { return data; }',
  'service/app.py':'from .storage import (\n save as persist,\n)\nfrom . import storage as disk\ndef run(data):\n    persist(data)\n    disk.save(data)\ndef shadow(persist):\n    persist(1)\ndef later():\n    persist(1)',
  'service/storage.py':'def save(data):\n    return data',
};
let model=analyze(snapshot(files));
const rust=explainFunction(model,'src/main.rs',2);
assert.deepEqual(rust.calls.find(c=>c.name==='persist').candidates,[{path:'src/storage.rs',line:1,name:'save'}]);
assert.deepEqual(rust.calls.find(c=>c.name==='disk::save').candidates,[{path:'src/storage.rs',line:1,name:'save'}]);
assert.deepEqual(rust.calls.find(c=>c.name==='other::save').candidates,[{path:'src/other.rs',line:1,name:'save'}]);
assert.equal(explainFunction(model,'src/main.rs',7).calls[0].candidates.length,0);
const js=explainFunction(model,'ui/app.ts',8);
assert.deepEqual(js.calls.find(c=>c.name==='persist').candidates,[{path:'ui/store.js',line:1,name:'save'}]);
assert.equal(js.calls.find(c=>c.name==='persist').bindings[0].line,1);
assert.ok(js.calls.find(c=>c.name==='persist').bindings[0].declaration.includes('save as persist'));
assert.deepEqual(js.calls.find(c=>c.name==='disk.save').candidates,[{path:'ui/store.js',line:1,name:'save'}]);
assert.deepEqual(js.calls.find(c=>c.name==='decode').candidates,[{path:'ui/codec.js',line:1,name:'parse'}]);
assert.equal(js.calls.find(c=>c.name==='save').candidates.length,0);
assert.equal(js.calls.find(c=>c.name==='unknown.save').candidates.length,0);
for(const line of [15,16,17]) assert.equal(explainFunction(model,'ui/app.ts',line).calls.find(c=>c.name==='persist').candidates.length,0);
const callers=callersOf(model,'ui/store.js',1);
assert.deepEqual(callers.callers.map(c=>[c.path,c.line,c.call]),[['ui/app.ts',9,'persist'],['ui/app.ts',10,'disk.save']]);
assert.ok(callers.callers[0].bindings[0].declaration.includes('save as persist'));
const py=explainFunction(model,'service/app.py',5);
assert.equal(py.calls.find(c=>c.name==='persist').candidates[0].path,'service/storage.py');
assert.equal(py.calls.find(c=>c.name==='disk.save').candidates[0].path,'service/storage.py');
assert.equal(explainFunction(model,'service/app.py',8).calls.find(c=>c.name==='persist').candidates.length,0);
assert.equal(callersOf(model,'service/storage.py',1).callers.length,3);
// 宣言を読む前は候補の関数を作らず、読み込むと同じ対応を更新する。
const partial={...files};delete partial['ui/store.js'];
model=analyze(snapshot(partial,['ui/store.js']));
assert.deepEqual(explainFunction(model,'ui/app.ts',8).calls.find(c=>c.name==='persist').unread,['ui/store.js']);
assert.equal(explainFunction(model,'ui/app.ts',8).calls.find(c=>c.name==='persist').candidates.length,0);
assert.equal(callersOf(model,'ui/codec.js',1).unread_files,1);
// 文字列・コメントの偽の宣言、type import、別関数内のimportを使わない。
const deceptive=analyze(snapshot({
 'app.js':'const text = "import { save as persist } from \\\"./store.js\\\"";\n// import { save as persist } from "./store.js";\nimport type { save as persist } from "./store.js";\nfunction run() { persist(); }',
 'store.js':'export function save() {}',
 'app.py':'def first():\n    from .store import save as persist\n    persist()\ndef later():\n    persist()',
 'store.py':'def save():\n    pass',
}));
assert.equal(explainFunction(deceptive,'app.js',4).calls[0].candidates.length,0);
assert.equal(explainFunction(deceptive,'app.py',1).calls[0].candidates[0].path,'store.py');
assert.equal(explainFunction(deceptive,'app.py',4).calls[0].candidates.length,0);
// 入れ子の関数の呼び出しを外側の関数の呼び出しへ重複させない。
const nested=analyze(snapshot({'app.js':'function outer() {\n  function inner() {\n    save();\n  }\n  inner();\n}\nfunction save() {}'}));
assert.ok(!explainFunction(nested,'app.js',1).calls.some(c=>c.name==='save'));
assert.deepEqual(callersOf(nested,'app.js',7).callers.map(c=>c.function),['inner']);
const limit=callersOf(analyze(snapshot(files)),'ui/store.js',1,1,1);
assert.equal(limit.limited,true);assert.equal(limit.scanned_functions,1);


const qualified=analyze(snapshot({'src/main.rs':'fn main() { crate::storage::save(); }','src/storage.rs':'pub fn save() {}'}));
assert.equal(explainFunction(qualified,'src/main.rs',1).calls[0].candidates[0].path,'src/storage.rs');
const qualifiedUnread=analyze(snapshot({'src/main.rs':'fn main() { crate::storage::save(); }'},['src/storage.rs']));
assert.deepEqual(explainFunction(qualifiedUnread,'src/main.rs',1).calls[0].unread,['src/storage.rs']);

console.log('call navigation: aliases, multiline imports, namespaces, default exports, scope collisions, unread targets, reverse callers and limits passed');
