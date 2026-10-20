import { readFile, stat } from 'node:fs/promises';
import { GetDb } from '../db';

export function SummarizeBotClaims(state: any, invites: {inviteCode:string, usesRemaining:number, infiniteUses:boolean}[]) {
    if (state?.version !== 1 || !state.users || typeof state.users !== 'object' || Array.isArray(state.users)) throw new Error('Invalid bot state');
    const codes = new Set<string>();
    const rows = new Map(invites.map(row=>[row.inviteCode,row]));
    const counts = {claims:0, issued:0, unused:0, redeemed:0, pending:0, revoked:0};
    for (const [discordId, entry] of Object.entries(state.users) as [string,any][]) {
        if (!/^\d{17,20}$/.test(discordId) || typeof entry?.code !== 'string' || codes.has(entry.code) || typeof entry.pending !== 'boolean') throw new Error('Invalid bot claim');
        codes.add(entry.code); counts.claims++;
        const row = rows.get(entry.code);
        if (!row) { if (entry.pending) counts.pending++; else {counts.revoked++;counts.issued++;} continue; }
        if (row.infiniteUses || ![0,1].includes(row.usesRemaining)) throw new Error('Invalid bot invite');
        counts.issued++;
        if (row.usesRemaining === 0) counts.redeemed++; else counts.unused++;
    }
    return counts;
}

export async function DiscordKeyStats(stateFile = process.env.DISCORD_KEY_STATE_FILE) {
    const db = GetDb().$client;
    const linkedAccounts = (db.prepare('SELECT count(*) AS n FROM discordlinks').get() as {n:number}).n;
    const result = {at:new Date().toISOString(), linkedAccounts, bot:null as ReturnType<typeof SummarizeBotClaims> | null, error:null as string | null};
    if (!stateFile) return {...result,error:'Bot claim statistics are not configured.'};
    try {
        if ((await stat(stateFile)).size > 5*1024*1024) throw new Error('State too large');
        const state = JSON.parse(await readFile(stateFile,'utf8'));
        const rows = db.prepare('SELECT invitecode AS inviteCode, usesRemaining, infiniteUses FROM invitecodes').all() as {inviteCode:string,usesRemaining:number,infiniteUses:number}[];
        result.bot = SummarizeBotClaims(state,rows.map(row=>({...row,infiniteUses:!!row.infiniteUses})));
    } catch { result.error = 'Bot claim statistics are unavailable; no zero counts have been assumed.'; }
    return result;
}
