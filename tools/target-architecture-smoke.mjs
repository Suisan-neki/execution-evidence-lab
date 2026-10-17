// Synthetic UI input and retained real target evidence; no participant evaluation.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {chromium} from 'playwright';
const root=path.resolve(import.meta.dirname,'..'),binary=process.env.REVIEW_BINARY||path.join(root,'target/debug/execution-evidence-lab');
const target=process.argv[2]||'presets';assert(['presets','ohif','monai'].includes(target));
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'eel-target-diagram-'));
const casePath=target==='presets'?path.join(temp,'case'):process.env.EEL_TARGET_ARCHITECTURE_CASE||path.join(root,target==='ohif'?'evaluations/ci-ohif/case':'evaluations/ci-monai-workflow/case');
function cli(args){const r=spawnSync(binary,args,{encoding:'utf8',maxBuffer:64*1024*1024});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);}
let state;
function change(action){const file=path.join(temp,'change.json');fs.writeFileSync(file,JSON.stringify({expected_revision:state.workspace.revision,actor:'合成の画面試験',reason:'実行済みの観測と構成図の対応を確認する',action}));state=cli(['case','apply',casePath,file]);}
let model;
if(target==='presets')state=cli(['case','init',casePath,'--template','general','--id','preset-ui','--title','合成の対象切替試験','--actor','合成','--reason','図の保存を確認']);
else{
 state=cli(['case','show',casePath]);model=JSON.parse(fs.readFileSync(path.join(root,`examples/${target==='ohif'?'ohif-browser':'monai-workflow'}/architecture.json`)));
 change({op:'add_architecture_model',model});
 const ohif={browser:['list-ui','canvas'],query:['list-http','list-id'],frames:['image-http','image-bytes'],orthanc:['server-pixels']};
 const monai={fixture:'input',input_read:'input',preprocess:'preprocess',train:'train',reference_inference:'train',checkpoint_save:'checkpoint',checkpoint_load:'checkpoint',inference:'inference',result_save:'result',readback:'reader'};
 for(const e of state.workspace.evidence){
  const pairs=target==='ohif'?Object.entries(ohif).flatMap(([object,observations])=>observations.map(id=>[object,{kind:'evidence',evidence_id:e.id,observation_id:id}])):[[monai[e.observation.subject],{kind:'reported_evidence',evidence_id:e.id,...e.observation}]];
  for(const [object,t] of pairs)change({op:'add_architecture_link',link:{id:`test-${state.workspace.architecture_links.length}`,model_id:model.id,model_version:model.version,object_id:object,target:t}});
 }
}
const server=spawn(binary,['review',casePath,'--port','0'],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);
let browser;
try{
 const url=await new Promise((resolve,reject)=>{let out='';const timer=setTimeout(()=>reject(Error(errors||'server timeout')),15000);server.stdout.on('data',d=>{out+=d;const m=/http:\/\/127\.0\.0\.1:\d+/.exec(out);if(m){clearTimeout(timer);resolve(m[0]);}});server.once('exit',()=>{clearTimeout(timer);reject(Error(errors||'server exited'));});});
 browser=await chromium.launch({headless:true,executablePath:process.env.REVIEW_BROWSER_EXECUTABLE||undefined,args:['--no-sandbox',...JSON.parse(process.env.REVIEW_BROWSER_ARGS||'[]')]});
 const context=await browser.newContext({viewport:{width:1440,height:1100}}),page=await context.newPage(),pageErrors=[];
 page.on('pageerror',e=>pageErrors.push(e.message));page.setDefaultTimeout(20000);await page.goto(url+'/architecture');
 await page.getByLabel('記入者',{exact:true}).fill('合成の確認者');await page.getByLabel('記録する理由').fill('図・疑問・証拠の往復を試す');
 const headers={'X-Review-Token':await page.locator('meta[name=review-token]').getAttribute('content')};
 const data=()=>fetch(url+'/api/v1/architecture',{headers}).then(r=>r.json());
 if(target==='presets'){
  for(const id of ['openemr-trial','ohif-trial','monai-trial']){
   await page.getByLabel('対象と構成の版').selectOption(id+':1');
   await page.getByLabel('図の詳しさ').selectOption('containers');
   await page.locator('likec4-view [data-eel-object="'+(id==='openemr-trial'?'db':id==='ohif-trial'?'orthanc':'checkpoint')+'"]').waitFor();
   await page.getByRole('button',{name:'この試験用構成を事例に追加する'}).click();
   await page.getByText('試験用構成を事例に追加しました。',{exact:true}).waitFor();
   assert((await data()).models.some(m=>m.id===id));
  }
  assert.equal((await data()).models.length,3);
  await page.reload();await page.getByLabel('対象と構成の版').selectOption('ohif-trial:1');await page.getByLabel('図の詳しさ').selectOption('containers');
  await page.locator('likec4-view [data-eel-object="orthanc"]').waitFor();
 }else{
  await page.getByLabel('対象と構成の版').selectOption(model.id+':1');await page.getByLabel('図の詳しさ').selectOption('containers');
  const groups=Map.groupBy(state.workspace.evidence,e=>e.manifest.correlation.run_id);
  assert.equal(await page.getByLabel('図に対応づける試行').locator('option').count(),groups.size+1);
  const views=(await data()).links;
  for(const [run,evs] of groups){
   await page.getByLabel('図に対応づける試行').selectOption(evs[0].id);
   const fault=target==='ohif'?evs[0].execution.recipe.trial.fault:evs[0].reported.context.fault;
   for(const object of target==='ohif'?['browser','orthanc','store']:['train','checkpoint','inference','reader']){
    const links=views.filter(v=>v.link.object_id===object&&evs.some(e=>e.id===v.link.target.evidence_id));
    let expected=links.length&&links.every(v=>v.applicable)?links.some(v=>v.observed==='fail')?'fail':links.some(v=>v.observed==null||v.observed==='not_observed')?'unknown':'pass':'unknown';
    const node=page.locator(`likec4-view [data-eel-object="${object}"]`);await node.waitFor();
    assert.equal(await node.getAttribute('data-eel-state'),expected,run+' '+object);
   }
   // Assert missing later stages remain unknown even after the earlier successful run.
   if(target==='monai'&&['checkpoint_corrupt','interrupted','save_is_directory','input_missing'].includes(fault)){
    const evidence=evs.find(e=>e.observation.subject==='inference'),link=views.find(v=>v.link.target.evidence_id===evidence.id);
    assert.equal(link.observed,'not_observed');
   }
   const object=target==='ohif'?'browser':fault==='checkpoint_corrupt'?'checkpoint':fault==='interrupted'?'train':'inference';
   await page.locator(`likec4-view [data-eel-object="${object}"]`).click();
   const criterion=target==='ohif'?'canvas':fault==='checkpoint_corrupt'?'monai.stage／checkpoint_load':fault==='interrupted'?'monai.stage／train':'monai.stage／inference';
   await page.locator('#details-body section.record-action').filter({hasText:criterion}).getByRole('button',{name:'試験結果と証拠を見る'}).click();
   const dialog=page.getByRole('dialog');await dialog.waitFor();
   if(target==='monai'){assert.match(await dialog.innerText(),/対象の実行経路の独立した証明ではありません/);if(fault==='checkpoint_corrupt')assert.match(await dialog.getByLabel('選択した観測条件').innerText(),/期待と異なる/);if(fault==='interrupted')assert.match(await dialog.innerText(),/この段階の成功・失敗の報告はありません/);}
   await dialog.getByRole('button',{name:'元の構成図へ戻る'}).click();
   assert.equal(await page.locator(`likec4-view [data-eel-object="${object}"]`).getAttribute('data-eel-selected'),'true');
   fs.mkdirSync(path.join(root,'test-results'),{recursive:true});
   await page.screenshot({path:path.join(root,`test-results/architecture-${target}-${run}-1440.png`),fullPage:true});
  }
  // Persist a synthetic unanswered question on the selected object, then return to its evidence.
  await page.getByLabel('自分の言葉で記入').fill('この段階の報告がないとき、誰が追加確認する？');
  await page.getByRole('button',{name:'疑問・期待を記録して対応づける'}).click();
  await page.getByText('対応づけを記録しました。',{exact:true}).waitFor();
  await page.locator('#details-body').getByText('この段階の報告がないとき、誰が追加確認する？',{exact:true}).waitFor();
  await page.locator('#details-body').getByRole('button',{name:'試験結果と証拠を見る'}).first().click();await page.getByRole('dialog').getByRole('button',{name:'元の構成図へ戻る'}).click();
  // The UI can create the new reported target as well as the execution target.
  const option=await page.getByLabel('試行と観測条件').locator('option').evaluateAll((options,target)=>options.map(o=>o.value).find(v=>target!=='monai'||JSON.parse(v).subject==='inference'),target);
  await page.getByLabel('試行と観測条件').selectOption(option);
  const before=(await data()).links.length;
  await page.getByRole('button',{name:'この観測を対応づける'}).click();await page.getByText('対応づけを記録しました。',{exact:true}).waitFor();
  const after=await data();assert.equal(after.links.length,before+1);assert.equal(after.links.at(-1).link.target.kind,target==='monai'?'reported_evidence':'evidence');
 }
 for(const width of [390,320]){
  await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'page overflow');
  fs.mkdirSync(path.join(root,'test-results'),{recursive:true});await page.screenshot({path:path.join(root,`test-results/architecture-${target}-${width}.png`),fullPage:true});
 }
 assert.deepEqual(pageErrors,[]);await context.close();console.log(JSON.stringify({target,checks:['pinned target model','scoped outcomes and missing stages','same run grouping','diagram to question/evidence and return','persisted UI links','1440/390/320px'],participant_evaluation:'not_conducted'}));
}finally{if(browser)await browser.close();server.kill('SIGTERM');fs.rmSync(temp,{recursive:true,force:true});}
