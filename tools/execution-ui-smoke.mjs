// 画面で起動した固定版の実行・再試験・競合を、合成データで確認する。
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { chromium } from 'playwright';
const root=resolve(import.meta.dirname,'..'), binary=process.env.REVIEW_BINARY||join(root,'target/debug/execution-evidence-lab');
const temp=await mkdtemp(join(tmpdir(),'execution-ui-')), repository=join(temp,'repo'), casePath=join(temp,'case'), output=join(root,'test-results');
await mkdir(repository);await mkdir(output,{recursive:true});
const git=(...args)=>execFileSync('git',['-C',repository,...args],{encoding:'utf8'}).trim();
git('init','-q');git('config','user.name','Synthetic');git('config','user.email','test@example.invalid');git('remote','add','origin','https://github.com/example/observable-store.git');
await mkdir(join(repository,'examples/observable-store'),{recursive:true});await mkdir(join(repository,'adapters/observation'),{recursive:true});
let code=await readFile(join(root,'examples/observable-store/app.mjs'),'utf8');
code=code.replace("const input = JSON.parse(readFileSync(process.env.EEL_INPUT, 'utf8'));", "const input = JSON.parse(readFileSync(process.env.EEL_INPUT, 'utf8'));\nif (input.delay_ms) await new Promise(resolve => setTimeout(resolve, input.delay_ms));");
await writeFile(join(repository,'examples/observable-store/app.mjs'),code);
await writeFile(join(repository,'adapters/observation/observe.mjs'),await readFile(join(root,'adapters/observation/observe.mjs')));
git('add','.');git('commit','-qm','synthetic UI execution');const commit=git('rev-parse','HEAD');
const source={repository:'example/observable-store',commit,path:'examples/observable-store/app.mjs',line:1,end_line:1};
const criterion={assertion:'stored_matches_input',subject:'record-1'};
const spec={schema_version:1,revision:1,id:'ui-execution',title:'画面から固定版を試す合成事例',statements:[],context:{},checks:[{id:'stored',version:1,text:'試行用の保存物を読み出し、入力と一致する',origin:'合成の仕様',source,observation:criterion,rule:'observed_assertion',dependencies:['execution-stored'],questions:[]}],evidence:[],decisions:[],history:[]};
await writeFile(join(temp,'spec.json'),JSON.stringify(spec));execFileSync(binary,['case','init',casePath,'--file',join(temp,'spec.json')]);
const server=spawn(binary,['review',casePath,'--port','0','--repository-root',repository,'--execution-mode','local'],{stdio:['ignore','pipe','pipe']});
let browser;
try {
  const url=await new Promise((accept,reject)=>{let stdout='';const timer=setTimeout(()=>reject(Error('server did not start')),10000);server.stdout.on('data',d=>{stdout+=d;const match=/http:\/\/127\.0\.0\.1:\d+/.exec(stdout);if(match){clearTimeout(timer);accept(match[0]);}});server.once('exit',code=>{clearTimeout(timer);reject(Error('server exit '+code));});});
  browser=await chromium.launch({headless:true,executablePath:process.env.REVIEW_BROWSER_EXECUTABLE||undefined,args:['--no-sandbox']});
  const context=await browser.newContext({viewport:{width:1440,height:1100}}), page=await context.newPage(), errors=[];
  page.on('pageerror',e=>errors.push(e.message));await page.goto(url+'/review');await page.locator('#check-stored').waitFor();
  const state=async()=> (await context.request.get(url+'/api/case')).json();
  await page.locator('#actor').fill('合成の確認者');await page.locator('#reason').fill('画面から固定版の失敗と再試験を確かめる');
  await page.locator('#check-stored .external-setup summary').click();
  await page.locator('#args-stored').fill('["failure"]');await page.locator('#input-stored').fill('{"message":"合成の入力","delay_ms":700}');
  async function start() {
    const response=page.waitForResponse(r=>r.url().endsWith('/api/execute')&&r.request().method()==='POST');
    await page.locator('.execute-recipe[data-check="stored"]').click();const accepted=await response;
    assert.equal(accepted.status(),202);return accepted;
  }
  const accepted=await start(), request=accepted.request();
  const auth={Origin:url,'X-Review-Token':request.headers()['x-review-token']};
  await page.waitForFunction(()=>document.querySelector('.execute-recipe').disabled);
  assert.ok((await page.locator('#execution-status').innerText()).includes('実行中'));
  assert.equal((await state()).workspace.evidence.length,0);
  const duplicate=await context.request.post(url+'/api/execute',{headers:auth,data:request.postDataJSON()});assert.equal(duplicate.status(),409);
  await page.screenshot({path:join(output,'execution-running-desktop.png'),fullPage:true});
  await page.waitForFunction(()=>document.querySelector('#check-stored .badge').textContent==='条件を満たさない');
  assert.equal((await state()).workspace.evidence.length,1);
  await page.locator('.load-prior-recipe[data-check="stored"]').click();await page.locator('#args-stored').fill('["normal"]');
  await start();await page.waitForFunction(()=>document.querySelector('#check-stored .badge').textContent==='条件を満たす');
  let current=await state();assert.equal(current.workspace.evidence.length,2);
  assert.equal(current.workspace.evidence[0].execution.recipe.args[0],'failure');assert.equal(current.workspace.evidence[1].execution.recipe.args[0],'normal');
  await page.locator('#args-stored').fill('["timeout"]');await page.locator('#timeout-stored').fill('80');
  await start();await page.waitForFunction(()=>document.querySelector('#check-stored .badge').textContent==='判定できない' && document.querySelector('#execution-status').textContent.includes('取り込みました'));
  current=await state();assert.equal(current.workspace.evidence.length,3);assert.equal(current.workspace.evidence[2].execution.timed_out,true);
  // 実行中の変更は取り込まず、試行のcomplete.jsonを残す。
  await page.locator('#args-stored').fill('["normal"]');await page.locator('#timeout-stored').fill('5000');
  const beforeRuns=await readdir(join(casePath,'runs'));await start();
  let newRun;
  for(let i=0;i<200;i++) {
    const runs=await readdir(join(casePath,'runs'));newRun=runs.find(r=>!beforeRuns.includes(r));
    if(newRun) break;await new Promise(resolve=>setTimeout(resolve,5));
  }
  assert.ok(newRun);
  const changed=await context.request.post(url+'/api/change',{headers:auth,data:{expected_revision:current.workspace.revision,actor:'合成の確認者',reason:'実行中の変更の反例',action:{op:'set_context',key:'configuration',value:'changed'}}});assert.equal(changed.status(),200);
  await page.waitForFunction(()=>document.querySelector('#execution-status').textContent.includes('取り込みを完了できませんでした'));
  current=await state();assert.equal(current.workspace.evidence.length,3);assert.equal(current.workspace.context.configuration,'changed');
  assert.ok((await page.locator('#execution-status').innerText()).includes('試行記録'));
  assert.ok(JSON.parse(await readFile(join(casePath,'runs',newRun,'complete.json'),'utf8')));
  await page.screenshot({path:join(output,'execution-conflict-desktop.png'),fullPage:true});
  await start();await page.waitForFunction(()=>document.querySelector('#check-stored .badge').textContent==='条件を満たす' && document.querySelector('#execution-status').textContent.includes('取り込みました'));
  assert.equal((await state()).workspace.evidence.length,4);
  await page.setViewportSize({width:390,height:844});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.screenshot({path:join(output,'execution-completed-mobile.png'),fullPage:true});
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({browser:await browser.version(),checks:['opt-in target','start and status','reads during execution','duplicate rejected','failure and retest','timeout stays unknown','concurrent edit keeps run without importing','retry after conflict','mobile layout'],data:'synthetic',human_evaluation:'not_conducted'}));
} finally {await browser?.close();server.kill('SIGTERM');await rm(temp,{recursive:true,force:true});}
