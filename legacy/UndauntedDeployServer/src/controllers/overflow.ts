import { CapacityUnavailable, HuntLimit } from './capacity';
import { logger } from '../logger';

type Request = { GameMode: string; GameArgs: string; HuntId: string; ExpectedPlayers: string[] | undefined; Overflow?: boolean };
type Connection = { host: string; port: number };
const capacityRetryAt = new Map<string, number>();

export function OverflowUrl() {
    const value = process.env.OVERFLOW_DEPLOYSERVER_URL;
    if (!value) return undefined;
    const url = new URL(value);
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.username || url.password || url.pathname !== '/' || url.search || url.hash)
        throw new Error('OVERFLOW_DEPLOYSERVER_URL must be an HTTP loopback tunnel');
    return url;
}

export async function RemoteLaunch(url: URL, body: Request): Promise<Connection | undefined> {
    // Capacity refusals are shared by waiting parties, not by persistent-world launches.
    const capacityKey = `${url.origin}|${body.GameMode}`;
    const now = Date.now();
    for (const [key, until] of capacityRetryAt) if (until <= now) capacityRetryAt.delete(key);
    if (capacityRetryAt.has(capacityKey)) return undefined;
    // A tunnel can accept TCP while its remote target is down. Probe without side effects
    // before POST, so that failure still permits a safe local launch.
    try {
        const probe = await fetch(new URL('/gameservers', url), {signal: AbortSignal.timeout(2000), redirect:'error'});
        const status = await probe.json() as any;
        if (!probe.ok || !Array.isArray(status.servers)) return undefined;
        logger.info({huntId:body.HuntId, worker:status.capacity ?? {running:status.servers.filter((s:any)=>['hunt','tutorial'].includes(s.kind)).length}}, 'routing: worker capacity');
    } catch { logger.warn({huntId:body.HuntId}, 'routing: worker probe unavailable'); return undefined; }
    let response: Response;
    try {
        response = await fetch(new URL('/api/matchmaker/handle-matchmaking-for-player', url), {
            method: 'POST', headers: {'content-type': 'application/json'}, body: JSON.stringify(body),
            signal: AbortSignal.timeout(120_000), redirect: 'error'
        });
    } catch (error) {
        // A refused connection never reached the worker. A timeout/reset might have spawned a hunt;
        // do not launch a duplicate locally when the result is ambiguous.
        if ((error as any)?.cause?.code === 'ECONNREFUSED') return undefined;
        logger.warn({huntId:body.HuntId}, 'routing: worker timeout or ambiguous transport failure; no duplicate fallback');
        throw error;
    }
    const result = await response.json() as any;
    if (response.status === 503 && result.error === 'capacity_unavailable') {
        const seconds = Number(response.headers.get('retry-after'));
        const delay = Number.isFinite(seconds) && seconds > 0 ? Math.min(30, Math.max(1, seconds)) : 10;
        capacityRetryAt.set(capacityKey, Date.now() + delay * 1000);
        logger.info({huntId:body.HuntId,reason:result.reason}, 'routing: worker capacity rejection');
        return undefined;
    }
    if (!response.ok || typeof result.host !== 'string' || !Number.isInteger(result.port) || result.port < 1 || result.port > 65535)
        {logger.warn({huntId:body.HuntId,status:response.status}, 'routing: worker HTTP error'); throw new Error(`Overflow launch failed (${response.status})`);}
    logger.info({huntId:body.HuntId,host:result.host,port:result.port,selected:'worker'}, 'routing: selected');
    return result;
}

export class HuntRouter {
    private pending = 0;
    constructor(private localCount: () => number, private remote = RemoteLaunch, private localStatus?: () => {running:number,pending:number,limit:number|null}) {}
    async launch<T>(body: Request, local: () => Promise<T>): Promise<T | Connection> {
        const url = OverflowUrl();
        if (!url || body.GameMode !== 'ISLAND') return local();
        const threshold = Number(process.env.OVERFLOW_AFTER_HUNTS ?? 4);
        if (!Number.isInteger(threshold) || threshold < 0) throw new Error('Invalid OVERFLOW_AFTER_HUNTS');
        const status=this.localStatus?.() ?? {running:this.localCount(),pending:this.pending,limit:HuntLimit()};
        const preferRemote = status.running + status.pending >= threshold;
        logger.info({huntId:body.HuntId,localRunning:status.running,pendingLocal:status.pending,localCapacity:status.limit,threshold,preferred:preferRemote?'worker':'primary'}, 'routing: preference');
        this.pending++;
        try {
            if (preferRemote) {
                const result = await this.remote(url, body);
                if (result) return result;
            }
            try { const result=await local(); logger.info({huntId:body.HuntId,selected:'primary',host:(result as any)?.host,port:(result as any)?.port}, 'routing: selected'); return result; }
            catch (error) {
                if (error instanceof CapacityUnavailable) logger.info({huntId:body.HuntId,reason:error.reason}, 'routing: local capacity rejection');
                if (!(error instanceof CapacityUnavailable) || preferRemote) throw error;
                const result = await this.remote(url, body);
                if (result) return result;
                throw error;
            }
        } finally { this.pending--; }
    }
}

export type GameserverSnapshot = { servers: any[]; complete: boolean };

// An empty result from an unreachable worker is not evidence that its sessions died.
// Preserve that distinction for matchmaking's cached-allocation liveness checks.
export async function ReadWorkerSnapshot(url: URL): Promise<GameserverSnapshot> {
    try {
        const response = await fetch(new URL('/gameservers', url), {signal: AbortSignal.timeout(1500), redirect: 'error'});
        if (!response.ok) { await response.body?.cancel(); return {servers: [], complete: false}; }
        const body = await response.json() as any;
        if (!Array.isArray(body.servers)) return {servers: [], complete: false};
        // Older workers do not report whether their own remote snapshots succeeded.
        return {servers: body.servers, complete: body.complete === true};
    } catch { return {servers: [], complete: false}; }
}

export async function DescribeOverflowSnapshot(): Promise<GameserverSnapshot> {
    const url = OverflowUrl();
    if (!url) return {servers: [], complete: true};
    const snapshot = await ReadWorkerSnapshot(url);
    return {...snapshot, servers: snapshot.servers.map(server => ({...server, host: 'overflow'}))};
}

export async function DescribeOverflow(): Promise<any[]> {
    return (await DescribeOverflowSnapshot()).servers;
}
