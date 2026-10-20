import {RemoveTestDb} from './setup';
import {after,test} from 'node:test';
import assert from 'node:assert/strict';
import {createHash,randomBytes} from 'node:crypto';
import {GetDb} from '../src/db';
import {MakePlayer} from './helpers';
import {RememberAccountKey,RecoverAccountKey} from '../src/controllers/accountrecovery';
after(()=>RemoveTestDb(()=>GetDb().$client.close()));
test('expiry cleanup uses an index instead of scanning every receipt',()=>{
 const plan=GetDb().$client.prepare('EXPLAIN QUERY PLAN DELETE FROM inventorytransactions WHERE createdDate < ?').all('2026-01-01');
 assert.match(JSON.stringify(plan),/inventorytransactions_created_date/);
});
test('recovery encrypts keys, rejects stale keys and excludes administrators',async()=>{
 process.env.ACCOUNT_KEY_RECOVERY='1';
 process.env.AUTH_SIGNING_PRIVKEY_B64='test-only-recovery-secret';
 const {UserId}=await MakePlayer();const db=GetDb().$client,key='UUK_'+ 'ab'.repeat(24);
 db.prepare('INSERT INTO userapikeys(userId,keyHash) VALUES(?,?)').run(UserId,createHash('sha256').update(key).digest('hex'));
 RememberAccountKey(UserId,key);
 assert.equal(RecoverAccountKey(UserId),key);
 assert.ok(!JSON.stringify(db.prepare('SELECT * FROM accountkeyrecovery').all()).includes(key));
 db.prepare('UPDATE users SET isAdmin=1 WHERE userId=?').run(UserId);assert.equal(RecoverAccountKey(UserId),null);
 db.prepare('UPDATE users SET isAdmin=0 WHERE userId=?').run(UserId);
 db.prepare('UPDATE userapikeys SET keyHash=? WHERE userId=?').run('0'.repeat(64),UserId);assert.equal(RecoverAccountKey(UserId),null);
 delete process.env.ACCOUNT_KEY_RECOVERY;
});

test('recovery fingerprints preserve the authentication join for registered and rotated random keys',async()=>{
 process.env.ACCOUNT_KEY_RECOVERY='1';
 process.env.AUTH_SIGNING_PRIVKEY_B64='test-only-recovery-secret';
 try {
  for(const bytes of [24,32]) {
   const {UserId}=await MakePlayer();const key='UUK_'+randomBytes(bytes).toString('hex');
   const digest=createHash('sha256').update(key,'utf8').digest('hex');
   const db=GetDb().$client;
   db.prepare('INSERT INTO userapikeys(userId,keyHash) VALUES(?,?)').run(UserId,digest);
   RememberAccountKey(UserId,key);
   const row=db.prepare('SELECT keyHash,ciphertext FROM accountkeyrecovery WHERE userId=?').get(UserId) as {keyHash:string,ciphertext:string};
   assert.equal(row.keyHash,digest);
   assert.equal(RecoverAccountKey(UserId),key);
   RememberAccountKey(UserId,key);
   assert.deepEqual(db.prepare('SELECT keyHash,ciphertext FROM accountkeyrecovery WHERE userId=?').get(UserId),row);
  }
 } finally {delete process.env.ACCOUNT_KEY_RECOVERY;}
});