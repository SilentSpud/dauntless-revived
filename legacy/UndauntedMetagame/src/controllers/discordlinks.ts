import { GetDb } from '../db';

export function DiscordAccount(discordId: string) {
    return GetDb().$client.prepare('SELECT d.userId, u.name AS username FROM discordlinks d JOIN users u ON u.userId=d.userId WHERE d.discordId=?').get(discordId) as {userId:string, username:string} | undefined;
}

// Called only by an authenticated administrator after the bot verifies the existing key.
// This table is additive: linking never writes account keys, characters or progression.
export function LinkDiscordAccount(discordId: unknown, userId: unknown) {
    if (typeof discordId !== 'string' || !/^\d{17,20}$/.test(discordId) || typeof userId !== 'string' || !/^UID-[A-Za-z0-9-]{1,100}$/.test(userId)) return {status:'invalid_key'};
    const db = GetDb().$client;
    return db.transaction(() => {
        if (!db.prepare('SELECT 1 FROM users WHERE userId=?').get(userId)) return {status:'invalid_key'};
        const existing = DiscordAccount(discordId);
        if (existing && existing.userId !== userId) return {status:'discord_already_linked'};
        if (db.prepare('SELECT 1 FROM discordlinks WHERE userId=? AND discordId<>?').get(userId, discordId)) return {status:'account_already_linked'};
        db.prepare('INSERT OR IGNORE INTO discordlinks(discordId,userId,linkedAt) VALUES(?,?,?)').run(discordId,userId,new Date().toISOString());
        return {status:'linked'};
    }).immediate();
}
