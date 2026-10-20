import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Keys, backend, saveState } from './keys.mjs';
import { launcherInvite, inviteMessage } from './invite.mjs';
const config={Mode:'Public',PublicHost:'game.example.com',Ports:{gateway:443},CertFingerprint:'a'.repeat(64),ServerName:'Test & Hunt'};
test('claims produce complete launcher invites and valid registration code characters',async()=>{
  for(let i=0;i<50;i++) {
    const f=setup();
    const service=new Keys({version:1,users:{}},async()=>{},f.api);
    const result=await service.run(user,true);
    assert.match(result.code,/^[A-Za-z0-9-]{4,64}$/);
    const invite=launcherInvite(config,result.code), parsed=new URL(invite);
    assert.equal(parsed.protocol,'dauntless-revived:');assert.equal(parsed.hostname,'join');
    assert.equal(parsed.searchParams.get('code'),result.code);
    assert.equal(parsed.searchParams.get('fp'),config.CertFingerprint);
    assert.equal(parsed.searchParams.get('name'),config.ServerName);
    assert.ok(inviteMessage(config,result.code).includes(invite));
  }
  assert.throws(()=>launcherInvite(config,'DR-invalid_code'));
  assert.throws(()=>launcherInvite({...config,CertFingerprint:'wrong'},'GOOD-CODE'));
});
const user = '123456789012345678';

test('private claims recover uncertain DMs and reuse one invite across repeated claims',async()=>{
  const f=setup(); const first=await f.service.run(user,true);
  f.state.users[user].delivery='reserved';
  const seen=[];
  await Promise.all([f.service.deliverPrivate(user,async code=>seen.push(code)),f.service.deliverPrivate(user,async code=>seen.push(code))]);
  assert.deepEqual(seen,[first.code,first.code]);assert.equal(f.count(),1);
  assert.equal(f.state.users[user].deliveryChannel,'ephemeral');
  f.rows.get(first.code).usesRemaining=0;
  assert.deepEqual(await f.service.deliverPrivate(user,()=>assert.fail('must not show redeemed invite')),{status:'redeemed'});
});

test('failed private response preserves the existing invite for retry',async()=>{
  const f=setup();
  await assert.rejects(f.service.deliverPrivate(user,async()=>{throw Error('response failed')}));
  assert.equal(f.state.users[user].delivery,'unsent');
  await f.service.deliverPrivate(user,async code=>assert.equal(code,'DR-test'));
  assert.equal(f.count(),1);
});
test('old Join invites explain account-key recovery without verifying an invite as a credential',async()=>{
  const f=setup(); f.api.identity=async()=>assert.fail('Join invite must never be sent as an account key');
  assert.deepEqual(await f.service.link(user,'dauntless-revived://join?v=2&code=OLD-CODE'),{status:'invite_not_key'});
  assert.equal(f.count(),0); assert.equal(f.links.size,0);
});
test('legacy short keys, backup files and Discord-formatted keys retain the original credential',async()=>{
  const f=setup(); const key='oldkey12'; const seen=[];
  f.api.identity=async input=>{seen.push(input);return {userId:'UID-account',username:'Slayer'};};
  for(const input of [key,` \`${key}\` `,`\`\`\`\n${key}\n\`\`\``,`Dauntless Revived account key\nUsername: Slayer\nKey: ${key}\n\nKeep this file private.`])
    assert.deepEqual(await f.service.link(user,input),{status:'linked'});
  assert.deepEqual(seen,[key,key,key,key]); assert.equal(f.count(),0);
});
test('ambiguous backups and malformed input never link; registration codes receive specific guidance',async()=>{
  const f=setup(); let verifies=0; f.api.identity=async()=>{verifies++;return null;};
  for(const input of ['Key: firstkey\nKey: secondkey','bad key value','x'.repeat(8193)])
    assert.deepEqual(await f.service.link(user,input),{status:'invalid_key_format'});
  assert.equal(verifies,0);
  assert.deepEqual(await f.service.link(user,'DR-old-registration'),{status:'invite_not_key'});
  assert.equal(f.links.size,0); assert.equal(f.count(),0);
});
function setup() {
  const state = {version: 1, users: {}};
  const rows = new Map();
  const links = new Map();
  let creates = 0;
  const api = {linkedAccount: async id => links.get(id), linkAccount: async (id, uid) => {
    if (links.has(id) && links.get(id).userId !== uid) return {status:'discord_already_linked'};
    if ([...links].some(([other, value]) => other !== id && value.userId === uid)) return {status:'account_already_linked'};
    links.set(id,{userId:uid}); return {status:'linked'};
  }, find: async code => rows.get(code), create: async code => {
    creates++; rows.set(code, {inviteCode: code, usesRemaining: 1, infiniteUses: false});
  }};
  return {state, rows, links, api, count: () => creates, service: new Keys(state, async () => {}, api, () => 'DR-test')};
}
test('concurrent claims and DM retries reuse one single-use code', async () => {
  const f = setup();
  const results = await Promise.all(Array.from({length: 20}, () => f.service.run(user, true)));
  assert.equal(f.count(), 1);
  assert.ok(results.every(r => r.code === 'DR-test' && r.status === 'ready'));
});
test('status distinguishes unclaimed, issued, redeemed and revoked without minting replacements', async () => {
  const f = setup();
  assert.equal((await f.service.run(user, false)).status, 'none');
  assert.equal(f.count(), 0);
  await f.service.run(user, true);
  f.rows.get('DR-test').usesRemaining = 0;
  assert.deepEqual(await f.service.run(user, false), {status: 'redeemed'});
  assert.deepEqual(await f.service.run(user, true), {status: 'redeemed'});
  f.rows.clear();
  assert.deepEqual(await f.service.run(user, true), {status: 'revoked'});
  assert.equal(f.count(), 1);
});
test('an uncertain backend response is recovered without issuing another code', async () => {
  const f = setup(); const create = f.api.create;
  f.api.create = async code => { await create(code); throw new Error('timeout after write'); };
  await assert.rejects(f.service.run(user, true));
  assert.equal((await f.service.run(user, true)).code, 'DR-test');
  assert.equal(f.count(), 1);
});
test('a failed state write never permits issuance on a subsequent request', async () => {
  const f = setup(); f.service.save = async () => { throw new Error('disk full'); };
  await assert.rejects(f.service.run(user, true));
  await assert.rejects(f.service.run(user, true));
  assert.equal(f.count(), 0);
  assert.deepEqual(f.state.users, {});
});
test('backend credentials cannot be sent to remote URLs or redirects', async () => {
  for (const url of ['https://example.com', 'http://example.com', 'http://127.0.0.1@evil.test', 'http://127.0.0.1/path'])
    assert.throws(() => backend(url, 'secret'));
  let request;
  const api = backend('http://127.0.0.1:61000', 'secret', async (url, options) => {
    request = {url: String(url), ...options}; return new Response('{"InviteCodes":[]}', {status: 200});
  });
  assert.equal(await api.find('DR-test'), undefined);
  assert.equal(request.redirect, 'error');
  assert.equal(request.headers['x-undaunted-user-api-key'], 'secret');
});

test('linking verifies account ownership, persists identity only and prevents conflicting links', async () => {
  const f = setup(); const secret = 'UUK_test-private-launcher-key'; let saved;
  f.api.identity = async key => key === secret ? {userId:'UID-account',username:'Slayer'} : null;
  f.service.save = async value => { saved = JSON.stringify(value); };
  assert.deepEqual(await f.service.link(user, 'invalid-private-key'), {status:'invalid_key'});
  assert.deepEqual(await f.service.link(user, secret), {status:'linked'});
  assert.equal(saved, undefined);
  assert.equal(f.links.get(user).userId, 'UID-account');
  assert.ok(!JSON.stringify(f.service.state).includes(secret));
  assert.deepEqual(await f.service.run(user, false), {status:'linked'});
  assert.deepEqual(await f.service.run(user, true), {status:'linked'});
  assert.equal(f.count(), 0);
  assert.deepEqual(await f.service.link('223456789012345678', secret), {status:'account_already_linked'});
  f.api.identity = async () => ({userId:'UID-other',username:'Other'});
  assert.deepEqual(await f.service.link(user, secret), {status:'discord_already_linked'});
});
test('failed link persistence leaves the account unlinked and retryable', async () => {
  const f = setup(); f.api.identity = async () => ({userId:'UID-account',username:'Slayer'});
  f.api.linkAccount = async () => { throw new Error('disk full'); };
  await assert.rejects(f.service.link(user, 'private-key-for-testing'), /disk full/);
  assert.equal(f.service.state.links, undefined);
});
test('legacy verified links migrate idempotently without issuing or changing keys', async () => {
  const f = setup();
  f.state.links = {[user]:{userId:'UID-account',username:'Slayer'}};
  await f.service.migrateLinks(); await f.service.migrateLinks();
  assert.equal(f.links.size,1);
  assert.equal(f.count(),0);
  assert.equal((await f.service.run(user,true)).status,'linked');
});
test('identity verification sends the supplied key only to loopback and rejects invalid keys', async () => {
  let sent; let status=200;
  const api = backend('http://127.0.0.1:61000','admin-secret',async (url, options) => {
    sent={url:String(url),...options}; return new Response(JSON.stringify({UserId:'UID-account',Username:'Slayer'}),{status});
  });
  assert.deepEqual(await api.identity('player-key'),{userId:'UID-account',username:'Slayer'});
  assert.equal(sent.url,'http://127.0.0.1:61000/undaunted/api/GetUserInfo');
  assert.equal(sent.headers['x-undaunted-user-api-key'],'player-key');
  assert.equal(sent.redirect,'error');
  status=401; assert.equal(await api.identity('bad-key'),null);
});

test('ephemeral recipients receive the same invite once by DM, without minting another',async()=>{
 const f=setup();await f.service.deliverPrivate(user,async()=>{});let sent=0;
 assert.equal((await f.service.deliver(user,async code=>{assert.equal(code,'DR-test');sent++;return {id:'message'};})).status,'sent');
 assert.equal((await f.service.deliver(user,async()=>{sent++;})).status,'already_sent');
 assert.equal(sent,1);assert.equal(f.count(),1);assert.equal(f.state.users[user].deliveryChannel,'dm');
});
test('ambiguous DM after ephemeral migration stays reserved and is reconciled',async()=>{
 const f=setup();await f.service.deliverPrivate(user,async()=>{});
 assert.equal((await f.service.deliver(user,async()=>{throw Error('network timeout');})).status,'delivery_uncertain');
 assert.equal(f.state.users[user].deliveryChannel,'dm');
 assert.equal((await f.service.deliver(user,async()=>{throw Error('must not send again');},async()=>({id:'existing',createdAt:'now'}))).status,'already_sent');
 assert.equal(f.count(),1);
});

test('blocked history lookup keeps the reserved invite and explains DM settings',async()=>{
  const f=setup(); await f.service.run(user,true);
  f.state.users[user].delivery='reserved';
  const result=await f.service.deliver(user,()=>assert.fail('must not duplicate'),async()=>{throw {code:50007}});
  assert.equal(result.status,'dm_disabled');
  assert.equal(f.state.users[user].delivery,'reserved');
  assert.equal(f.count(),1);
});


test('Windows transient state replacement retries atomically and preserves old state until success',async()=>{
  const fs=await import('node:fs/promises');const {tmpdir}=await import('node:os');const {join}=await import('node:path');
  const dir=await fs.mkdtemp(join(tmpdir(),'dr-key-state-'));const file=join(dir,'keys.json');
  try {
    await fs.writeFile(file,'old');let calls=0;const waits=[];
    await saveState(file,{version:1,users:{}},{platform:'win32',sleep:async ms=>waits.push(ms),replace:async(a,b)=>{
      assert.equal(await fs.readFile(file,'utf8'),'old');
      if(++calls<=3)throw Object.assign(Error('locked'),{code:['EPERM','EACCES','EBUSY'][calls-1]});
      await fs.rename(a,b);
    }});
    assert.equal(calls,4);assert.deepEqual(waits,[50,100,200]);
    assert.deepEqual(JSON.parse(await fs.readFile(file,'utf8')),{version:1,users:{}});
    for(const [platform,code,expected] of [['win32','EPERM',7],['win32','ENOSPC',1],['linux','EPERM',1]]){
      calls=0;const failure=Object.assign(Error('failed'),{code});
      await assert.rejects(saveState(file,{version:1,users:{pending:{}}},{platform,sleep:async()=>{},replace:async()=>{++calls;throw failure;}}),error=>error===failure);
      assert.equal(calls,expected);assert.deepEqual(JSON.parse(await fs.readFile(file,'utf8')),{version:1,users:{}});
    }
  } finally {await fs.rm(dir,{recursive:true,force:true});}
});

test('backend-issued invite survives final commit failure and restart without a second issuance',async()=>{
  const f=setup();let durable;let saves=0;
  f.service.save=async value=>{if(++saves===2)throw Error('rename failed');durable=JSON.stringify(value);};
  await assert.rejects(f.service.run(user,true));assert.equal(f.count(),1);
  const restarted=new Keys(JSON.parse(durable),async()=>{},f.api,()=>assert.fail('must reuse pending invite'));
  assert.deepEqual(await restarted.run(user,true),{status:'ready',code:'DR-test'});assert.equal(f.count(),1);
});
