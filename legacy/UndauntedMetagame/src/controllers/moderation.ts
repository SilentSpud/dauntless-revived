import {GetDb} from '../db';
import {DeviceSignalInfo,SnapshotDeviceBan} from './devicesignals';
import {ClientAddressOf, IsLoopbackAddress, NormalizeAddress} from '../middleware/RequestOrigin';
import type {Request, Response} from 'express';

const playingAddresses=new Map<string,{address:string,last_seen:number}>();
export function ExpirePlayingAddresses(now=Date.now()) {
    for(const [id,row] of playingAddresses) if(now-row.last_seen>=90000) playingAddresses.delete(id);
}
setInterval(ExpirePlayingAddresses,10000).unref();
export function EndPlayingAddress(accountId:string){playingAddresses.delete(accountId);}
export function RecordPlayingAddress(req:Request,accountId:string,now=Date.now()) {
    const address=ClientAddressOf(req);
    if(NormalizeAddress(address) && !IsLoopbackAddress(address)) playingAddresses.set(accountId,{address,last_seen:now});
}
export function BanFor(accountId?: string, address?: string): {reason:string}|undefined {
    return GetDb().$client.prepare('SELECT reason FROM account_bans WHERE active=1 AND (expires_at IS NULL OR expires_at>?) AND (account_id=? OR address=?) LIMIT 1')
        .get(Date.now(), accountId ?? '', address ?? '') as {reason:string}|undefined;
}
export function CheckPlayerAccess(req:Request,res:Response,accountId?:string):boolean {
    const address=ClientAddressOf(req);
    const publicAddress=NormalizeAddress(address) && !IsLoopbackAddress(address) ? address : undefined;
    const ban=BanFor(accountId,publicAddress);
    if(ban){res.status(403).json({error:'account_banned',reason:ban.reason});return false;}
    return true;
}
export function ModerationInfo(accountId:string){
    const db=GetDb().$client;
    ExpirePlayingAddresses();
    const user=db.prepare('SELECT name FROM users WHERE userId=?').get(accountId) as {name:string}|undefined;
    return {username:user?.name ?? null,ban:db.prepare('SELECT reason,address,CASE WHEN expires_at<=? THEN 0 ELSE active END AS active,updated_at,expires_at FROM account_bans WHERE account_id=?').get(Date.now(),accountId) ?? null,
        devices:DeviceSignalInfo(accountId),addresses:playingAddresses.has(accountId)?[playingAddresses.get(accountId)!]:[]};
}
export function SetAccountBan(accountId:unknown,reason:unknown,active:unknown,address:unknown,actor:string,durationHours?:unknown,includeDevice:unknown=false){
    if(typeof includeDevice!=='boolean')throw Error('invalid_device_scope');
    if(typeof accountId!=='string' || typeof reason!=='string' || !reason.trim() || reason.length>500 || typeof active!=='boolean') throw Error('invalid_ban');
    const ip=address == null || address==='' ? null : NormalizeAddress(address);
    if(address && (!ip || IsLoopbackAddress(ip))) throw Error('invalid_address');
    if(durationHours!==undefined && (typeof durationHours!=='number' || !Number.isInteger(durationHours) || durationHours<1 || durationHours>720)) throw Error('invalid_duration');
    const expiresAt=active && typeof durationHours==='number' ? Date.now()+durationHours*3600000 : null;
    const db=GetDb().$client;
    return db.transaction(()=>{
        const user=db.prepare('SELECT isAdmin FROM users WHERE userId=?').get(accountId) as {isAdmin:number}|undefined;
        if(!user)throw Error('account_not_found');
        if(user.isAdmin)throw Error('admin_account_protected');
        ExpirePlayingAddresses();
        if(ip && playingAddresses.get(accountId)?.address!==ip)throw Error('address_not_observed');
        SnapshotDeviceBan(accountId,active && includeDevice);
        const args=[accountId,reason.trim(),ip,Number(active),Date.now(),actor];
        db.prepare(`INSERT INTO account_bans(account_id,reason,address,active,updated_at,actor,expires_at) VALUES (?,?,?,?,?,?,?) ON CONFLICT(account_id) DO UPDATE SET
            reason=excluded.reason,address=excluded.address,active=excluded.active,updated_at=excluded.updated_at,actor=excluded.actor,expires_at=excluded.expires_at`).run(...args,expiresAt);
        db.prepare('INSERT INTO moderation_events(account_id,reason,address,active,created_at,actor,expires_at) VALUES (?,?,?,?,?,?,?)').run(...args,expiresAt);
        return ModerationInfo(accountId);
    })();
}

export function ModerationLog(kind:'bans'|'events',offset=0){
    if(!Number.isSafeInteger(offset) || offset<0 || offset>1000000)throw Error('invalid_offset');
    const db=GetDb().$client,now=Date.now();
    const rows=kind==='bans'
        ? db.prepare(`SELECT b.account_id,u.name AS username,b.reason,b.updated_at,b.expires_at,b.actor FROM account_bans b LEFT JOIN users u ON u.userId=b.account_id
            WHERE b.active=1 AND (b.expires_at IS NULL OR b.expires_at>?) ORDER BY b.updated_at DESC,b.account_id LIMIT 51 OFFSET ?`).all(now,offset)
        : db.prepare(`SELECT e.id,e.account_id,u.name AS username,e.reason,e.active,e.created_at,e.expires_at,e.actor FROM moderation_events e LEFT JOIN users u ON u.userId=e.account_id
            ORDER BY e.id DESC LIMIT 51 OFFSET ?`).all(offset);
    return {rows:rows.slice(0,50),nextOffset:rows.length>50?offset+50:null};
}
