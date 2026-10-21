import { test } from "bun:test";
import assert from 'node:assert/strict';
import {processSample,startWorkerHealth} from './worker-health.ts';
import {startDashboard} from './dashboard.ts';

test('worker process CPU uses whole-host percentage and excludes reused PIDs',()=>{
  const old={at:'2026-01-01T00:00:00Z',receivedBytes:1000,sentBytes:1000,processes:[{pid:7,startedAt:'old',cpuSeconds:10}]};
  const current={at:'2026-01-01T00:01:00Z',receivedBytes:8500,sentBytes:16000,processes:[{pid:7,startedAt:'old',cpuSeconds:40},{pid:8,startedAt:'new',cpuSeconds:50}]};
  const value=processSample(current,old,2);
  assert.equal(value.processes[0].cpuPercent,25);assert.equal(value.processes[1].cpuPercent,null);
  assert.equal(value.netInKbit,1);assert.equal(value.netOutKbit,2);
});
test('worker reports independent service health without forwarding secrets or player IPs',async()=>{
  const server=await startWorkerHealth({port:0,allowlistSecret:'private-secret',collect:async()=>({at:new Date().toISOString(),processes:[],receivedBytes:0,sentBytes:0}),fetcher:async(url,options)=>{
    if(url.endsWith('/status')) {assert.equal(options.headers['x-allowlist-secret'],'private-secret');return new Response(JSON.stringify({entries:[{ip:'1.2.3.4'}],lastApply:{ok:true,enabled:true},ports:'8758-8760'}));}
    if(url.endsWith('/gameservers'))return new Response(JSON.stringify({servers:[{id:'world',expectedPlayers:['UID-private'],port:8760}],capacity:{running:1,pending:2,limit:38,cpu:{blocked:true,percent:85,limit:80,resume:65}}}));
    return new Response('{}',{status:503});
  }});
  try{
    const url=`http://127.0.0.1:${server.port}/health`;
    assert.equal((await fetch(url,{headers:{'x-forwarded-for':'1.2.3.4'}})).status,403);
    const body=await (await fetch(url)).json();
    assert.deepEqual(body.services,{deploy:true,allowlist:true,backend:false});
    assert.equal(body.hunts[0].expectedPlayers,1);assert.equal(body.firewall.addresses,1);
    assert.equal(body.capacity.running,1);assert.equal(body.capacity.limit,38);assert.equal(body.capacity.cpu.blocked,true);
    assert.ok(Number.isFinite(Date.parse(body.capacitySampleAt)));
    assert.ok(!JSON.stringify(body).includes('private'));assert.ok(!JSON.stringify(body).includes('1.2.3.4'));
  }finally{await server.stop();}
});
test('failed allocator probe exposes unknown capacity instead of a fresh zero',async()=>{
 const server=await startWorkerHealth({port:0,collect:async()=>({at:new Date().toISOString(),processes:[],receivedBytes:0,sentBytes:0}),fetcher:async()=>new Response('{}',{status:503})});
 try {
  const body=await(await fetch(`http://127.0.0.1:${server.port}/health`)).json();
  assert.equal(body.services.deploy,false);assert.equal(body.capacity,null);assert.equal(body.capacitySampleAt,null);
  assert.deepEqual(body.hunts,[]); // Legacy consumer compatibility; services.deploy guards this list.
 } finally {await server.stop();}
});
test('main dashboard exposes worker health only behind owner authentication and never sends owner key to worker',async()=>{
  const key='private-owner-key-for-test';
  const server=await startDashboard({key,port:0,backend:'http://127.0.0.1:61000',workerUrl:'http://127.0.0.1:61112',fetcher:async(url,options)=>{
    if(url.pathname==='/health'){assert.equal(options.headers,undefined);return new Response(JSON.stringify({at:new Date().toISOString(),services:{deploy:true},hunts:[],cpu:20}));}
    return new Response(JSON.stringify(url.pathname.endsWith('GetAllUsers')?{Users:[]}:{players:[],instances:[],playersOnline:0}));
  }});
  try{
    const url=`http://127.0.0.1:${server.port}/api/status`;
    assert.equal((await fetch(url)).status,401);
    const result=await(await fetch(url,{headers:{'x-dashboard-key':key}})).json();
    assert.equal(result.worker.online,true);assert.equal(result.worker.sample.cpu,20);
    assert.ok(!JSON.stringify(result).includes(key));
  }finally{await server.stop();}
});
test('main dashboard rejects remote worker URLs',async()=>{
  await assert.rejects(startDashboard({key:'owner-key-long-enough',port:0,backend:'http://127.0.0.1:61000',workerUrl:'https://example.com'}),/loopback tunnel/);
});

test('worker monitoring tolerates a missed poll, expires, and recovers',async()=>{
 const {monitorWorker}=await import('./dashboard-worker.ts');
 let now=Date.now(),fail=false;
 const monitor=await monitorWorker('http://127.0.0.1:61112','test',async()=>{if(fail)throw Error('timeout');return new Response(JSON.stringify({at:new Date(now).toISOString(),services:{deploy:true},hunts:[]}));},()=>now);
 try{fail=true;now+=6000;await monitor.poll();assert.equal(monitor.state.online,true);assert.equal(monitor.state.status,'delayed');now+=25000;assert.equal(monitor.state.online,false);fail=false;await monitor.poll();assert.equal(monitor.state.online,true);assert.equal(monitor.state.status,'online');}finally{monitor.close();}
});
