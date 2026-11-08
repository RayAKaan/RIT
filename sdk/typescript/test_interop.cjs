'use strict';
// Requires a compiled client and the built local protocol binary, supplied by environment.
const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const readline = require('node:readline');
const {Client} = require(process.env.RIT_TS_CLIENT);
const child = spawn(process.env.RIT_PROTOCOL_BINARY, [], {stdio:['pipe','pipe','inherit']});
const lines = readline.createInterface({input:child.stdout});
const queue=[];
lines.on('line',line=>{const waiter=queue.shift(); if (!waiter) throw new Error('unexpected response'); waiter.resolve(JSON.parse(line));});
child.on('error',e=>{for(const waiter of queue.splice(0))waiter.reject(e);});
const transport = request => new Promise((resolve,reject)=>{queue.push({resolve,reject}); child.stdin.write(JSON.stringify(request)+'\n');});
(async()=>{
  const client=new Client(transport);
  assert.equal((await client.initialize()).protocolVersion,'2025-03-26');
  assert.deepEqual((await client.request('tools/list')).tools,[]);
  await assert.rejects(client.request('tools/call',{name:'publish'}),/unavailable or unauthorized/);
  child.stdin.end();
  const status=await new Promise(resolve=>child.on('close',resolve));
  assert.equal(status,0); console.log('TypeScript stdio discovery/denial interoperability passed');
})().catch(e=>{console.error(e); child.kill();process.exitCode=1;});
