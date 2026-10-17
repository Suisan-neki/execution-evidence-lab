// サーバーのPATHからNode.jsを外し、読取・解析APIを確認する。架空のコードは実行しない。
import assert from 'node:assert/strict';
import {execFileSync,spawn} from 'node:child_process';
import {mkdtemp,mkdir,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const binary=process.env.REVIEW_BINARY||join(root,'target/debug/execution-evidence-lab');
const temp=await mkdtemp(join(tmpdir(),'eel-native-reader-'));
const emptyPath=join(temp,'empty-path');await mkdir(emptyPath);
const caseDir=join(temp,'case'),tokenPath=join(temp,'api-token');
const env={...process.env,PATH:emptyPath};
execFileSync(binary,['case','init',caseDir,'--template','general','--id','native-reader','--title','架空の読取試験','--actor','架空の確認者','--reason','Rustへの移植確認'],{env,stdio:'pipe'});
const server=spawn(binary,['api',caseDir,'--port','0','--token-file',tokenPath],{env,stdio:['ignore','pipe','pipe']});
let stderr='';server.stderr.on('data',d=>{stderr+=d;});
try {
 const url=await new Promise((accept,reject)=>{let text='';const timer=setTimeout(()=>reject(Error('native server startup timeout: '+stderr)),10000);server.stdout.on('data',d=>{text+=d;const m=/http:\/\/127\.0\.0\.1:\d+/.exec(text);if(m){clearTimeout(timer);accept(m[0]);}});server.once('exit',code=>{clearTimeout(timer);reject(Error('server exited '+code+': '+stderr));});});
 const token=await readFile(tokenPath,'utf8');
 async function request(body,expected=200){const r=await fetch(url+'/api/v1/repository',{method:'POST',headers:{'Content-Type':'application/json','X-Review-Token':token},body:JSON.stringify(body)});const data=await r.json();assert.equal(r.status,expected,JSON.stringify(data));return data;}
 const {snapshot}=JSON.parse(await readFile(join(root,'tests/fixtures/reader/navigation.json'),'utf8'));
 const analysis=await request({operation:'analyze',snapshot,path:'ui/app.ts',line:8});
 assert.equal(analysis.guide.calls.find(c=>c.name==='persist').candidates[0].path,'ui/store.js');
 for(const operation of ['overview','plan']){const r=await request({operation,snapshot});assert.equal(r.schema_version,1);assert.equal(r.result.commit,snapshot.commit);}
 const detail=await request({operation:'function',snapshot,path:'ui/app.ts',line:8});assert.equal(detail.result.found,true);
 const inquiry=await request({operation:'inquire',snapshot,question:'persist の入力は？',path:'ui/app.ts',line:8});assert.ok(inquiry.result.functions.length);assert.deepEqual(inquiry.result.failed_files,[]);
 await request({operation:'read',url:'https://example.invalid/a/b',reference:'main'},400);
 await request({operation:'analyze',snapshot:{...snapshot,files:{'ui/app.ts':'x'.repeat(128*1024+1)}},path:null,line:null},400);
 let live=null;
 if(process.argv.includes('--live')){
   const read=await request({operation:'read',url:'https://github.com/sjl/t',reference:'70817fd'});
   assert.match(read.snapshot.commit,/^70817fd/);assert.ok(Object.keys(read.snapshot.files).length>0);
   const path=Object.keys(read.snapshot.files).find(p=>p.endsWith('.py'));assert.ok(path);
   const partial=structuredClone(read.snapshot);delete partial.files[path];
   const file=await request({operation:'file',snapshot:partial,path});assert.equal(typeof file.snapshot.files[path],'string');
   const open=await request({operation:'open',url:'https://github.com/sjl/t',reference:read.snapshot.commit});
   assert.equal(open.result.commit,read.snapshot.commit);assert.ok(open.result.components.length>0);
   live={commit:read.snapshot.commit,inventory:read.snapshot.inventory.length,files:Object.keys(read.snapshot.files).length};
 }
 // 解析対象に含む宣言は起動されず、Node.js用の一時スクリプトも作られない。
 assert.equal(stderr,'');
 console.log(JSON.stringify({checks:['API without Node.js in server PATH','analyze','overview','plan','function','inquire','non-GitHub URL rejected','oversized text rejected'],live}));
} finally {server.kill('SIGTERM');await new Promise(resolve=>server.exitCode!==null||server.signalCode!==null?resolve():server.once('exit',resolve));await rm(temp,{recursive:true,force:true});}
