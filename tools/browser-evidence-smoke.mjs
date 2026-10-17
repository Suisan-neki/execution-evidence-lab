// 写真の実行試験とは別に、結果画面の表示・認証ヘッダー・欠落時の案内を合成記録で確かめる。
import assert from 'node:assert/strict';
import {spawn,execFileSync} from 'node:child_process';
import {mkdtemp,rm,mkdir,readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,join} from 'node:path';
import {chromium} from 'playwright';
const root=resolve(import.meta.dirname,'..'),binary=process.env.REVIEW_BINARY||join(root,'target/debug/execution-evidence-lab');
const temp=await mkdtemp(join(tmpdir(),'eel-browser-evidence-')),casePath=join(temp,'case');
execFileSync(binary,['case','init',casePath,'--actor','合成の確認者','--reason','画面写真の表示試験']);
execFileSync(binary,['case','run',casePath,'--actor','合成の確認者','--reason','画面写真の表示試験','--condition','save','--fault','normal']);
const server=spawn(binary,['review',casePath,'--port','0'],{stdio:['ignore','pipe','pipe']});let stderr='',browser;server.stderr.on('data',d=>stderr+=d);
try{
 const url=await new Promise((accept,reject)=>{let text='';const timer=setTimeout(()=>reject(Error(stderr||'server timeout')),10000);server.stdout.on('data',d=>{text+=d;const m=/http:\/\/127\.0\.0\.1:\d+/.exec(text);if(m){clearTimeout(timer);accept(m[0]);}});server.once('exit',()=>{clearTimeout(timer);reject(Error(stderr||'server exited'));});});
 browser=await chromium.launch({headless:true,executablePath:process.env.REVIEW_BROWSER_EXECUTABLE||undefined,args:['--no-sandbox']});
 const context=await browser.newContext({viewport:{width:1440,height:1100}}),page=await context.newPage();let available=true,photoRequests=0;const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/api/case',async route=>{const response=await route.fetch();const state=await response.json();const evidence=state.workspace.evidence[0];const recipe=JSON.parse(await readFile(join(root,'examples/openemr-browser/recipe.json'),'utf8'));
 evidence.execution={schema_version:1,recipe,tree:'a'.repeat(40),files:[],runtime_version:'synthetic',mode:'local',snapshot_unchanged:true,stdout_excerpt:'',stderr_excerpt:'',diagnostics:[],observations:[],
   containers:{
     browser:{version:'151.0.7922.34',prepared:true,completed:false,detail:'途中で止まった合成の記録',
       steps:[{id:'saved',phase:'action',completed:true,text:'保存しました <img src=x onerror=alert(1)>',detail:'操作完了',screenshot:'browser/action-saved.png'},
              {id:'missing',phase:'action',completed:false,text:null,detail:'要素が見つからない',screenshot:null}],
       responses:[{watch:'save',request_step:'saved',method:'POST',path:'/save',status:200,body_match:null}]},
     database_reads:[{id:'row',performed:false,exit_code:null,text:'',detail:'DB停止により読取未完了'}]
   }};
 await route.fulfill({response,json:state});});
 await page.route('**/api/v1/browser-screenshot/**',async route=>{photoRequests++;assert.ok(route.request().headers()['x-review-token']);await route.fulfill(available?{status:200,contentType:'image/png',body:Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=','base64')}:{status:404,contentType:'application/json',body:JSON.stringify({error:'この事例内に対応する画面写真がありません。'})});});
 await page.goto(url+'/review');await page.getByText('変更履歴と実行時の詳しい記録',{exact:true}).click();const panel=page.locator('.browser-evidence');await panel.waitFor();await panel.locator('summary').click();
 assert.match(await panel.innerText(),/本文は未観測/);assert.match(await panel.innerText(),/DB停止により読取未完了/);assert.match(await panel.innerText(),/操作未完了/);assert.equal(await panel.locator('img').count(),0);
 await panel.getByRole('button',{name:'savedの画面写真を開く'}).click();const dialog=page.locator('.browser-photo-dialog');await dialog.locator('img').waitFor();await page.waitForFunction(()=>document.querySelector('.browser-photo-dialog img')?.naturalWidth===1);assert.equal(photoRequests,1);
 await mkdir(join(root,'test-results'),{recursive:true});await page.screenshot({path:join(root,'test-results/browser-evidence-1440.png'),fullPage:true});await dialog.getByRole('button',{name:'画面写真を閉じる'}).click();assert.equal(await dialog.count(),0);
 available=false;await panel.getByRole('button',{name:'savedの画面写真を開く'}).click();await page.getByRole('status').filter({hasText:'この事例内に対応する画面写真がありません'}).waitFor();assert.equal(await dialog.locator('img').count(),0);await page.keyboard.press('Escape');
 await page.setViewportSize({width:390,height:1000});await page.screenshot({path:join(root,'test-results/browser-evidence-390.png'),fullPage:true});assert.deepEqual(errors,[]);
 console.log(JSON.stringify({checks:['separate display/HTTP/DB sections','partial steps','escaped page text','authenticated photo request','photo dialog','missing photo message','1440px and 390px screenshots']}));
}finally{if(browser)await browser.close();server.kill('SIGTERM');await rm(temp,{recursive:true,force:true});}
