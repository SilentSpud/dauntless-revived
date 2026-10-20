import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Keys} from './keys.mjs';
const user = '123456789012345678';
function fixture(state = {version:1,users:{}}) {
  let saved, sends = 0;
  const rows = new Map();
  const api = {linkedAccount:async()=>null,find:async code=>rows.get(code),create:async code=>rows.set(code,{usesRemaining:1,infiniteUses:false})};
  const save = async value=>{saved=structuredClone(value);};
  return {service:new Keys(state,save,api),api,save,state,rows,get saved(){return saved;},send:async()=>{sends++;},get sends(){return sends;}};
}
test('concurrent claims and restart send exactly one DM',async()=>{
  const f=fixture();
  const results=await Promise.all(Array.from({length:10},()=>f.service.deliver(user,f.send)));
  assert.equal(f.sends,1); assert.equal(results.filter(r=>r.status==='sent').length,1);
  const restarted=new Keys(f.saved,f.save,f.api);
  assert.equal((await restarted.deliver(user,f.send)).status,'already_sent'); assert.equal(f.sends,1);
});
test('ambiguous Discord result is never retried, even after restart',async()=>{
  const f=fixture();
  assert.equal((await f.service.deliver(user,async()=>{throw new Error('timeout');})).status,'delivery_uncertain');
  assert.equal((await new Keys(f.saved,f.save,f.api).deliver(user,f.send)).status,'delivery_uncertain');
  assert.equal(f.sends,0);
});
test('explicit DM rejection retries the same code only',async()=>{
  const f=fixture();
  assert.equal((await f.service.deliver(user,async()=>{throw Object.assign(new Error(),{code:50007});})).status,'dm_disabled');
  const code=f.state.users[user].code;
  assert.equal((await f.service.deliver(user,f.send)).status,'sent');
  assert.equal(f.state.users[user].code,code); assert.equal(f.rows.size,1);
});
test('legacy issued codes are not resent',async()=>{
  const f=fixture({version:1,users:{[user]:{code:'DR-old',pending:false}}});
  f.rows.set('DR-old',{usesRemaining:1,infiniteUses:false});
  assert.equal((await f.service.deliver(user,f.send)).status,'already_sent'); assert.equal(f.sends,0);
});
test('failed durable reservation prevents a DM',async()=>{
  const f=fixture(); await f.service.run(user,true);
  f.service.save=async()=>{throw new Error('disk full');};
  await assert.rejects(f.service.deliver(user,f.send)); assert.equal(f.sends,0);
});

test('reserved delivery reconciles existing message without duplicate',async()=>{
  const f=fixture(); await f.service.deliver(user,async()=>{throw new Error('timeout');});
  assert.equal((await f.service.deliver(user,f.send,async()=>({id:'123',createdAt:'2026-10-04T04:00:00Z'}))).status,'already_sent');
  assert.equal(f.sends,0);assert.equal(f.state.users[user].messageId,'123');
});
test('complete history proving no message retries same reserved code',async()=>{
  const f=fixture();await f.service.deliver(user,async()=>{throw new Error('timeout');});const code=f.state.users[user].code;
  assert.equal((await f.service.deliver(user,f.send,async()=>null)).status,'sent');
  assert.equal(f.sends,1);assert.equal(f.state.users[user].code,code);assert.equal(f.rows.size,1);
});
test('incomplete history cannot authorize resend',async()=>{
  const f=fixture();await f.service.deliver(user,async()=>{throw new Error('timeout');});
  assert.equal((await f.service.deliver(user,f.send,async()=>undefined)).status,'delivery_uncertain');assert.equal(f.sends,0);
});
test('no mutual guild rejection preserves invite for retry after joining',async()=>{
  const f=fixture();assert.equal((await f.service.deliver(user,async()=>{throw Object.assign(new Error(),{code:50278});})).status,'no_mutual_guild');
  const code=f.state.users[user].code;assert.equal(f.state.users[user].delivery,'unsent');
  assert.equal((await f.service.deliver(user,f.send)).status,'sent');assert.equal(f.state.users[user].code,code);assert.equal(f.rows.size,1);
});
