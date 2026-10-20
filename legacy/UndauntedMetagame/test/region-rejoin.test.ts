import {RemoveTestDb} from './setup';
import './matchmakingenv';
import {test,after} from 'node:test';
import assert from 'node:assert/strict';
import {GetDb} from '../src/db';
import {SetRegionReader} from '../src/controllers/huntregion';
import {HandlePlayerMatchmaking,CheckAndUpdateQueueStatus,ResetMatchmakingForTests} from '../src/controllers/matchmaking';
after(()=>RemoveTestDb(()=>GetDb().$client.close()));
test('changing from EU to US cannot reuse a ready solo EU candidate',async()=>{
 const saved=globalThis.fetch;let region:'main'|'us'='main';const calls:any[]=[];
 SetRegionReader(()=>region);
 globalThis.fetch=(async(_url:any,init:any)=>{const b=JSON.parse(init.body);calls.push(b);return new Response(JSON.stringify({host:b.Region==='us'?'us':'main',port:39000}),{status:200});}) as typeof fetch;
 try {
  await HandlePlayerMatchmaking('ISLAND','','Hunt_Test_A','UID-region-test',true);
  await new Promise(r=>setTimeout(r,20));
  assert.equal((await CheckAndUpdateQueueStatus('UID-region-test'))?.Host,'main');
  region='us';
  await HandlePlayerMatchmaking('ISLAND','','Hunt_Test_A','UID-region-test',true);
  await new Promise(r=>setTimeout(r,20));
  assert.equal((await CheckAndUpdateQueueStatus('UID-region-test'))?.Host,'us');
  assert.equal(calls.length,2);
  await HandlePlayerMatchmaking('SHARED','','ShatteredIsles_TrainingDojo','UID-region-test');
  assert.equal(calls.at(-1).Region,'us');
 }finally{globalThis.fetch=saved;SetRegionReader(()=>'main');ResetMatchmakingForTests();}
});
