import {createCipheriv, createDecipheriv, createHash, randomBytes} from 'node:crypto';
import {GetDb} from '../db';

function encryptionKey() {
    const secret=process.env.AUTH_SIGNING_PRIVKEY_B64;
    if(!secret) throw Error('Account recovery encryption unavailable');
    return createHash('sha256').update('dauntless-account-recovery-v1\0'+secret).digest();
}
export function RememberAccountKey(userId:string, key:string) {
    if(process.env.ACCOUNT_KEY_RECOVERY !== '1') return;
    const db=GetDb().$client;
    // Equality fingerprint for the existing userapikeys join, not password storage.
    // Issued UUKs contain 24 random bytes (registration) or 32 (rotation); keep
    // this digest byte-compatible with HashUserAPIKey. Imported weak keys do
    // not inherit that entropy guarantee and must be rotated, not slow-hashed here.
    const hash=createHash('sha256').update(key).digest('hex');
    const existing=db.prepare('SELECT keyHash FROM accountkeyrecovery WHERE userId=?').get(userId) as {keyHash:string}|undefined;
    if(existing?.keyHash===hash) return;
    const iv=randomBytes(12),cipher=createCipheriv('aes-256-gcm',encryptionKey(),iv);
    cipher.setAAD(Buffer.from(userId));
    const data=Buffer.concat([iv,cipher.update(key,'utf8'),cipher.final(),cipher.getAuthTag()]).toString('base64');
    db.prepare('INSERT INTO accountkeyrecovery(userId,keyHash,ciphertext) VALUES(?,?,?) ON CONFLICT(userId) DO UPDATE SET keyHash=excluded.keyHash,ciphertext=excluded.ciphertext').run(userId,hash,data);
}
export function RecoverAccountKey(userId:string):string|null {
    if(process.env.ACCOUNT_KEY_RECOVERY !== '1') return null;
    const row=GetDb().$client.prepare('SELECT r.ciphertext FROM accountkeyrecovery r JOIN userapikeys k ON k.userId=r.userId AND k.keyHash=r.keyHash JOIN users u ON u.userId=r.userId WHERE r.userId=? AND u.isAdmin=0').get(userId) as {ciphertext:string}|undefined;
    if(!row) return null;
    const data=Buffer.from(row.ciphertext,'base64');
    const cipher=createDecipheriv('aes-256-gcm',encryptionKey(),data.subarray(0,12));
    cipher.setAAD(Buffer.from(userId));cipher.setAuthTag(data.subarray(-16));
    return Buffer.concat([cipher.update(data.subarray(12,-16)),cipher.final()]).toString('utf8');
}
