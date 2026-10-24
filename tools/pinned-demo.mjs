// コードの改修→再確認待ち→再試験を、合成のGitリポジトリで再現する。
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, join, dirname } from 'node:path';
const root = resolve(import.meta.dirname,'..'), binary = process.env.REVIEW_BINARY || join(root,'target/debug/mugiurushi');
const output = resolve(process.argv[2] || join(root,'evaluations','pinned-'+Date.now()));
// 親だけを用意し、既存の試行ディレクトリは再利用しない。
await mkdir(dirname(output), { recursive: true });
await mkdir(output);const repository=join(output,'repo'), casePath=join(output,'case');await mkdir(repository);
const git=(...args)=>execFileSync('git',['-C',repository,...args],{encoding:'utf8'}).trim();
const run=(...args)=>JSON.parse(execFileSync(binary,args,{encoding:'utf8',stdio:['ignore','pipe','pipe']}));
git('init','-q');git('config','user.name','Synthetic test');git('config','user.email','test@example.invalid');git('remote','add','origin','https://github.com/example/observable-store.git');
await mkdir(join(repository,'examples/observable-store'),{recursive:true});await mkdir(join(repository,'adapters/observation'),{recursive:true});
for(const path of ['adapters/observation/observe.mjs','adapters/observation/observe.py','examples/observable-store/app.py'])await writeFile(join(repository,path),await readFile(join(root,path)));
const appPath='examples/observable-store/app.mjs', fixed=await readFile(join(root,appPath),'utf8');
const broken=fixed.replace("const matches = JSON.stringify(JSON.parse(readFileSync(destination, 'utf8'))) === JSON.stringify(input);", "const matches = false; // 比較条件を誤った合成の旧版");
assert.notEqual(broken,fixed);await writeFile(join(repository,appPath),broken);git('add','.');git('commit','-qm','synthetic before fix');const before=git('rev-parse','HEAD');
const source={repository:'example/observable-store',commit:before,path:appPath,line:1,end_line:1};
const check={id:'stored',version:1,text:'試行用の保存物を読み出し、入力と一致する',origin:'合成の仕様',source,rule:'manual_only',dependencies:[],questions:[]};
const spec={schema_version:1,revision:1,id:'pinned-demo',title:'改修と再試験の合成事例',statements:[],context:{},checks:[check],evidence:[],decisions:[],history:[]};
await writeFile(join(output,'spec.json'),JSON.stringify(spec));let state=run('case','init',casePath,'--file',join(output,'spec.json'));
async function change(action,reason){await writeFile(join(output,'change.json'),JSON.stringify({expected_revision:state.workspace.revision,actor:'合成の確認者',reason,action}));state=run('case','apply',casePath,join(output,'change.json'));return state;}
const criterion={assertion:'stored_matches_input',subject:'record-1'};
await change({op:'configure_observation',check_id:'stored',criterion},'条件に対応する観測を指定する');
const recipe={schema_version:1,check_id:'stored',source,runtime:'node',entrypoint:appPath,args:['normal'],input:{message:'合成の入力'},criterion,timeout_ms:5000};
async function trial(scenario){recipe.args=[scenario];await writeFile(join(output,'recipe.json'),JSON.stringify(recipe));state=run('case','execute',casePath,join(output,'recipe.json'),'--repository-root',repository,'--mode','local','--actor','合成の確認者','--reason',scenario+'の試行');return state;}
const old=await trial('normal');assert.equal(old.checks[0].verdict,'not_satisfied');
await change({op:'decide',role:'developer',action:'modify',check_ids:['stored'],change_reference:'合成の比較条件を修正する',confidence:null,elapsed_ms:null},'保存物はあるが、比較条件を確認する必要がある');
await writeFile(join(repository,appPath),fixed);git('add','.');git('commit','-qm','synthetic after fix');const after=git('rev-parse','HEAD');
recipe.source={...source,commit:after};await change({op:'bind_source',check_id:'stored',source:recipe.source},'比較条件を直したコードの版を参照する');
assert.equal(state.checks[0].verdict,'unknown');assert.equal(state.workspace.decisions[0].basis[0].verdict,'not_satisfied');
const corrected=await trial('normal');assert.equal(corrected.checks[0].verdict,'satisfied');
const verdicts={normal:'satisfied',failure:'not_satisfied',missing:'unknown',conflict:'unknown'};
for(const [scenario,expected] of Object.entries(verdicts))assert.equal((await trial(scenario)).checks[0].verdict,expected);
const first=state.workspace.evidence[0], second=state.workspace.evidence[1];
const comparison=run('case','compare',casePath,'--before',first.id,'--after',second.id,'--condition',check.id);
assert.equal(comparison.before.evidence.observed.verdict,'not_satisfied');assert.equal(comparison.after.evidence.observed.verdict,'satisfied');
run('case','show',casePath);execFileSync(binary,['case','report',casePath,'--output',join(output,'review.json')]);
const result={schema_version:1,case_path:casePath,repository_root:repository,before_commit:before,after_commit:after,verdicts,comparison,human_evaluation:'not_conducted',data:'synthetic'};
await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));
console.log(JSON.stringify({output,checks:['code change','stale evidence','decision basis','retest','four observation outcomes','before-after comparison','review report'],result:'passed',human_evaluation:'not_conducted'}));
