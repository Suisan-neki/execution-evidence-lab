// 読み込み宣言と関数名から候補を作る。実行時の束縛を確定する処理ではない。
import { maskCode, rustPath } from './scanner.js';
const IDENT = /^[A-Za-z_$][\w$]*$/;
function scopeCollision(file, name, range) {
  if(!range || !IDENT.test(name)) return false;
  const clean=maskCode(file.text,/\.rs$/.test(file.path),/\.py$/.test(file.path));
  const lines=clean.split('\n').slice(range.start-1,range.end);
  const offset=clean.split('\n').slice(0,range.start-1).join('\n').length+(range.start>1?1:0);
  const header=range.header_end!==undefined ? clean.slice(offset,range.header_end) : lines[0];
  const parameters=/\(([\s\S]*)\)/.exec(header)?.[1]||'';
  const escaped=name.replace(/[$]/g,'\\$');
  if(new RegExp('(?:^|[^\\w$])'+escaped+'(?:$|[^\\w$])').test(parameters)) return true;
  const endOffset=clean.split('\n').slice(0,range.end).join('\n').length;
  const remainder=range.header_end!==undefined ? clean.slice(range.header_end+1,endOffset) : lines.join('\n').slice(lines[0].length);
  return new RegExp('\\b(?:let|const|var|fn|function|def|class)\\s+(?:mut\\s+)?'+escaped+'(?![\\w$])|(?:^|\\n)\\s*'+escaped+'\\s*=').test(remainder);
}
export function callTargets(model, file, name, range) {
  const parts=name.split(/::|\./), local=parts[0], imported=parts.at(-1);
  const bindings=(file.bindings||[]).filter(b=>b.local===local && (!b.nested || (range && b.line > range.start && b.end_line <= range.end)));
  if(scopeCollision(file,local,range)) return {candidates:[],bindings,unread:[],reason:'引数や局所宣言に同名の記述があり、対応先を特定できません。'};
  let targets=[];
  if(bindings.length) {
    targets=bindings.filter(b=>parts.length===1 ? b.kind!=='namespace' : b.kind==='namespace' && parts.length===2)
      .map(b=>({...b,name:b.kind==='namespace'?imported:b.imported}));
  } else if(parts.length===1) targets=[{target:file.path,name:imported,kind:'local'}];
  else if(parts[0]==='crate') {
    const path=rustPath(parts.slice(1,-1),new Set((model.inventory||model.files).map(f=>f.path)));
    if(path) targets=[{target:path,name:imported,kind:'qualified'}];
  }
  const candidates=[], unread=[];
  for(const target of targets) {
    if(!target.target) continue;
    const destination=model.files.find(f=>f.path===target.target);
    if(!destination) {unread.push(target.target);continue;}
    let symbolName=target.name;
    if(target.kind==='default') {
      const clean=maskCode(destination.text);
      symbolName=/\bexport\s+default\s+(?:async\s+)?function\s+([A-Za-z_$][\w$]*)/.exec(clean)?.[1]
        || /\bexport\s+default\s+([A-Za-z_$][\w$]*)\s*;/.exec(clean)?.[1];
    }
    for(const symbol of destination.symbols.filter(s=>s.name===symbolName))
      if(!candidates.some(c=>c.path===destination.path&&c.line===symbol.line)) candidates.push({path:destination.path,line:symbol.line,name:symbol.name});
  }
  return {candidates,bindings,unread:[...new Set(unread)],reason:candidates.length ? (bindings.length?'読み込み宣言の名前と参照先の宣言から選んだ候補です。':'同じファイルの名前、またはcrateの記述から選んだ候補です。') : unread.length?'参照先をまだ読み取っていません。':'対応する宣言は抽出できませんでした。動的な呼び出しや未対応構文は未解決です。'};
}
