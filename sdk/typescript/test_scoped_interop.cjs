'use strict';
const assert = require('node:assert/strict');
const {spawn,spawnSync} = require('node:child_process');
const {mkdtempSync,rmSync} = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const readline = require('node:readline');
const {Client} = require(process.env.RIT_TS_CLIENT);
const dir=mkdtempSync(path.join(os.tmpdir(),'rit-ts-'));
const init=spawnSync('git',['init','-q',dir]);assert.equal(init.status,0);
const child=spawn(process.env.RIT_PROTOCOL_BINARY,['--repo',dir],{stdio:['pipe','pipe','inherit']});
const queue=[];readline.createInterface({input:child.stdout}).on('line',line=>{const waiter=queue.shift();if(!waiter)throw new Error('unexpected response');waiter.resolve(JSON.parse(line));});
child.on('error',err=>{for(const pending of queue.splice(0))pending.reject(err);});
const transport=request=>new Promise((resolve,reject)=>{queue.push({resolve,reject});child.stdin.write(JSON.stringify(request)+'\n');});
(async()=>{const client=new Client(transport);
  const list=await client.request('tools/list');assert.equal(list.tools.length,15);
  for (const [name,field] of [["workspace.list","reconciliation"],["checkpoint.list","content_checkpoints"],["submission.list","submissions"],["candidate.list","candidates"],["workspace.operations","operations"],["checkpoint.restores","restores"],["checkpoint.extractions","extractions"],["submission.reconcile","submissions"]]) {
    const data=(await client.request("tools/call",{name,arguments:{}})).structuredContent;assert.ok(field in data);assert.equal(data.authenticated,false);
  }
  const result=await client.request('tools/call',{name:'repository.inspect',arguments:{}});assert.equal(result.structuredContent.inspection.object_format,'sha1');
  await assert.rejects(client.request('tools/call',{name:'repository.inspect',arguments:{path:'..'}}));
  child.stdin.end();const status=await new Promise(resolve=>child.on('close',resolve));assert.equal(status,0);console.log('TypeScript scoped repository read and path denial passed');
})().catch(e=>{console.error(e);child.kill();process.exitCode=1;}).finally(()=>rmSync(dir,{recursive:true,force:true}));
