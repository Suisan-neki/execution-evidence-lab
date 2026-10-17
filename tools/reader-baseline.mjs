// 移植前のブラウザ用解析で、架空データの期待結果を保存する。対象コードは実行しない。
// 通常の試験は保存済みJSONをRustで読み、Node.jsなしで比較する。
import fs from 'node:fs';
import vm from 'node:vm';
import {analyze} from '../ui/scanner.js';
import {explainFunction,callersOf} from '../ui/comprehension.js';
import {overview} from '../ui/overview.js';
import {functionDetail,inquire} from '../ui/inquiry.js';
import {plan} from '../ui/trial-plan.js';
function extract(file,start,end,expr){const s=fs.readFileSync(new URL(file,import.meta.url),'utf8');return vm.runInNewContext(s.slice(s.indexOf(start),s.indexOf(end))+'\nJSON.stringify('+expr+')',{Buffer});}
const scanner=JSON.parse(extract('scanner-tests.mjs','const files =','const analysis =','fixture'));
const navigation=JSON.parse(extract('call-navigation-tests.mjs','const snapshot=','let model=','snapshot(files)'));
const notes=JSON.parse(extract('understanding-tests.mjs','const COMMIT =','const model =','snap'));
const partial=structuredClone(navigation);delete partial.files['ui/store.js'];partial.omitted.push({path:'ui/store.js',reason:'初回の読取範囲外'});
function snapshot(files,extra=[]){return {schema:1,fullName:'example/project',description:'架空のアプリ',reference:'main',commit:'a'.repeat(40),truncated:false,files,inventory:[...Object.entries(files).map(([path,t])=>({path,mode:'100644',size:Buffer.byteLength(t)})),...extra.map(path=>({path,mode:'100644',size:10}))],omitted:extra.map(path=>({path,reason:'初回の読取範囲外'}))};}
const unicode=snapshot({'src/main.rs':'// 日本語🐟\nfn main() {\n let text = "🦀 fn fake() {}";\n write();\n}\nfn write() { println!("こんにちは"); }\n', 'src/raw.rs':'/* 外 /* 内 */ 外 */\nfn borrow(x: &\'static str) {\n let s = r###"fn fake() {}"###;\n return;\n}\n','other.js':'const fake = /function fake() {}/;\nfunction real() { console.log("日本語"); }\n'});
const config=snapshot({'package.json':JSON.stringify({name:'fake',main:'./index.js',bin:{fake:'index.js'},scripts:{test:'node tests/basic.js',start:'node index.js'},dependencies:{fake:'1'},devDependencies:{jest:'1'}},null,2),'index.js':'import * as storage from "./store.js";\nfunction main() {\n const p = process.argv[2];\n storage.save(p);\n}\n','store.js':'export function save(p) {\n writeFileSync(p, "test");\n}\n','Makefile':'test:\n\tnode tests/basic.js\n','.github/workflows/test.yml':'steps:\n  - run: npm test\n','tests/basic.js':'// save is tested here\n','pyproject.toml':'[project]\ndependencies = ["pydicom >=3", "cram"]\n[project.scripts]\nrun = "app.main:main"\n','app/__init__.py':'','app/__main__.py':'from .main import main\nmain()\n','app/main.py':'def main():\n    print("hello")\n','setup.py':'console_scripts=["cli=app.main:main"]\n','README.md':'# App\n\nThis is a fictional application used only for reader tests.\n\n```sh\nnpm start\n```\n'});
const nested=snapshot({'app.js':'function outer() {\n  function inner() {\n    save();\n  }\n  inner();\n}\nfunction save() {}','app.py':'def first():\n    from .store import save as persist\n    persist()\ndef later():\n    persist()','store.py':'def save():\n    pass'});
const unsupported=snapshot({'README.md':'# Example\n\n架空のGoアプリケーションについて説明するための文章です。\n'},['cmd/run.go']);unsupported.truncated=true;
for(const [name,s] of Object.entries({scanner,navigation,partial,notes,unicode,config,nested,unsupported})){
 const m=analyze(s);const functions=m.files.flatMap(f=>f.symbols.map(sym=>({path:f.path,line:sym.line,guide:explainFunction(m,f.path,sym.line),callers:callersOf(m,f.path,sym.line),detail:functionDetail(m,f.path,sym.line)})));
 const questions=['保存できなかったらどうなる？','入力と設定を調べたい','送信と読み出しのテスト','main はどこ？','起動方法','？？？'];
 const value={snapshot:s,model:m,overview:overview(s,m),plan:plan(s,m),functions,questions:questions.map(question=>({question,result:inquire(s,m,question)}))};
 fs.writeFileSync(new URL('../tests/fixtures/reader/'+name+'.json',import.meta.url),JSON.stringify(value,null,2)+'\n');
}
console.log('reader baseline: 8 synthetic snapshots saved');
