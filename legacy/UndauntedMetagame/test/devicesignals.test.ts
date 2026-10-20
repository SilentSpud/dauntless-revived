import {RemoveTestDb} from './setup';
import {test,after} from 'node:test';
import assert from 'node:assert/strict';
import {createHmac} from 'node:crypto';
import {GetDb} from '../src/db';
import {ReportDeviceSignals,SnapshotDeviceBan,DeviceSignalInfo} from '../src/controllers/devicesignals';
import {SetAccountBan} from '../src/controllers/moderation';

after(()=>RemoveTestDb(()=>GetDb().$client.close()));
const secret='ab'.repeat(32),uuid='12345678-1234-5678-9abc-123456789abc',tpm='12'.repeat(32);
process.env.DEVICE_SIGNAL_HMAC_KEY=secret;
function user(id:string){GetDb().$client.prepare('INSERT INTO users(userId,name,notes,isAdmin) VALUES (?,?,0,0)').run(id,id);return id;}

test('strict input validation, HMAC-only storage and replacement without missing-device buckets',()=>{
    const id=user('device-validation'),db=GetDb().$client;
    for(const body of [null,[],42,{accountId:'victim'},{smbiosUuid:''},{smbiosUuid:'0'.repeat(32)},{smbiosUuid:'00000000-0000-0000-0000-000000000000'},{smbiosUuid:'ffffffff-ffff-ffff-ffff-ffffffffffff'},{tpmEkSha256:'0'.repeat(64)},{tpmEkSha256:'f'.repeat(64)},{tpmEkSha256:123},{tpmEkSha256:'x'.repeat(64)}])assert.throws(()=>ReportDeviceSignals(id,body),/invalid_device_signals/);
    delete process.env.DEVICE_SIGNAL_HMAC_KEY;
    assert.throws(()=>ReportDeviceSignals(id,{}),/device_signals_unavailable/);
    process.env.DEVICE_SIGNAL_HMAC_KEY='ab';
    assert.throws(()=>ReportDeviceSignals(id,{}),/device_signals_unavailable/);
    process.env.DEVICE_SIGNAL_HMAC_KEY=secret;
    assert.deepEqual(ReportDeviceSignals(id,{smbiosUuid:uuid.toUpperCase(),tpmEkSha256:tpm}),{blocked:false});
    const rows=db.prepare('SELECT * FROM account_device_signals WHERE account_id=?').all(id) as any[];
    assert.equal(rows.length,2);
    assert.ok(!JSON.stringify(rows).includes(uuid));assert.ok(!JSON.stringify(rows).includes(tpm));
    for(const row of rows){
        const value=row.kind==='smbiosUuid'?uuid:tpm;
        assert.equal(row.signal_hash,createHmac('sha256',Buffer.from(secret,'hex')).update(row.kind+'\0'+value).digest('hex'));
        assert.notEqual(row.signal_hash,createHmac('sha256',Buffer.from(secret,'hex')).update('other\0'+value).digest('hex'));
    }
    const info=DeviceSignalInfo(id);assert.equal(info.verified,false);assert.equal(info.claimed,true);assert.equal(info.signals[0].hash.length,12);
    ReportDeviceSignals(id,{smbiosUuid:uuid});assert.equal(DeviceSignalInfo(id).signals.length,1);
    ReportDeviceSignals(id,{});assert.deepEqual(DeviceSignalInfo(id).signals,[]);
    assert.throws(()=>SnapshotDeviceBan(id,true),/device_not_observed/);
    assert.throws(()=>ReportDeviceSignals('does-not-exist',{smbiosUuid:uuid}),/account_not_found/);
});

test('only explicit snapshots block matching claims; expiry, revocation, missing claims and replacement are safe',()=>{
    const source=user('device-source'),other=user('device-other'),db=GetDb().$client;
    ReportDeviceSignals(source,{smbiosUuid:uuid,tpmEkSha256:tpm});
    SetAccountBan(source,'manual verified evidence',true,null,'owner',168);
    assert.equal(ReportDeviceSignals(other,{smbiosUuid:uuid}).blocked,false);
    db.transaction(()=>SnapshotDeviceBan(source,true))();
    assert.equal(DeviceSignalInfo(source).enabled,true);
    assert.deepEqual(ReportDeviceSignals(other,{smbiosUuid:uuid}),{blocked:true,reason:'manual verified evidence'});
    assert.equal(ReportDeviceSignals(other,{tpmEkSha256:tpm}).blocked,true);
    assert.equal(ReportDeviceSignals(other,{}).blocked,false);
    assert.equal(ReportDeviceSignals(other,{tpmEkSha256:'34'.repeat(32)}).blocked,false);
    ReportDeviceSignals(source,{}); // Later claims must not erase the moderator's snapshot.
    assert.equal(ReportDeviceSignals(other,{smbiosUuid:uuid}).blocked,true);
    db.prepare('UPDATE account_bans SET expires_at=? WHERE account_id=?').run(Date.now()-1,source);
    assert.equal(ReportDeviceSignals(other,{smbiosUuid:uuid}).blocked,false);
    SetAccountBan(source,'manual verified evidence',true,null,'owner',168);
    SetAccountBan(source,'appeal',false,null,'owner');
    assert.equal(ReportDeviceSignals(other,{smbiosUuid:uuid}).blocked,false);
    SnapshotDeviceBan(source,false);assert.equal(DeviceSignalInfo(source).enabled,false);
});

test('stale and future claims cannot be snapshotted; failure preserves the prior snapshot',()=>{
    const id=user('device-stale'),db=GetDb().$client;
    ReportDeviceSignals(id,{smbiosUuid:uuid});SnapshotDeviceBan(id,true);
    db.prepare('UPDATE account_device_signals SET observed_at=? WHERE account_id=?').run(Date.now()-86400000,id);
    assert.throws(()=>SnapshotDeviceBan(id,true),/device_not_observed/);
    assert.equal(DeviceSignalInfo(id).enabled,true);
    db.prepare('UPDATE account_device_signals SET observed_at=? WHERE account_id=?').run(Date.now()+86400000,id);
    assert.throws(()=>SnapshotDeviceBan(id,true),/device_not_observed/);
    SnapshotDeviceBan(id,false);assert.equal(DeviceSignalInfo(id).enabled,false);
});
