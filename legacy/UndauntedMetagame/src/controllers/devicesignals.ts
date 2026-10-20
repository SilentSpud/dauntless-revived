import {createHmac} from 'node:crypto';
import {GetDb} from '../db';

type Signal = {kind:string,signal_hash:string,observed_at:number};
const FreshFor=24*60*60*1000;

// These are client claims, not attestation. Never use this evidence to auto-ban.
export function ReportDeviceSignals(accountId:string,body:unknown):{blocked:boolean,reason?:string}{
    if(!body || typeof body!=='object' || Array.isArray(body) || Object.keys(body).some(k=>k!=='smbiosUuid' && k!=='tpmEkSha256'))throw Error('invalid_device_signals');
    const secret=process.env.DEVICE_SIGNAL_HMAC_KEY;
    if(!secret || !/^[0-9a-f]{64,}$/i.test(secret) || secret.length%2!==0)throw Error('device_signals_unavailable');
    const signals:Signal[]=[],now=Date.now();
    for(const [kind,value] of Object.entries(body)){
        if(typeof value!=='string')throw Error('invalid_device_signals');
        const normalized=value.toLowerCase();
        const valid=kind==='smbiosUuid' ? /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(normalized) : /^[0-9a-f]{64}$/.test(normalized);
        if(!valid || /^(0+|f+)$/.test(normalized.replaceAll('-','')) || normalized==='03000200-0400-0500-0006-000700080009')throw Error('invalid_device_signals');
        signals.push({kind,signal_hash:createHmac('sha256',Buffer.from(secret,'hex')).update(kind+'\0'+normalized).digest('hex'),observed_at:now});
    }
    const db=GetDb().$client;
    return db.transaction(()=>{
        if(!db.prepare('SELECT 1 FROM users WHERE userId=?').get(accountId))throw Error('account_not_found');
        db.prepare('DELETE FROM account_device_signals WHERE account_id=?').run(accountId);
        const insert=db.prepare('INSERT INTO account_device_signals(account_id,kind,signal_hash,observed_at) VALUES (?,?,?,?)');
        for(const s of signals)insert.run(accountId,s.kind,s.signal_hash,now);
        const match=db.prepare(`SELECT b.reason FROM device_ban_signals d JOIN account_bans b ON b.account_id=d.account_id
            WHERE d.kind=? AND d.signal_hash=? AND b.active=1 AND (b.expires_at IS NULL OR b.expires_at>?) LIMIT 1`);
        for(const s of signals){
            const ban=match.get(s.kind,s.signal_hash,now) as {reason:string}|undefined;
            if(ban)return {blocked:true,reason:ban.reason};
        }
        return {blocked:false};
    })();
}

// Caller includes this in the account-ban transaction; snapshots never change on login.
export function SnapshotDeviceBan(accountId:string,enabled:boolean,now=Date.now()){
    const db=GetDb().$client;
    const signals=enabled ? db.prepare('SELECT kind,signal_hash,observed_at FROM account_device_signals WHERE account_id=? AND observed_at>? AND observed_at<=?').all(accountId,now-FreshFor,now) as Signal[] : [];
    if(enabled && !signals.length)throw Error('device_not_observed');
    db.prepare('DELETE FROM device_ban_signals WHERE account_id=?').run(accountId);
    const insert=db.prepare('INSERT INTO device_ban_signals(account_id,kind,signal_hash) VALUES (?,?,?)');
    for(const s of signals)insert.run(accountId,s.kind,s.signal_hash);
}

export function DeviceSignalInfo(accountId:string){
    const db=GetDb().$client;
    const rows=db.prepare('SELECT kind,signal_hash,observed_at FROM account_device_signals WHERE account_id=? ORDER BY kind').all(accountId) as Signal[];
    return {claimed:true,verified:false,enabled:!!db.prepare('SELECT 1 FROM device_ban_signals WHERE account_id=? LIMIT 1').get(accountId),
        signals:rows.map(s=>({kind:s.kind,hash:s.signal_hash.slice(0,12),observedAt:s.observed_at,fresh:s.observed_at>Date.now()-FreshFor && s.observed_at<=Date.now()}))};
}
