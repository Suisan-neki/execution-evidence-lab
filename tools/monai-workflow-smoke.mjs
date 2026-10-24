import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync,execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';

const root=path.resolve(import.meta.dirname,'..'),bin=process.env.REVIEW_BINARY||path.join(root,'target/debug/mugiurushi');
const original=path.join(root,'examples/monai-workflow/recipe.json'),recipe=JSON.parse(fs.readFileSync(original));
const repository=path.join(root,'evaluations/ci-environments/monai-cpu/repository');
const output=path.join(root,'evaluations/ci-monai-workflow');fs.mkdirSync(output);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const stages=['fixture','input_read','preprocess','train','reference_inference','checkpoint_save','checkpoint_load','inference','result_save','readback'];
const labels=['合成画像とラベルを保存できる','入力を読み出せる','前処理を完了できる','3回の更新でモデルの重みが変わる','保存前のモデルで推論できる','チェックポイントを保存できる','同じ重みを再読込できる','再読込後の推論が保存前の出力と一致する','推論結果を保存できる','別のプロセスで結果を読み出して照合できる'];
function cli(args){
 const r=spawnSync(bin,args,{encoding:'utf8',maxBuffer:32*1024*1024,timeout:300000});
 assert.equal(r.error,undefined,String(r.error));assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);
}
const caseDir=path.join(output,'case'),spec={schema_version:1,revision:1,id:'monai-workflow',title:'合成画像で学習・保存・再読込を試す',statements:[],context:{},checks:stages.map((id,i)=>({id,version:1,text:labels[i],origin:'Codexが選んだ合成データの試験条件',source:recipe.source,rule:'manual_only',dependencies:[],questions:[]})),evidence:[],decisions:[],history:[]};
fs.writeFileSync(path.join(output,'case-spec.json'),JSON.stringify(spec,null,2));
let view=cli(['case','init',caseDir,'--file',path.join(output,'case-spec.json')]);
function change(action,reason){
 const file=path.join(output,'change.json');fs.writeFileSync(file,JSON.stringify({expected_revision:view.workspace.revision,actor:'合成の試験担当',reason,action}));view=cli(['case','apply',caseDir,file]);
}
for(const id of stages)change({op:'configure_reported_observation',check_id:id,criterion:{assertion:'monai.stage',subject:id}},'段階ごとの報告を条件へ対応づける');
let frameworkCommit=execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim();
if(process.env.GITHUB_EVENT_PATH){const event=JSON.parse(fs.readFileSync(process.env.GITHUB_EVENT_PATH));frameworkCommit=event.pull_request?.head?.sha||process.env.GITHUB_SHA||frameworkCommit;}
const workflowSource={repository:'Suisan-neki/mugiurushi',commit:frameworkCommit,path:'examples/monai-workflow/workflow.py',line:1,end_line:fs.readFileSync(path.join(root,'examples/monai-workflow/workflow.py'),'utf8').trimEnd().split('\n').length};
const results=[];
function run(label,fault){
 const folder=path.join(output,label);fs.mkdirSync(folder);
 const configured=structuredClone(recipe);configured.env.EEL_MONAI_FAULT=fault;
 const file=path.join(folder,'recipe.json');fs.writeFileSync(file,JSON.stringify(configured,null,2));
 for(const s of configured.support)fs.copyFileSync(path.join(path.dirname(original),s.path),path.join(folder,s.path));
 const executed=spawnSync(bin,['environment','run',file,'--repository-root',repository,'--output',path.join(folder,'runs')],{encoding:'utf8',maxBuffer:32*1024*1024,timeout:300000});
 fs.writeFileSync(path.join(folder,'cli.stdout.log'),executed.stdout||'');fs.writeFileSync(path.join(folder,'cli.stderr.log'),executed.stderr||'');
 assert.equal(executed.error,undefined,String(executed.error));
 const runs=fs.readdirSync(path.join(folder,'runs'));assert.equal(runs.length,1);
 const directory=path.join(folder,'runs',runs[0]),recordBytes=fs.readFileSync(path.join(directory,'environment.json')),record=JSON.parse(recordBytes);
 const progressFile=path.join(directory,'result/workflow.json');
 if(!fs.existsSync(progressFile)){
  console.error(record.report.detail,record.diagnostics);
  for(const log of ['execution.stdout.log','execution.stderr.log'])if(fs.existsSync(path.join(directory,log)))console.error(fs.readFileSync(path.join(directory,log),'utf8').slice(-10000));
 }
 assert.equal(record.report.preparation,'succeeded');assert.equal(record.container_removed,true);assert.deepEqual(record.diagnostics,[]);
 const workflowBytes=fs.readFileSync(progressFile),workflow=JSON.parse(workflowBytes);
 console.log(label,JSON.stringify({exit:executed.status,target:record.report.target,workflow_completed:workflow.completed,stages:workflow.stages.map(s=>[s.id,s.status]),metrics:workflow.metrics}));
 assert.equal(executed.status,fault==='normal'?0:1);assert.equal(record.report.target,fault==='normal'?'succeeded':'failed');
 assert.deepEqual(workflow.stages.map(s=>s.id),stages);
 const status=id=>workflow.stages.find(s=>s.id===id).status;
 if(fault==='normal'){
  assert(workflow.stages.every(s=>s.status==='succeeded'));assert.equal(workflow.completed,true);
  assert.equal(workflow.metrics.completed_updates,3);assert.equal(workflow.metrics.losses.length,3);assert(workflow.metrics.losses.every(Number.isFinite));
  assert.notEqual(workflow.metrics.initial_parameters_sha256,workflow.metrics.trained_parameters_sha256);
  assert.equal(workflow.metrics.trained_model_sha256,workflow.metrics.restored_model_sha256);
  assert(workflow.metrics.max_abs_error<=1e-6);assert(workflow.metrics.readback_max_abs_error<=1e-6);
 }else if(fault==='input_missing'){
  assert.equal(status('input_read'),'failed');assert.equal(status('train'),'not_observed');assert(!fs.existsSync(path.join(directory,'result/input.npy')));
 }else if(fault==='save_is_directory'){
  assert.equal(status('checkpoint_save'),'failed');assert.equal(status('checkpoint_load'),'not_observed');assert(fs.statSync(path.join(directory,'result/checkpoint.pt')).isDirectory());
 }else if(fault==='checkpoint_corrupt'){
  assert.equal(status('checkpoint_load'),'failed');assert.equal(status('inference'),'not_observed');assert.notEqual(hash(fs.readFileSync(path.join(directory,'result/checkpoint.pt'))),workflow.metrics.saved_checkpoint_sha256);
 }else{
  assert.equal(status('train'),'running');assert.equal(status('checkpoint_save'),'not_observed');assert.equal(workflow.completed,false);assert.equal(workflow.metrics.completed_updates,1);
  assert.equal(record.report.steps.at(-1).exit_code,-9);
 }
 if(fault!=='normal')assert(!fs.existsSync(path.join(directory,'result/prediction.npy')));
 // Verify retained artifact bytes outside the target process; numerical readback is a separate container process.
 for(const artifact of workflow.artifacts){
  assert(!artifact.path.split('/').includes('..')&&!path.isAbsolute(artifact.path));
  const bytes=fs.readFileSync(path.join(directory,'result',artifact.path));assert.equal(bytes.length,artifact.bytes);assert.equal(hash(bytes),artifact.sha256);
 }
 const correlation={run_id:runs[0],record_id:'synthetic-monai-input-144',attempt_id:runs[0]+'-workflow'};
 const observations=workflow.stages.filter(s=>['succeeded','failed'].includes(s.status)).map((s,i)=>({sequence:i+1,correlation,assertion:'monai.stage',subject:s.id,outcome:s.status==='succeeded'?'pass':'fail',detail:JSON.stringify({stage:s,workflow_run:runs[0],workflow_sha256:hash(workflowBytes),environment_sha256:hash(recordBytes),workflow_completed:workflow.completed}),source:workflowSource}));
 // completed describes this recovered report, not the target workflow; unfinished stages have no observation.
 const trace={schema_version:1,correlation,producer:'MONAI workflow report adapter',origin:'固定したコンテナ内の段階報告を回収。独立した実行経路の証明ではない。',completed:true,source:recipe.source,
  context:{image:record.recipe.image,images:{workflow:record.recipe.image},browser_version:null,platform:record.recipe.platform,source:recipe.source,support:record.recipe.support,runner_sha256:record.runner_sha256,packages:record.report.packages,model:workflow.model,seed:workflow.seed,updates:workflow.updates,atol:workflow.atol,rtol:workflow.rtol,fault,synthetic_input_sha256:workflow.metrics.input_sha256},observations};
 fs.writeFileSync(path.join(folder,'reported-trace.json'),JSON.stringify(trace,null,2));
 for(const id of stages){
  const condition=view.workspace.checks.find(c=>c.id===id);
  change({op:'import_reported',condition:{id,version:condition.version,text:condition.text,origin:condition.origin,kind:'custom'},trace},label+'の段階報告を取り込む');
 }
 for(const check of view.checks){
  const state=status(check.check_id),expected=state==='succeeded'?'satisfied':state==='failed'?'not_satisfied':'unknown';
  assert.equal(check.verdict,expected,label+' '+check.check_id);
 }
 const entry={label,fault,directory,record,workflow,evidence_ids:view.workspace.evidence.filter(e=>e.manifest.correlation.run_id===runs[0]).map(e=>({id:e.id,check_id:e.check_id})),verdicts:view.checks.map(c=>({id:c.check_id,verdict:c.verdict}))};
 fs.writeFileSync(path.join(folder,'summary.json'),JSON.stringify({label,fault,run_id:runs[0],verdicts:entry.verdicts,workflow},null,2));return entry;
}
for(const [label,fault] of [['normal','normal'],['input-missing','input_missing'],['checkpoint-corrupt','checkpoint_corrupt'],['save-is-directory','save_is_directory'],['interrupted','interrupted'],['retry','normal']])results.push(run(label,fault));
const normal=results[0],retry=results.at(-1);
for(const key of ['input_sha256','labels_sha256','preprocessed_sha256','trained_model_sha256','trained_parameters_sha256','prediction_sha256'])assert.equal(normal.workflow.metrics[key],retry.workflow.metrics[key],key);
const deps=cli(['environment','compare',normal.directory,retry.directory]);assert.equal(deps.packages_comparable,true);assert.equal(deps.recipe_changed,false);assert.deepEqual(deps.added_or_changed,[]);assert.deepEqual(deps.removed_or_changed,[]);
fs.writeFileSync(path.join(output,'dependency-comparison.json'),JSON.stringify(deps,null,2));
const before=normal.evidence_ids.find(e=>e.check_id==='train').id,after=retry.evidence_ids.find(e=>e.check_id==='train').id;
const comparison=cli(['case','compare',caseDir,'--before',before,'--after',after,'--condition','train']);
fs.writeFileSync(path.join(output,'case-comparison.json'),JSON.stringify(comparison,null,2));
fs.writeFileSync(path.join(output,'summary.json'),JSON.stringify({source:recipe.source,workflow_source:workflowSource,results:results.map(r=>({label:r.label,fault:r.fault,verdicts:r.verdicts})),scope:'Synthetic CPU workflow and reported observations; no clinical or participant evaluation.'},null,2));
console.log('MONAI training, checkpoint/readback, four real faults, partial-stage reports and deterministic retry passed.');
