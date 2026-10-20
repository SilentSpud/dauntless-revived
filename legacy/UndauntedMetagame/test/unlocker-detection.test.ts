import {RemoveTestDb} from './setup';
import './authenv';
import {test,after} from 'node:test';
import assert from 'node:assert/strict';
import {GetDb} from '../src/db';
import {users} from '../src/db/schema';
import {SignMetagameJWTForUid,ValidateMetagameJWTAndGetPayload} from '../src/controllers/auth';
import {BanFor,ModerationInfo,ModerationLog,SetAccountBan} from '../src/controllers/moderation';
import {EnforceKnownUnlockerMutation as enforce} from '../src/controllers/unlocker-detection';
after(()=>RemoveTestDb(()=>GetDb().$client.close()));
const source='nova-unlocker:platinum:user-request';
const mutation={source,addStackedItems:[{catalogId:'CURRENCY_PLATINUM',quantity:10}]};
test('exact nonempty mutation suspends only authenticated actor for seven days and records bounded evidence',()=>{
 const db=GetDb();db.insert(users).values([{userId:'UID-attacker',name:'Attacker',notes:0},{userId:'UID-victim',name:'Victim',notes:0}]).run();
 const token=SignMetagameJWTForUid('UID-attacker');
 const before=Date.now();assert.deepEqual(enforce({userId:'UID-attacker'},{...mutation,accountId:'UID-victim',characterId:'victim-character'}),{blocked:true,suspended:true});
 assert.throws(()=>ValidateMetagameJWTAndGetPayload(token));
 const ban=ModerationInfo('UID-attacker').ban as any;
 assert.ok(ban.expires_at>=before+7*86400000&&ban.expires_at<=Date.now()+7*86400000);assert.equal(ban.address,null);
 assert.equal(BanFor('UID-victim'),undefined);assert.match(ban.reason,/addStackedItems=1/);
 assert.ok(ModerationLog('events').rows.some((r:any)=>r.account_id==='UID-attacker'&&r.actor==='anticheat:nova-marker'));
 const expiry=ban.expires_at;enforce({userId:'UID-attacker'},mutation);assert.equal((ModerationInfo('UID-attacker').ban as any).expires_at,expiry);
});
test('generic reads, empty probes, marker lookalikes, game servers and missing auth are never detections',()=>{
 for(const [auth,body] of [[undefined,mutation],[{userId:'UID-clean',IsGameserver:true},mutation],[{userId:'UID-clean'},{source}],
  [{userId:'UID-clean'},{source:'local-editor:empty-connection-check',addStackedItems:[]}],
  [{userId:'UID-clean'},{...mutation,source:source+'-lookalike'}], [{userId:'UID-clean'},{...mutation,source:'legitimate-game'}]])
   assert.deepEqual(enforce(auth as any,body),{blocked:false,suspended:false});
});
test('all supplied exact mutation signatures detected, existing permanent bans preserved and admins blocked without ban',()=>{
 const db=GetDb();let i=0;
 for(const source of ['local-editor:catalog-cosmetics:user-request','local-editor:user-request','local-editor:equipment:user-request','local-editor:cells-five-each:user-request','local-editor:unlock-equipment:user-request','local-editor:tonics:user-request']){
  const id=`UID-signature-${i++}`;db.insert(users).values({userId:id,name:id,notes:0}).run();
  assert.equal(enforce({userId:id},{source,saveInstancedItems:[{}]}).suspended,true);
 }
 db.insert(users).values([{userId:'UID-permanent',name:'Permanent',notes:0},{userId:'UID-protected',name:'Protected',notes:0,isAdmin:true}]).run();
 SetAccountBan('UID-permanent','Prior decision',true,null,'owner');enforce({userId:'UID-permanent'},mutation);
 assert.equal((ModerationInfo('UID-permanent').ban as any).expires_at,null);
 assert.deepEqual(enforce({userId:'UID-protected'},mutation),{blocked:true,suspended:false});assert.equal(BanFor('UID-protected'),undefined);
});
