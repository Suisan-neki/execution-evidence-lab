import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {fixture,studyUID,seriesUID,sopUID,patientID} from '../examples/ohif-browser/fixture.mjs';

const root=path.resolve(import.meta.dirname,'..');
const bin=process.env.REVIEW_BINARY||path.join(root,'target/debug/execution-evidence-lab');
const buildSummary=JSON.parse(fs.readFileSync(path.join(root,'evaluations/ci-environments/ohif-build/summary.json')));
const buildDir=buildSummary.normal,buildRecord=JSON.parse(fs.readFileSync(path.join(buildDir,'environment.json')));
const builtManifest=fs.readFileSync(path.join(buildDir,'result/build-files.json'));
const output=path.join(root,'evaluations/ci-ohif');fs.mkdirSync(output,{recursive:true});
const source=buildRecord.recipe.source;
const browserInfo=JSON.parse(fs.readFileSync(path.join(root,'node_modules/playwright-core/browsers.json'))).browsers.find(b=>b.name==='chromium');
const data=fixture(),pixelHash=createHash('sha256').update(data.pixels).digest('hex');
fs.writeFileSync(path.join(output,'synthetic.dcm'),data.file);
const framePath=`/dicom-web/studies/${studyUID}/series/${seriesUID}/instances/${sopUID}/frames/1`;
const css=selector=>({locator:{by:'css',selector}});
const webBuild={run_id:path.basename(buildDir),manifest_sha256:createHash('sha256').update(builtManifest).digest('hex'),source};
const nginx='nginx:1.27.4-alpine@sha256:4ff102c5d78d254a6f0da062b3cf39eaf07f01eec0927fd21e219d0af8bc0591';
const orthanc='orthancteam/orthanc:26.9.1@sha256:d2705f2c56547e55ce9bc2250fd22d05d326493b3f3d9fa191ecb7db8ca6b5a2';
function command(args,options={}){
 const r=spawnSync(bin,args,{encoding:'utf8',maxBuffer:32*1024*1024,timeout:360000,...options});
 assert.equal(r.error,undefined,String(r.error));assert.equal(r.status,0,(r.stderr||'').slice(-12000));
 return JSON.parse(r.stdout);
}
function recipe(rejected){
 const config={routerBasename:null,showStudyList:true,extensions:[],modes:[],showWarningMessageForCrossOrigin:false,showCPUFallbackMessage:false,showLoadingIndicator:true,maxNumberOfWebWorkers:1,defaultDataSourceName:'local',dataSources:[{namespace:'@ohif/extension-default.dataSourcesModule.dicomweb',sourceName:'local',configuration:{friendlyName:'Synthetic Orthanc',name:'Orthanc',qidoRoot:'/dicom-web',wadoRoot:'/dicom-web',wadoUriRoot:'/wado',qidoSupportsIncludeField:false,imageRendering:'wadors',thumbnailRendering:'wadors',enableStudyLazyLoad:true,supportsFuzzyMatching:false,supportsWildcard:false,staticWado:false,omitQuotationForMultipartRequest:true}}]};
 const server=`server { listen 80; root /usr/share/nginx/html; add_header Cross-Origin-Opener-Policy same-origin always; add_header Cross-Origin-Embedder-Policy require-corp always; location / { try_files $uri $uri/ /index.html; } ${rejected?'location ~ /dicom-web/.*/frames/ { return 503; }':''} location /dicom-web/ { proxy_pass http://orthanc:8042; } location /orthanc/ { proxy_pass http://orthanc:8042/; } location /wado { proxy_pass http://orthanc:8042; } }`;
 const setup=`<!doctype html><html><p id="prepared">preparing synthetic fixture</p><script>(async()=>{try{const d=await fetch('/eel-synthetic.dcm').then(r=>r.arrayBuffer());const r=await fetch('/orthanc/instances',{method:'POST',headers:{'Content-Type':'application/dicom'},body:d});if(!r.ok)throw Error('upload '+r.status);document.querySelector('#prepared').textContent='fixture loaded';}catch(e){document.querySelector('#prepared').textContent=e.message;}})();</script></html>`;
 const env={EEL_CONFIG:'window.config='+JSON.stringify(config)+';',EEL_NGINX:server,EEL_SETUP:setup};
 const encoded=data.file.toString('base64'),parts=[];
 for(let i=0;i<encoded.length;i+=3000){const key='EEL_DICOM_'+parts.length;env[key]=encoded.slice(i,i+3000);parts.push('$'+key);}
 const shell=`printf '%s' "$EEL_NGINX" > /etc/nginx/conf.d/default.conf; printf '%s' "$EEL_CONFIG" > /usr/share/nginx/html/app-config.js; printf '%s' "$EEL_SETUP" > /usr/share/nginx/html/eel-setup.html; printf '%s' "${parts.join('')}" | base64 -d > /usr/share/nginx/html/eel-synthetic.dcm; exec nginx -g 'daemon off;'`;
 const check=(id,scope,method,target,text)=>({id,scope,method,target,text,expect:'contains'});
 return {schema_version:1,check_id:'image',source,runtime:'browser',entrypoint:'/',args:[],input:{synthetic_fixture_sha256:createHash('sha256').update(data.file).digest('hex'),studyUID,seriesUID,sopUID,pixel_sha256:pixelHash,request_rejected:rejected},criterion:{assertion:'eel.trial',subject:'image'},timeout_ms:180000,
  trial:{schema_version:1,fault:rejected?'http_rejected':'normal',checks:[
   check('list-ui','検査一覧の表示','browser_text','list-ui','SYNTHETIC'),check('list-http','検査一覧の取得','browser_response','list','status=200'),check('list-id','検査識別子の照合','browser_response','list','body_match=true'),
   check('image-http','画像のHTTP応答','browser_response','frame','status=200'),check('image-bytes','ブラウザーが取得した画素','browser_response','frame','sha256='+pixelHash),
   check('canvas','画像領域の明暗','browser_canvas','canvas','variation=true'),check('server-pixels','画像サーバーの直接読取','server_http','frame-direct','sha256='+pixelHash),
  ]},
  containers:{services:[{name:'orthanc',image:orthanc,role:'other',expose:8042,env:{DICOM_WEB_PLUGIN_ENABLED:'true',ORTHANC__REMOTE_ACCESS_ALLOWED:'true',ORTHANC__AUTHENTICATION_ENABLED:'false'},volumes:[],command:[],ready:{http:{path:'/system',tls:false}},ready_timeout_ms:60000},
   {name:'viewer',image:nginx,role:'app',expose:80,env,volumes:[],web_build:webBuild,command:['sh','-ec',shell],ready:{http:{path:'/index.html',tls:false}},ready_timeout_ms:30000}]},
  browser:{service:'viewer',tls:false,start_path:'/eel-setup.html',expected_version:browserInfo.browserVersion,
   preparation:[{id:'fixture-ready',action:'read_text',target:css('#prepared'),contains:'fixture loaded'}],
   actions:[{id:'open-list',action:'goto',path:'/'},{id:'list-ui',action:'read_text',target:css('table tbody'),contains:'SYNTHETIC'},{id:'open-viewer',action:'goto',path:'/viewer?StudyInstanceUIDs='+studyUID},{id:'canvas',action:'read_canvas',target:css('[data-cy="viewport-pane"] canvas:visible'),min_levels:64,min_pixels:4096}],
   responses:[{id:'list',path:'/dicom-web/studies',method:'GET',contains:studyUID},{id:'frame',path:framePath,method:'GET',payload:'dicom_frame'}],database:[],
   http_reads:[{id:'frame-direct',service:'orthanc',path:framePath,payload:'dicom_frame'}],rejection:rejected?{watch:'frame',status:503}:null},
 };
}
// One case keeps normal, actual rejection and a new normal run without overwriting evidence.
const caseDir=path.join(output,'case');
const spec={schema_version:1,revision:1,id:'ohif-image',title:'OHIFで合成画像を取得して表示する',statements:[],context:{},checks:[{id:'image',version:1,text:'試験用の検査を一覧に出し、指定した画像データと明暗を表示できる',origin:'Codexが選んだ試験用の条件',source,observation:{assertion:'eel.trial',subject:'image'},rule:'manual_only',dependencies:[],questions:[]}],evidence:[],decisions:[],history:[]};
fs.writeFileSync(path.join(output,'case-spec.json'),JSON.stringify(spec,null,2));command(['case','init',caseDir,'--file',path.join(output,'case-spec.json')]);
const results=[];
for(const [label,rejected] of [['normal',false],['image-rejected',true],['retry',false]]){
 const recipePath=path.join(output,label+'-recipe.json');fs.writeFileSync(recipePath,JSON.stringify(recipe(rejected),null,2));
 const view=command(['case','execute',caseDir,recipePath,'--repository-root',path.join(root,'evaluations/ci-environments/ohif-build/repository'),'--mode','local','--actor','合成の試験担当','--reason',label],{env:{...process.env,EEL_PREPARED_ENVIRONMENTS:path.dirname(buildDir)}});
 const evidence=view.workspace.evidence.at(-1),trace=evidence.execution;
 fs.writeFileSync(path.join(output,label+'.json'),JSON.stringify({verdict:view.checks[0].verdict,evidence},null,2));
 console.log(label,JSON.stringify({verdict:view.checks[0].verdict,steps:trace.containers.browser.steps,checks:trace.trial.results,http:trace.containers.browser.responses,direct:trace.containers.http_reads}));
 const check=id=>trace.trial.results.find(c=>c.id===id).observed;
 for(const id of ['list-ui','list-http','list-id','server-pixels'])assert.equal(check(id),'pass',label+' '+id);
 if(rejected){assert.equal(check('image-http'),'fail');assert.notEqual(check('image-bytes'),'pass');assert.notEqual(check('canvas'),'pass');assert.equal(trace.trial.fault_effective,true);assert.equal(view.checks[0].verdict,'not_satisfied');}
 else{for(const c of trace.trial.results)assert.equal(c.observed,'pass',label+' '+c.id);assert.equal(view.checks[0].verdict,'satisfied');}
 assert.deepEqual(trace.containers.leftovers,[]);assert.equal(trace.containers.services.find(s=>s.name==='viewer').web_build.manifest_sha256,webBuild.manifest_sha256);
 results.push({label,evidence_id:evidence.id,run_id:evidence.manifest.correlation.run_id,verdict:view.checks[0].verdict});
}
fs.writeFileSync(path.join(output,'summary.json'),JSON.stringify({source,web_build:webBuild,studyUID,seriesUID,sopUID,pixelHash,results,human_evaluation:'not_conducted'},null,2));
console.log('Actual OHIF list, frame bytes, canvas pixels, direct read, rejection and retry passed.');
