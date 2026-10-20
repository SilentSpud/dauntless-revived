import assert from 'node:assert/strict';
import {test} from 'node:test';
import {HuntLifetime,HuntLifetimeFromEnv} from '../src/controllers/huntlifetime';
const hunt={id:'h',isRamsgate:false,isTrainingDojo:false,startTime:new Date(0),readyAt:0};
test('the documented ceiling retires an instance whatever its occupancy, and never a persistent world',()=>{
 const policy=new HuntLifetime(120000,10000,0);
 assert.equal(policy.reason(hunt,4,119999),undefined);
 assert.equal(policy.reason(hunt,4,120000),'max-age');
 assert.equal(policy.reason(hunt,undefined,120000),'max-age');
 assert.equal(policy.reason({...hunt,isRamsgate:true},0,999999),undefined);
 assert.equal(policy.reason({...hunt,isTrainingDojo:true},0,999999),undefined);
});
test('empty cleanup requires continuous fresh zero readings and resets on arrivals or missing data',()=>{
 const policy=new HuntLifetime(120000,10000,0);
 assert.equal(policy.reason(hunt,0,10000),undefined);
 assert.equal(policy.reason(hunt,1,19000),undefined);
 assert.equal(policy.reason(hunt,0,20000),undefined);
 assert.equal(policy.reason(hunt,undefined,29000),undefined);
 assert.equal(policy.reason(hunt,0,30000),undefined);
 assert.equal(policy.reason(hunt,0,39999),undefined);
 assert.equal(policy.reason(hunt,0,40000),'empty');
 policy.prune(new Set());
 assert.equal(policy.reason(hunt,0,41000),undefined);
});
test('invalid lifetime configuration fails instead of silently disabling cleanup',()=>{
 assert.throws(()=>HuntLifetimeFromEnv({HUNT_MAX_AGE_MINUTES:'0'}));
 assert.throws(()=>HuntLifetimeFromEnv({HUNT_EMPTY_MINUTES:'NaN'}));
 assert.throws(()=>HuntLifetimeFromEnv({HUNT_SILENT_MINUTES:'0'}));
 assert.throws(()=>HuntLifetimeFromEnv({HUNT_UNREADY_GRACE_MINUTES:'1441'}));
});
test('default cleanup needs two minutes of fresh zero counts after readiness',()=>{
 const policy=HuntLifetimeFromEnv({});
 assert.equal(policy.reason({...hunt,readyAt:undefined},0,0),undefined);
 assert.equal(policy.reason({...hunt,readyAt:undefined},0,120000),undefined);
 assert.equal(policy.reason(hunt,0,120000),undefined);
 assert.equal(policy.reason(hunt,0,180000),undefined);
 assert.equal(policy.reason(hunt,0,240000),'empty');
});
test('missed observations reset the empty interval instead of treating a gap as empty',()=>{
 const policy=HuntLifetimeFromEnv({});
 assert.equal(policy.reason(hunt,0,0),undefined);
 assert.equal(policy.reason(hunt,0,120000),undefined);
 assert.equal(policy.reason(hunt,0,180000),undefined);
 assert.equal(policy.reason(hunt,0,240000),'empty');
});
test('an instance that never reported ready is judged on occupancy once its grace has passed',()=>{
 const policy=HuntLifetimeFromEnv({});
 const stuck={...hunt,readyAt:undefined};
 assert.equal(policy.reason(stuck,0,299999),undefined);
 assert.equal(policy.reason(stuck,0,300000),undefined);
 assert.equal(policy.reason(stuck,0,360000),undefined);
 assert.equal(policy.reason(stuck,0,420000),'empty');
});
test('an instance that never reported ready but has players is left alone',()=>{
 const policy=HuntLifetimeFromEnv({});
 const stuck={...hunt,readyAt:undefined};
 for(let at=300000;at<=600000;at+=60000)assert.equal(policy.reason(stuck,2,at),undefined);
});
test('continuous silence retires an instance while telemetry works elsewhere on the host',()=>{
 const policy=HuntLifetimeFromEnv({});
 const neighbour={...hunt,id:'n'};
 let last;
 for(let at=0;at<=900000;at+=60000){
  assert.equal(policy.reason(neighbour,3,at),undefined);
  last=policy.reason(hunt,undefined,at);
  if(at<900000)assert.equal(last,undefined);
 }
 assert.equal(last,'silent');
});
test('silence proves nothing while no instance on the host reports at all',()=>{
 const policy=HuntLifetimeFromEnv({});
 for(let at=0;at<=1800000;at+=60000)assert.equal(policy.reason(hunt,undefined,at),undefined);
});
test('a reading of any kind interrupts the silent interval',()=>{
 const policy=HuntLifetimeFromEnv({});
 const neighbour={...hunt,id:'n'};
 for(let at=0;at<=840000;at+=60000){
  policy.reason(neighbour,3,at);
  assert.equal(policy.reason(hunt,undefined,at),undefined);
 }
 assert.equal(policy.reason(hunt,1,870000),undefined);
 assert.equal(policy.reason(hunt,undefined,900000),undefined);
});
