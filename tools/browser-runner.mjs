// 基盤に組み込むブラウザ操作。対象リポジトリのスクリプトは読み込まない。
import fs from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const request=JSON.parse(await fs.readFile(process.argv[2],'utf8'));
const dir=path.dirname(process.argv[2]), spec=request.spec;
const report={schema_version:1,run_id:request.run_id,version:'',prepared:false,completed:false,steps:[],responses:[],blocked_requests:0,detail:''};
let browser, page, secrets={}, masks=[], requests=new Map(), active=null, pending=new Set(), deadline=Date.now()+request.timeout_ms;
const redact=text=>{let s=String(text);for(const value of Object.values(secrets).filter(v=>typeof v==='string'&&v.length)){s=s.split(value).join('[秘匿]');}return s.slice(0,8192);};
const replace=s=>s.replaceAll('{{test_value}}',request.test_value);
const save=async(name,data)=>{const temp=path.join(dir,name+'.tmp');await fs.writeFile(temp,JSON.stringify(data),{mode:0o600,flag:'wx'});await fs.rename(temp,path.join(dir,name));};
function target(t){let scope=page;for(const frame of t.frames||[])scope=scope.frameLocator(frame);const l=t.locator;return l.by==='label'?scope.getByLabel(l.text,{exact:true}):l.by==='role'?scope.getByRole(l.role,{name:l.name,exact:true}):scope.locator(l.selector);}
function remaining(){const n=deadline-Date.now();if(n<=0)throw Error('ブラウザ試験が待機上限に達した');return Math.min(n,15000);}
async function steps(list,phase){
 for(const step of list){active=step.id;const r={id:step.id,phase,completed:false,text:null,screenshot:null,detail:''};report.steps.push(r);
  try{const options={timeout:remaining()};
   if(step.action==='goto'){const url=new URL(replace(step.path),request.origin);if(url.origin!==request.origin)throw Error('操作先のオリジンが不一致');await page.goto(url.href,{...options,waitUntil:'domcontentloaded'});}
   else if(step.action==='fill'){const value=step.value.source==='secret'?secrets[step.value.key]:replace(step.value.value);if(typeof value!=='string')throw Error('試験用の認証情報が未設定');const locator=target(step.target);if(step.value.source==='secret')masks.push(locator);await locator.fill(value,options);}
   else if(step.action==='click'){await target(step.target).click(options);}
   else if(step.action==='select'){await target(step.target).selectOption(replace(step.value),options);}
   else if(step.action==='read_text'){const base=target(step.target);const locator=step.contains?base.filter({hasText:replace(step.contains)}):base;await locator.waitFor({...options,state:'visible'});const text=await locator.innerText(options);if(text.length>8192)throw Error('表示の文字列が上限を超えた');r.text=redact(text);}
   else throw Error('未対応の操作');
   r.completed=true;r.detail='操作を完了した。保存の成功を示す記録とは別';
   if(!(step.action==='fill'&&step.value.source==='secret')){const name=`${phase}-${step.id}.png`;await page.screenshot({path:path.join(dir,name),timeout:remaining(),mask:[...masks,...page.frames().map(f=>f.locator('input[type=password]'))],animations:'disabled'});r.screenshot='browser/'+name;}
  }catch(error){r.detail=redact(error.message);report.detail=`${phase}の手順 ${step.id} を完了できなかった`;return false;}
 }
 active=null;return true;
}
try{
 if(process.env.EEL_BROWSER_SECRETS_FILE)secrets=JSON.parse(await fs.readFile(process.env.EEL_BROWSER_SECRETS_FILE,'utf8'));
 const {chromium}=await import(pathToFileURL(process.env.EEL_PLAYWRIGHT_MODULE).href);
 browser=await chromium.launch({headless:true,executablePath:process.env.EEL_BROWSER_EXECUTABLE||undefined,args:['--no-sandbox']});report.version=browser.version();
 if(report.version!==spec.expected_version)throw Error('ブラウザの版が試験設定と異なる');
 const context=await browser.newContext({viewport:{width:1440,height:1000},ignoreHTTPSErrors:spec.tls,serviceWorkers:'block',acceptDownloads:false});
 await context.route('**/*',async route=>{let allowed=false;try{allowed=request.origins.includes(new URL(route.request().url()).origin);}catch{}if(allowed)await route.continue();else{report.blocked_requests++;await route.abort('blockedbyclient');}});
 await context.routeWebSocket('**/*',ws=>ws.close());
 page=await context.newPage();page.on('dialog',d=>d.accept());page.on('download',d=>d.cancel());
 page.on('request',r=>requests.set(r,active));
 page.on('requestfinished',r=>requests.delete(r));page.on('requestfailed',r=>requests.delete(r));
 page.on('response',r=>{
  const p=(async()=>{const request_step=requests.get(r.request());if(!spec.actions.some(s=>s.id===request_step))return;const u=new URL(r.url());if(!request.origins.includes(u.origin))return;for(const w of spec.responses){if(w.path!==u.pathname||w.method!==r.request().method()||report.responses.length>=200)continue;let body_match=null;if(w.contains!=null){try{const body=await r.body();if(body.length<=1024*1024)body_match=body.toString('utf8').includes(replace(w.contains));}catch{}}
   report.responses.push({watch:w.id,method:r.request().method(),path:u.pathname,status:r.status(),body_match,request_step});}
  })().catch(()=>{});pending.add(p);p.finally(()=>pending.delete(p));
 });
 await page.goto(new URL(spec.start_path,request.origin).href,{timeout:remaining(),waitUntil:'domcontentloaded'});
 report.prepared=await steps(spec.preparation,'preparation');await save('ready.json',report);
 if(report.prepared){while(true){if(await fs.stat(path.join(dir,'stop')).then(()=>true,()=>false))throw Error('停止を要求された');if(await fs.stat(path.join(dir,'continue.json')).then(()=>true,()=>false)){const control=JSON.parse(await fs.readFile(path.join(dir,'continue.json'),'utf8'));if(control.run_id!==report.run_id||control.proceed!==true)throw Error('継続する試行が一致しない');break;}remaining();await new Promise(r=>setTimeout(r,30));}
  // 準備中の通信と、故障後の操作で観測した通信を混ぜない。
  await Promise.all([...pending]);report.responses=[];requests.clear();report.completed=await steps(spec.actions,'action');
 }
 await Promise.race([Promise.all([...pending]),new Promise(r=>setTimeout(r,Math.min(2000,Math.max(1,deadline-Date.now()))))]);
}catch(error){report.detail=redact(error.message);}finally{if(browser)await browser.close().catch(()=>{});if(!report.detail)report.detail=report.completed?'宣言した操作を完了した。画面・HTTP・DBの記録を別々に確認する':'操作を完了していない';
 if(!await fs.stat(path.join(dir,'ready.json')).then(()=>true,()=>false))await save('ready.json',report);
 await save('result.json',report);
}
