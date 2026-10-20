import {BanFor, SetAccountBan} from './moderation';
import {GetDb} from '../db';

// Exact mutation signatures from the supplied Nova 1.0.3 source. A name,
// user-agent, inventory read, or empty connection probe is not a detection.
const SOURCES = new Set([
    'local-editor:catalog-cosmetics:user-request',
    'local-editor:user-request',
    'local-editor:equipment:user-request',
    'local-editor:cells-five-each:user-request',
    'local-editor:unlock-equipment:user-request',
    'local-editor:tonics:user-request',
    'nova-unlocker:platinum:user-request',
]);
const DELTAS = ['addInstancedItems','removeInstancedItems','saveInstancedItems','addStackedItems','removeStackedItems'] as const;

// Call only after authentication, before any inventory operation. The authenticated
// actor is the sole sanction target; supplied account/character IDs are untrusted.
export function EnforceKnownUnlockerMutation(auth: {userId?: unknown; IsGameserver?: unknown} | undefined, body: unknown): {blocked:boolean; suspended:boolean} {
    if (!auth || auth.IsGameserver || typeof auth.userId !== 'string' || !/^UID-[A-Za-z0-9-]{1,100}$/.test(auth.userId) ||
        !body || typeof body !== 'object' || Array.isArray(body)) return {blocked:false,suspended:false};
    const request = body as Record<string,unknown>;
    if (typeof request.source !== 'string' || !SOURCES.has(request.source)) return {blocked:false,suspended:false};
    const counts = DELTAS.map(key => [key,Array.isArray(request[key]) ? request[key].length : 0] as const);
    if (!counts.some(([,count]) => count > 0)) return {blocked:false,suspended:false};
    const accountId = auth.userId;
    return GetDb().$client.transaction(() => {
    if (BanFor(accountId)) return {blocked:true,suspended:true};
    // Compact bounded evidence goes into the existing dashboard moderation log.
    // No tokens, arbitrary body text, IP ban, or device ban is recorded here.
    const reason = `Blocked inventory mutation; Nova 1.0.3 signature ${request.source}; ${counts.filter(([,n])=>n>0).map(([key,n])=>`${key}=${n}`).join(', ')}`;
    try { SetAccountBan(accountId,reason,true,null,'anticheat:nova-marker',168,false); }
    catch (error) {
        if (error instanceof Error && error.message === 'admin_account_protected') return {blocked:true,suspended:false};
        throw error; // Caller must fail closed if evidence cannot be persisted.
    }
    return {blocked:true,suspended:true};
    }).immediate();
}
