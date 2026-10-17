import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const target=process.argv[2];
assert(['ohif-build','monai-cpu'].includes(target),'target: ohif-build | monai-cpu');
const output=path.resolve('evaluations/ci-environments',target);
fs.mkdirSync(output,{recursive:true});
const original=path.resolve('examples',target,'recipe.json');
const recipe=JSON.parse(fs.readFileSync(original));
const root=path.join(output,'repository');
function command(program,args,options={}) {
  const result=spawnSync(program,args,{encoding:'utf8',maxBuffer:32*1024*1024,timeout:45*60*1000,...options});
  assert.equal(result.error,undefined,String(result.error));
  assert.equal(result.status,0,result.stderr?.slice(-12000));
  return result.stdout.trim();
}
fs.mkdirSync(root);
command('git',['init','-q',root]);
command('git',['-C',root,'remote','add','origin',`https://github.com/${recipe.source.repository}.git`]);
command('git',['-C',root,'fetch','--depth=1','--no-tags','origin',recipe.source.commit]);
assert.equal(command('git',['-C',root,'rev-parse','FETCH_HEAD']),recipe.source.commit);
const bin=path.resolve('target/debug/execution-evidence-lab');
function run(label,spec,success) {
  const dir=path.join(output,label);fs.mkdirSync(dir);
  const config=path.join(dir,'recipe.json');
  fs.writeFileSync(config,JSON.stringify(spec,null,2));
  for(const support of spec.support){
    const destination=path.join(dir,support.path);fs.mkdirSync(path.dirname(destination),{recursive:true});
    fs.copyFileSync(path.join(path.dirname(original),support.path),destination);
  }
  const result=spawnSync(bin,['environment','run',config,'--repository-root',root,'--output',path.join(dir,'runs')],{encoding:'utf8',maxBuffer:32*1024*1024,timeout:45*60*1000});
  fs.writeFileSync(path.join(dir,'cli.stdout.log'),result.stdout??'');
  fs.writeFileSync(path.join(dir,'cli.stderr.log'),result.stderr??'');
  assert.equal(result.error,undefined,String(result.error));
  const runs=fs.readdirSync(path.join(dir,'runs'));assert.equal(runs.length,1);
  const runDir=path.join(dir,'runs',runs[0]);
  const record=JSON.parse(fs.readFileSync(path.join(runDir,'environment.json')));
  console.log(label,JSON.stringify({status:result.status,preparation:record.report.preparation,target:record.report.target,packages:record.report.packages.length,detail:record.report.detail,diagnostics:record.diagnostics}));
  if(result.status!== (success?0:1)){
    for(const log of ['execution.stdout.log','execution.stderr.log','pull.stderr.log']){
      if(fs.existsSync(path.join(runDir,log)))console.error(fs.readFileSync(path.join(runDir,log),'utf8').slice(-14000));
    }
  }
  assert.equal(result.status,success?0:1);
  assert.equal(record.container_removed,true);
  assert.equal(record.timed_out,false);assert.equal(record.output_limit,false);
  assert.equal(record.diagnostics.length,0);
  assert.match(record.source_archive_sha256,/^[0-9a-f]{64}$/);
  return {directory:runDir,record};
}
const normal=run('normal',recipe,true);
assert.equal(normal.record.report.preparation,'succeeded');
assert.equal(normal.record.report.target,target==='ohif-build'?'not_observed':'succeeded');
assert(normal.record.report.packages.length>5);
if(target==='ohif-build'){
  assert(fs.statSync(path.join(normal.directory,'result/dist/index.html')).size>0);
  assert(JSON.parse(fs.readFileSync(path.join(normal.directory,'result/build-files.json'))).length>10);
  assert(normal.record.report.packages.some(p=>p.lock_integrity?.startsWith('sha512-')));
}else{
  const inference=JSON.parse(fs.readFileSync(path.join(normal.directory,'result/inference.json')));
  assert.equal(inference.device,'cpu');assert.equal(inference.torch,'2.8.0+cpu');assert.equal(inference.numpy,'2.3.3');
  assert.deepEqual(inference.shape,[1,1,64,64]);assert(inference.max_abs_error<=inference.atol);
  const failure=structuredClone(recipe);failure.command=['python','-c','raise FileNotFoundError("synthetic missing inference input")'];
  const failed=run('target-failed',failure,false);
  assert.equal(failed.record.report.preparation,'succeeded');assert.equal(failed.record.report.target,'failed');
  const retry=run('retry',recipe,true);
  const diff=JSON.parse(command(bin,['environment','compare',normal.directory,retry.directory]));
  assert.equal(diff.packages_comparable,true);assert.equal(diff.recipe_changed,false);assert.equal(diff.runner_changed,false);
  assert.deepEqual(diff.added_or_changed,[]);assert.deepEqual(diff.removed_or_changed,[]);
  fs.writeFileSync(path.join(output,'comparison.json'),JSON.stringify(diff,null,2));
}
const damaged=structuredClone(recipe);damaged.locks[0].sha256='0'.repeat(64);
const failed=run('preparation-failed',damaged,false);
assert.equal(failed.record.report.preparation,'failed');assert.equal(failed.record.report.target,'not_observed');
assert.equal(failed.record.report.steps.length,0);
const leftover=command('docker',['ps','-aq','--filter','name=eel-env-']);assert.equal(leftover,'');
fs.writeFileSync(path.join(output,'summary.json'),JSON.stringify({target,commit:recipe.source.commit,image:recipe.image,normal:normal.directory,preparationFailure:failed.directory,clinicallyValidated:false},null,2));
console.log('Pinned dependency preparation, phase separation, evidence and cleanup passed.');
