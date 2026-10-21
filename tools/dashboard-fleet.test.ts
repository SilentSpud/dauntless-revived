import { test } from "bun:test";
import assert from 'node:assert/strict';
import { fleetSummary } from './dashboard-fleet.ts';
const now=Date.now(), at=new Date(now).toISOString();
const main={at,cpu:50,logicalCpus:5,ramUsedMB:6000,ramTotalMB:12000,performance:{at,stale:false,processes:[{role:'ramsgate'},{role:'hunt'},{role:'tutorial'}]}};
const worker={configured:true,online:true,sample:{at,cpu:5,logicalCpus:2,ramUsedMB:2000,ramTotalMB:8000,hunts:[{}]}};
test('fleet totals sum RAM and hunts and label the unweighted CPU mean',()=>{
  const f=fleetSummary(main,worker,null,now);
  assert.deepEqual(f.rows.map(r=>r.hunts),[2,1]);
  assert.equal(f.totals.meanCpu,27.5); assert.equal(f.totals.hunts,3);
  assert.equal(f.totals.ramUsedMB,8000); assert.equal(f.totals.ramTotalMB,20000); assert.equal(f.totals.ramPercent,40);
});
test('offline or stale server cannot produce a misleading fleet total',()=>{
  const f=fleetSummary(main,{...worker,online:false},null,now);
  assert.equal(f.totals.meanCpu,null);assert.equal(f.totals.hunts,null);assert.equal(f.totals.ramUsedMB,null);
  assert.equal(fleetSummary(main,worker,null,now+31000).totals.online,0);
  assert.equal(fleetSummary({...main,performance:null},worker,null,now).totals.hunts,null);
});
test('three-server fleet includes AUS and excludes persistent worlds from worker hunt totals',()=>{
 const aus={...worker,sample:{...worker.sample,cpu:20,hunts:[{kind:'hunt'},{kind:'city'},{kind:'dojo'}]}};
 const f=fleetSummary(main,worker,null,now,aus);
 assert.equal(f.rows.length,3);assert.equal(f.totals.meanCpu,25);
 assert.equal(f.totals.hunts,4);assert.equal(f.totals.online,3);
 assert.equal(fleetSummary(main,worker,null,now,{...aus,online:false}).totals.meanCpu,null);
});
test('fresh allocator counts repair missing performance samples and include all five hosts',()=>{
 const capacity={running:4,pending:2,limit:10,cpu:{blocked:true,percent:70,limit:80,resume:65}};
 const primary={...main,performance:null,capacity,capacitySampleAt:at,diskFreeGB:12,hostUptimeSeconds:100};
 const f=fleetSummary(primary,worker,null,now,worker,worker,worker);
 assert.equal(f.rows.length,5);assert.equal(f.rows[0].hunts,4);assert.equal(f.totals.hunts,8);
 assert.deepEqual(f.rows[0].capacity,capacity);assert.equal(f.rows[0].huntSampleAt,at);
 assert.equal(f.rows[0].diskFreeGB,12);assert.equal(f.rows[0].hostUptimeSeconds,100);
 assert.equal(f.rows[2].name,'Server #3 · US (NA)');assert.equal(f.rows[4].name,'Server #5 · EU overflow');
});
test('stale or invalid capacity falls back to timestamped process data, never becomes zero',()=>{
 const capacity={running:4,pending:0,limit:10};
 for(const change of [{capacitySampleAt:new Date(now-31000).toISOString()},{capacitySampleAt:'invalid'},{capacity:{...capacity,running:-1}},{capacity:{...capacity,running:'4'}},{capacity:{...capacity,pending:0.5}}]) {
  const sample={...main,capacity,capacitySampleAt:at,...change};
  assert.equal(fleetSummary(sample,worker,null,now).rows[0].hunts,2);
  assert.equal(fleetSummary({...sample,performance:null},worker,null,now).rows[0].hunts,null);
 }
 assert.equal(fleetSummary({...main,capacity,capacitySampleAt:at},worker,'failed',now).rows[0].capacity,null);
});
test('worker allocator metadata overrides list count; failed deploy is unknown for game hosts',()=>{
 const node={...worker,sample:{...worker.sample,services:{deploy:true},capacity:{running:8,pending:1,limit:38,cpu:{blocked:'yes',percent:50}},capacitySampleAt:at}};
 let f=fleetSummary(main,worker,null,now,node);
 assert.equal(f.rows[2].hunts,8);assert.equal(f.rows[2].capacity.cpu,null);
 const failed={...worker,sample:{...worker.sample,hunts:[],services:{deploy:false}}};
 f=fleetSummary(main,failed,null,now,failed);
 assert.equal(f.rows[1].hunts,0);assert.equal(f.rows[2].hunts,null);assert.equal(f.totals.hunts,null);
});
