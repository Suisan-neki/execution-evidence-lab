// 既存3対象と参照アプリの構成モデルから、同じWeb Componentに全体・内部の図を生成する。
import {readFile,writeFile,mkdtemp,mkdir,rm} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {execFileSync} from 'node:child_process';
import {gzipSync} from 'node:zlib';
const root=resolve(import.meta.dirname,'..'),out=join(root,'ui/architecture-generated');
const paths=['examples/openemr-browser/architecture.json','examples/ohif-browser/architecture.json','examples/monai-workflow/architecture.json','examples/queue-display/architecture.json'];
const models=await Promise.all(paths.map(p=>readFile(join(root,p),'utf8').then(JSON.parse)));
const temp=await mkdtemp(join(tmpdir(),'eel-likec4-')),q=JSON.stringify;
function info(m){
 const prefix=m.id==='openemr-trial'?'':m.id.replaceAll('-','_')+'__',byId=new Map(m.elements.map(e=>[e.id,e]));
 const fqn=id=>{const e=byId.get(id);return e.parent?`${fqn(e.parent)}.${id}`:prefix+id;};
 const views={context:prefix+'context',containers:prefix+'containers'};
 function element(e,indent='  '){
  return `${indent}${e.parent?e.id:prefix+e.id} = ${e.kind} ${q(e.title)} {\n${indent}  description ${q(e.description)}\n${indent}  style { size xs\n${indent}    textSize lg }\n${m.elements.filter(c=>c.parent===e.id).map(c=>element(c,indent+'  ')).join('')}${indent}}\n`;
 }
 const roots=m.elements.filter(e=>!e.parent);
 return {m,fqn,views,source:roots.map(e=>element(e)).join('')+m.relations.map(r=>`  ${fqn(r.from)} -> ${fqn(r.to)} ${q(r.title)} {\n    description ${q(r.description)}\n    metadata { eelId ${q(r.id)}\n      eelModel ${q(m.id)} }\n  }`).join('\n'),
  viewSource:`view ${views.context} {\n title ${q(m.title+'・全体')}\n include ${roots.map(e=>fqn(e.id)).join(', ')}\n autoLayout TopBottom\n}\nview ${views.containers} {\n title ${q(m.title+'・内部')}\n include ${roots.map(e=>fqn(e.id)+(['system','boundary'].includes(e.kind)?'.*':'')).join(', ')}\n autoLayout ${m.id==='openemr-trial'?'LeftRight':'TopBottom'}\n}\n`};
}
const inputs=models.map(info);
const source=`specification {\n element person { style { shape person } }\n element system\n element app\n element component\n element database { style { shape storage } }\n element storage { style { shape storage } }\n element boundary\n}\nmodel {\n${inputs.map(i=>i.source).join('\n')}\n}\nviews {\n${inputs.map(i=>i.viewSource).join('\n')}\n}\n`;
try{
 await mkdir(out,{recursive:true});await writeFile(join(temp,'model.c4'),source);
 const cli=join(root,'node_modules/likec4/bin/likec4.mjs');
 execFileSync(process.execPath,[cli,'validate',temp],{stdio:'inherit'});
 execFileSync(process.execPath,[cli,'export','json',temp,'-o',join(temp,'model.json')],{stdio:'inherit'});
 execFileSync(process.execPath,[cli,'codegen','webcomponent',temp,'-o',join(temp,'bundle.js')],{stdio:'inherit'});
 const layout=JSON.parse(await readFile(join(temp,'model.json'),'utf8')),modelMaps={};
 const value=(r,key)=>Array.isArray(r.metadata?.[key])?r.metadata[key][0]:r.metadata?.[key];
 for(const i of inputs){
  const selected=Object.values(i.views).map(id=>{if(!layout.views[id])throw Error('Missing view '+id);return layout.views[id];});
  for(const e of i.m.elements.filter(e=>e.parent))if(!selected[1].nodes.some(n=>n.id===i.fqn(e.id)))throw Error('内部の要素を展開できていません: '+i.m.id+'/'+e.id);
  const nodes=Object.fromEntries(i.m.elements.map(e=>[i.fqn(e.id),e.id]));
  const relations=Object.fromEntries(Object.entries(layout.relations).filter(([,r])=>value(r,'eelModel')===i.m.id).map(([id,r])=>[id,value(r,'eelId')]));
  const edges=Object.fromEntries(selected.flatMap(v=>v.edges.map(e=>[e.id,e.relations.map(r=>relations[r]).filter(Boolean)])));
  modelMaps[i.m.id]={nodes,edges,views:i.views};
 }
 await writeFile(join(out,'model.c4'),source);
 await writeFile(join(out,'ids.json'),JSON.stringify({...modelMaps['openemr-trial'],models:modelMaps}));
 const original=await readFile(join(temp,'bundle.js'),'utf8'),fonts=original.match(/@font-face\{[^}]*\}/g)||[];
 if(fonts.length!==6)throw Error('LikeC4のフォント定義を再確認してください');
 const bundle=Buffer.from(original.replace(/@font-face\{[^}]*\}/g,''));
 await writeFile(join(out,'likec4.js.gz'),gzipSync(bundle,{level:9,mtime:0}));
 for(const [pkg,name] of [['likec4','LikeC4'],['react','React'],['react-dom','ReactDOM'],['@likec4/core','LikeC4Core']])await writeFile(join(out,'LICENSE-'+name),await readFile(join(root,'node_modules',pkg,'LICENSE')));
 const published=await readFile(join(root,'node_modules/likec4/react/index.mjs'),'utf8');
 const notices=[...new Set((published.match(/\/\*+[\s\S]*?\*\//g)||[]).filter(c=>/@license|copyright/i.test(c)))];
 await writeFile(join(out,'THIRD-PARTY-NOTICES.txt'),'License notices retained from likec4@1.60.0/react/index.mjs.\n\n'+notices.join('\n\n')+'\n');
 console.log(JSON.stringify({likec4:'1.60.0',models:models.map(m=>m.id),views:inputs.flatMap(i=>Object.values(i.views)),bytes:bundle.length,gzip_bytes:gzipSync(bundle,{level:9}).length}));
}finally{await rm(temp,{recursive:true,force:true});}
