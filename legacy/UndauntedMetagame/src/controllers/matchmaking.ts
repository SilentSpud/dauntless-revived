import { PlayerRegion, RegionQueueKey, HuntRegion, RegionChoice } from './huntregion';
import { CapacityTurns } from './capacityturns';
import { logger } from "../logger";
import crypto from "node:crypto";
import { MatchmakingRoster } from './matchmakingroster';
import {
    ClearPartyCandidate, GetPartyOf, LAST_CANDIDATE_REJOIN_MS, MarkPartyCandidateReady, MarkPartyCandidateServed, Party, PartyCandidate,
    PartyForMatchmaking, PartyNow, RemoveFromPartyCandidate, SetCandidateLeaveHook, SetPartyCandidate, TouchPlayer
} from "./party";

const MATCHMAKING_MODE = process.env.MATCHMAKING_MODE;
const DEPLOYSERVER_URL = process.env.DEPLOYSERVER_URL;
const DEPLOYSERVER_MATCHMAKING_PATH = "/api/matchmaker/handle-matchmaking-for-player";
MatchmakingRoster.setClock(PartyNow);

// A party's join waits this long at most for the deploy server, so the leader's first status
// poll (sent right after the join) can already travel; the rest is picked up by later polls
const PARTY_JOIN_WAIT_MS = 2500;
// A player who joins the hunt their party is already on (the client sends a second join
// while it loads into the island) gets MATCHING for this long and nothing new is started;
// if they are still asking after it, they are sent to the party's server again
export const REJOIN_PARK_MS = 20 * 1000;
const SOLO_REJOIN_WINDOW_MS = 60 * 1000;
const SOLO_JOIN_DEDUPE_MS = 30 * 1000;
// Allow the deploy server's 180 s startup grace, pacing, and its 120 s worker hop.
// One deadline covers both response headers and its complete JSON body.
const DEFAULT_DEPLOY_TIMEOUT_MS = 240 * 1000;
const HEALTH_TIMEOUT_MS = 3000;
const HEALTH_CACHE_MS = 5000;
// A hunt queue goes to the deploy server this long after its first join (checked when a queued
// player polls), or at once when it reaches QUEUE_FULL_PLAYERS
const QUEUE_WAIT_MS = 5 * 1000;
const QUEUE_FULL_PLAYERS = 4;
// A live player's polls renew this lease; disconnected waiters expire.
export const CAPACITY_WAIT_MS = 60 * 1000;
const CapacityOrder = new CapacityTurns(CAPACITY_WAIT_MS);
// The deploy server answers "capacity" whenever two launches are already waiting to spawn, so a
// retry usually fits a few seconds later; ten seconds between tries left most of the minute unused
const CAPACITY_RETRY_MS = 3 * 1000;

type MatchmakingQueueData = {
    Private?: boolean,
    Region: HuntRegion,
    HuntId: string,
    RetryAfter?: number,
    CapacityDeadline?: number,
    Players: string[],
    CandidateIds: Map<string, string>,
    FirstPlayerAddedTime: number,
    Resolved: boolean
};

export type MatchmakingResult = {
    Region?: HuntRegion,

    Ready: boolean,
    HuntId: string,
    CandidateId: string,
    Host: string,
    Port: number,
    Failed?: boolean,          // no game server could be started: the status poll answers FAILED
    PartyCandidate?: boolean,  // part of a party's candidate (no per-hunt queue behind it)
    PartyMemberIds?: string[], // the candidate's members, for the status reply's playerStates
    ParkedAt?: number,
    Private?: boolean,
    QueuedAt?: number,
    DirectKey?: string,
    LaunchDeadline?: number,
    SessionId?: string,
    AllocatedAt?: number,
    FirstSentAt?: number
};

type LaunchResult = {
    capacity?: boolean,
    retryAfterMs?: number,
    succeeded: boolean,
    readyNow: boolean,
    host: string,
    port: number,
    sessionId?: string,
    allocatedAt?: number
};

let MatchmakingQueueMap: Map<string, MatchmakingQueueData> = new Map<string, MatchmakingQueueData>();
let MatchmakingResultMap: Map<string, MatchmakingResult> = new Map<string, MatchmakingResult>();
const PlayerQueueMap = new Map<string, MatchmakingQueueData>();
// The server each player was last told to travel to (the status poll's IN_PROGRESS)
const LastSentMap = new Map<string, { Host: string, Port: number, HuntId: string, SessionId?: string, At: number }>();
// A native session belongs to this allocation, never to a mutable host:port slot.
// Legacy deploy replies have no safe shared-session identity.
export function GameSessionForCandidate(entry: MatchmakingResult) {
    return entry.SessionId ?? entry.CandidateId;
}

function DeployTimeoutMs() {
    const setting = process.env.MATCHMAKING_DEPLOY_TIMEOUT_MS;
    if (setting === undefined) return DEFAULT_DEPLOY_TIMEOUT_MS;
    const value = Number(setting);
    if (!/^\d+$/.test(setting) || !Number.isSafeInteger(value) || value < 1000 || value > 900000) {
        logger.warn('Invalid MATCHMAKING_DEPLOY_TIMEOUT_MS; using 240000');
        return DEFAULT_DEPLOY_TIMEOUT_MS;
    }
    return value;
}

async function WithDeadline<T>(work: (signal: AbortSignal) => Promise<T>, timeoutMs: number): Promise<T> {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        return await Promise.race([
            work(controller.signal),
            new Promise<never>((_, reject) => {
                timer = setTimeout(() => {
                    const error = new Error(`Deploy request exceeded ${timeoutMs} ms`);
                    controller.abort(error);
                    reject(error);
                }, timeoutMs);
            })
        ]);
    } finally { if (timer) clearTimeout(timer); }
}

type HealthSnapshot = { startedAt: number, until: number, complete: boolean, sessions: Set<string> };
let HealthCache: HealthSnapshot | undefined;
let HealthRequest: Promise<HealthSnapshot> | undefined;

async function AllocationHealth(entry: MatchmakingResult): Promise<boolean | undefined> {
    // A successful launch just confirmed this session. Legacy replies have no identity
    // safe to check: port reuse must never turn an old process into a live new one.
    if (!entry.SessionId || PartyNow() - (entry.AllocatedAt ?? 0) < HEALTH_CACHE_MS) return undefined;
    if (!HealthCache || HealthCache.until <= PartyNow()) {
        HealthRequest ??= (async () => {
            const startedAt = PartyNow();
            let complete = false;
            let sessions = new Set<string>();
            try {
                const body = await WithDeadline(async signal => {
                    const reply = await fetch(`http://${DEPLOYSERVER_URL}/gameservers`, {signal, redirect: 'error'});
                    if (!reply.ok) throw new Error(`Game server health returned ${reply.status}`);
                    return await reply.json();
                }, HEALTH_TIMEOUT_MS);
                if (Array.isArray(body?.servers) && body.servers.every((server: any) => typeof server?.id === 'string')) {
                    sessions = new Set(body.servers.map((server: any) => server.id));
                    complete = body.complete === true;
                }
            } catch { /* An unavailable worker is unknown, not proof of a dead process. */ }
            return {startedAt, until: PartyNow() + HEALTH_CACHE_MS, complete, sessions};
        })();
        const pending = HealthRequest;
        try { HealthCache = await pending; }
        finally { if (HealthRequest === pending) HealthRequest = undefined; }
    }
    const snapshot = HealthCache;
    if (!snapshot || snapshot.startedAt < (entry.AllocatedAt ?? 0)) return undefined;
    if (snapshot.sessions.has(entry.SessionId)) return true;
    return snapshot.complete ? false : undefined;
}

function FailCandidate(candidateId: string, reason: string) {
    CapacityWaits.delete(candidateId);
    CapacityOrder.remove(candidateId);
    MatchmakingRoster.cancelCandidate(candidateId);
    for (const [playerId, entry] of MatchmakingResultMap) {
        if (entry.CandidateId !== candidateId) continue;
        entry.Failed = true;
        entry.Ready = false;
        entry.LaunchDeadline = undefined;
        const party = GetPartyOf(playerId);
        if (party) {
            ClearPartyCandidate(party, candidateId, reason);
            if (party.LastCandidate?.CandidateId === candidateId) party.LastCandidate = null;
        }
    }
}

function AllocationExpired(entry: MatchmakingResult) {
    const at = entry.PartyCandidate ? entry.AllocatedAt : entry.FirstSentAt ?? entry.AllocatedAt;
    const lifetime = entry.PartyCandidate ? LAST_CANDIDATE_REJOIN_MS : SOLO_REJOIN_WINDOW_MS;
    return at !== undefined && PartyNow() - at >= lifetime;
}

async function ReadyAllocationUsable(entry: MatchmakingResult) {
    if (entry.Failed) return false;
    if (entry.PartyCandidate && AllocationExpired(entry)) {
        FailCandidate(entry.CandidateId, 'allocation reuse expired');
        return false;
    }
    const health = await AllocationHealth(entry);
    if (health === false) {
        FailCandidate(entry.CandidateId, 'allocated game server exited');
        return false;
    }
    // Positive session identity keeps a healthy running hunt reusable. Without
    // positive evidence, polls cannot keep extending the original fallback lease.
    if (health !== true && AllocationExpired(entry)) {
        FailCandidate(entry.CandidateId, 'allocation reuse expired');
        return false;
    }
    return !entry.Failed;
}

function ApplyLaunchResult(playerId: string, entry: MatchmakingResult, game: LaunchResult) {
    if (MatchmakingResultMap.get(playerId) !== entry || entry.Failed) return;
    entry.LaunchDeadline = undefined;
    if (game.succeeded) {
        entry.Host = game.host;
        entry.Port = game.port;
        entry.SessionId = game.sessionId;
        entry.AllocatedAt = game.allocatedAt ?? PartyNow();
        entry.Ready = true;
    } else FailCandidate(entry.CandidateId, 'no game server could be started');
}

function StoreEntry(playerId: string, entry: MatchmakingResult) {
    entry.QueuedAt ??= PartyNow();
    const previous = MatchmakingResultMap.get(playerId);
    if (previous && previous.CandidateId !== entry.CandidateId) MatchmakingRoster.removePlayer(playerId, previous.CandidateId, 'replaced');
    MatchmakingResultMap.set(playerId, entry);
    MatchmakingRoster.recordQueued(playerId, entry.CandidateId, entry.HuntId);
}

function PruneWaitingQueue(queue: MatchmakingQueueData) {
    if (queue.Resolved) return;
    queue.Players = queue.Players.filter(playerId => {
        const candidateId = queue.CandidateIds.get(playerId);
        if (candidateId && MatchmakingRoster.queueMemberActive(playerId, candidateId)) return true;
        if (candidateId) FailCandidate(candidateId, 'queued player stopped polling');
        queue.CandidateIds.delete(playerId);
        if (PlayerQueueMap.get(playerId) === queue) PlayerQueueMap.delete(playerId);
        return false;
    });
    const key = RegionQueueKey(queue.HuntId, queue.Region);
    if (!queue.Players.length && MatchmakingQueueMap.get(key) === queue) MatchmakingQueueMap.delete(key);
}


type CapacityWait = { next: number, deadline: number, busy: boolean, retry: () => Promise<boolean>, retryMs: () => number };
const CapacityWaits = new Map<string, CapacityWait>();
function ParkCapacity(CandidateId: string, retry: () => Promise<boolean>, retryMs: () => number = () => CAPACITY_RETRY_MS) {
    // No background launches: only an active candidate's status poll can retry.
    for (const [id, wait] of CapacityWaits) {
        const entries = [...MatchmakingResultMap.entries()].filter(([, entry]) => entry.CandidateId === id && !entry.Failed && !entry.Ready);
        if (wait.deadline < PartyNow()) FailCandidate(id, 'capacity wait expired');
        if (wait.deadline < PartyNow() || entries.length === 0) CapacityWaits.delete(id);
    }
    if (CapacityWaits.size >= 1000) return false;
    CapacityWaits.set(CandidateId, {next: PartyNow() + retryMs(), deadline: PartyNow() + CAPACITY_WAIT_MS, busy: false, retry, retryMs});
    const member = [...MatchmakingResultMap.entries()].find(([, entry]) => entry.CandidateId === CandidateId && entry.PartyCandidate);
    const party = member && GetPartyOf(member[0]);
    if (party?.Candidate?.CandidateId === CandidateId) party.Candidate.CapacityDeadline = PartyNow() + CAPACITY_WAIT_MS;
    return true;
}

function HuntIdRequiresMatchmaking(HuntId: string){
    return !HuntId.includes("Ramsgate") && !HuntId.includes("Dojo");
    //return HuntId.includes("CR19") || HuntId.includes("11A") || HuntId.includes("Story");
}

// Each account once, in first-seen order. A hunt server waits for every expected player, so a
// player listed twice keeps it waiting for someone who never comes: on 22 September 2026 the
// list was V, V, O and the airship countdown froze with both real players ready.
export function DistinctPlayers(PlayerIds: string[]): string[] {
    return [...new Set(PlayerIds)];
}

// Never throws: an unreachable deploy server or an unusable answer is a failed launch
async function LaunchGameOnDeployserver(GameMode: string, GameArgs: string, HuntId: string, AskedPlayers: string[] | undefined, Region: RegionChoice = 'main'): Promise<LaunchResult> {
    const candidate = ['ISLAND','CITY'].includes(GameMode) && AskedPlayers?.length
        ? MatchmakingResultMap.get(AskedPlayers[0])?.CandidateId : undefined;
    if (!candidate) return LaunchGameOnDeployserverNow(GameMode, GameArgs, HuntId, AskedPlayers, Region);
    const active = new Set([...MatchmakingResultMap.values()].filter(e=>!e.Ready && !e.Failed).map(e=>e.CandidateId));
    const queuedAt = Math.min(...AskedPlayers!.map(id => MatchmakingResultMap.get(id)?.QueuedAt ?? PartyNow()));
    // City seats and hunt processes are separate resources; a full city must not hold up hunts.
    if (!CapacityOrder.start(candidate, `${Region}:${GameMode}`, PartyNow(), active, queuedAt))
        return {succeeded:false,readyNow:false,host:'',port:0,capacity:true};
    let capacity = false;
    try {
        const result = await LaunchGameOnDeployserverNow(GameMode, GameArgs, HuntId, AskedPlayers, Region);
        capacity = result.capacity === true;
        return result;
    } finally { CapacityOrder.finish(candidate, capacity, PartyNow()); }
}

async function LaunchGameOnDeployserverNow(GameMode: string, GameArgs: string, HuntId: string, AskedPlayers: string[] | undefined, Region: RegionChoice = 'main'): Promise<LaunchResult> {
    const ExpectedPlayers = AskedPlayers === undefined ? undefined : DistinctPlayers(AskedPlayers);

    if(AskedPlayers !== undefined && ExpectedPlayers!.length !== AskedPlayers.length){
        logger.warn(`mm: ${HuntId}: ${AskedPlayers.length - ExpectedPlayers!.length} repeated expected player(s) dropped (${AskedPlayers.join(",")})`);
    }

    logger.info(`Querying DeployServer for GameMode: ${GameMode} HuntId ${HuntId} with ${ExpectedPlayers?.length} Expected Players!`);

    const URL = "http://" + DEPLOYSERVER_URL + DEPLOYSERVER_MATCHMAKING_PATH;
    const Failed: LaunchResult = { succeeded: false, readyNow: false, host: "", port: 0 };

    let MatchmakingResult: {status: number, body: any, retryAfter?: string | null};

    try{
        MatchmakingResult = await WithDeadline(async signal => {
            const reply = await fetch(URL, {
                method: "POST",
                signal,
                redirect: 'error',
                headers: {"Content-Type": "application/json"},
                body: JSON.stringify({
                    GameMode,
                    GameArgs,
                    HuntId,
                    ExpectedPlayers,
                    ...(['ISLAND','CITY','SHARED'].includes(GameMode) && Region !== 'main' ? {Region} : {})
                })
            });
            if (reply.status === 200 || reply.status === 503) return {status: reply.status, body: await reply.json(), retryAfter: reply.headers.get("retry-after")};
            void reply.body?.cancel().catch(() => {});
            return {status: reply.status, body: undefined};
        }, DeployTimeoutMs());
    }
    catch(error){
        // A timeout/reset may have started a remote game. End this attempt rather
        // than automatically issuing another allocation with an ambiguous result.
        logger.error(`DeployServer allocation failed: ${(error as Error)?.message}`);

        return Failed;
    }

    if (MatchmakingResult.status === 503) {
        const body = MatchmakingResult.body;
        if (body?.error === 'capacity_unavailable' && ['memory', 'ports', 'hunts', 'cpu'].includes(body.reason)) {
            logger.warn({reason: body.reason, huntId: HuntId, capacityWaitMs: CAPACITY_WAIT_MS}, 'mm: allocation waiting for capacity');
            const seconds = Number(MatchmakingResult.retryAfter);
            const retryAfterMs = MatchmakingResult.retryAfter?.trim() && Number.isFinite(seconds) && seconds > 0 ? Math.min(30_000, Math.max(CAPACITY_RETRY_MS, seconds * 1000)) : CAPACITY_RETRY_MS;
            return {...Failed, capacity: true, retryAfterMs};
        }
        return Failed;
    }
    if(MatchmakingResult.status === 200){
        const MatchmakingData = MatchmakingResult.body;

        const Host = MatchmakingData?.host;
        const Port = MatchmakingData?.port;

        // The client needs a non-empty host and a numeric port, or it polls forever or refuses the session
        if(typeof Host !== "string" || Host.length === 0 || !Number.isInteger(Port) || Port <= 0 || Port > 65535){
            logger.error(`DeployServer returned no usable game server`);

            return Failed;
        }
        const SessionId = typeof MatchmakingData.sessionId === 'string' && /^[A-Za-z0-9-]{1,64}$/.test(MatchmakingData.sessionId) ? MatchmakingData.sessionId : undefined;
        logger.info(`DeployServer returned gameserver ${Host}:${Port}`);

        return {
            succeeded: true,
            readyNow: true,
            host: Host,
            port: Port,
            sessionId: SessionId,
            allocatedAt: PartyNow()
        }
    }
    else{
        logger.error(`DeployServer returned status ${MatchmakingResult.status}`);

        return Failed;
    }
}

async function WaitBriefly(Work: Promise<unknown>) {
    let Timer: ReturnType<typeof setTimeout> | undefined;
    try {
        await Promise.race([Work, new Promise<void>(Resolve => { Timer = setTimeout(Resolve, 1500); })]);
    } finally { if (Timer) clearTimeout(Timer); }
}

async function PopQueue(HuntId: string, MatchmakingQueue: MatchmakingQueueData | undefined = MatchmakingQueueMap.get(HuntId)){
    if(MatchmakingQueue == undefined || MatchmakingQueue.Resolved || (MatchmakingQueue.RetryAfter ?? 0) > PartyNow()){
        return;
    }

    PruneWaitingQueue(MatchmakingQueue);
    if (!MatchmakingQueue.Players.length) return;

    MatchmakingQueue.Resolved = true;

    if(MatchmakingQueueMap.get(RegionQueueKey(HuntId, MatchmakingQueue.Region)) === MatchmakingQueue){
        MatchmakingQueueMap.delete(RegionQueueKey(HuntId, MatchmakingQueue.Region));
    }

    const allocationId = crypto.randomUUID();
    const deadline = PartyNow() + DeployTimeoutMs();
    const members = MatchmakingQueue.Players.map(playerId => ({playerId, candidateId: MatchmakingQueue.CandidateIds.get(playerId)!}));
    for (const {playerId, candidateId} of members) {
        const entry = MatchmakingResultMap.get(playerId);
        if (entry?.CandidateId === candidateId) entry.LaunchDeadline = deadline;
    }
    MatchmakingRoster.beginAllocation(allocationId, members, deadline);
    const GameOnDeployServer: LaunchResult = MatchmakingQueue.CapacityDeadline !== undefined && PartyNow() >= MatchmakingQueue.CapacityDeadline
        ? {succeeded: false, readyNow: false, host: '', port: 0}
        : await LaunchGameOnDeployserver("ISLAND", "", HuntId, MatchmakingQueue.Players, MatchmakingQueue.Region);

    if (GameOnDeployServer.capacity) {
        MatchmakingRoster.releaseAllocation(allocationId);
        for (const {playerId, candidateId} of members) {
            const entry = MatchmakingResultMap.get(playerId);
            if (entry?.CandidateId === candidateId) entry.LaunchDeadline = undefined;
        }
        MatchmakingQueue.CapacityDeadline ??= PartyNow() + CAPACITY_WAIT_MS;
        if (PartyNow() < MatchmakingQueue.CapacityDeadline && MatchmakingQueue.Players.length > 0) {
            MatchmakingQueue.Resolved = false;
            MatchmakingQueue.RetryAfter = PartyNow() + (GameOnDeployServer.retryAfterMs ?? CAPACITY_RETRY_MS);
            // A private retry must never enter or merge with the public matchmaking pool.
            if (MatchmakingQueue.Private) return;
            // Keep taking joiners while waiting: dropped from the map, every new player for this hunt
            // opened a queue of its own, asked for another server and made the capacity wait worse
            const QueueKey = RegionQueueKey(HuntId, MatchmakingQueue.Region);
            const Newer = MatchmakingQueueMap.get(QueueKey);
            // Players who joined while the launch request was out opened a fresh queue under this
            // key. Fold them in if they fit and that queue has not asked for a server yet
            if(Newer !== undefined && Newer !== MatchmakingQueue && !Newer.Resolved && Newer.RetryAfter === undefined
                && MatchmakingQueue.Players.length + Newer.Players.length <= QUEUE_FULL_PLAYERS){
                for(const Player of Newer.Players){
                    const CandidateId = Newer.CandidateIds.get(Player);
                    if(CandidateId === undefined || MatchmakingQueue.Players.includes(Player)) continue;
                    MatchmakingQueue.Players.push(Player);
                    MatchmakingQueue.CandidateIds.set(Player, CandidateId);
                    if(PlayerQueueMap.get(Player) === Newer) PlayerQueueMap.set(Player, MatchmakingQueue);
                }
                Newer.Players = [];
                Newer.CandidateIds.clear();
                Newer.Resolved = true;
                MatchmakingQueueMap.set(QueueKey, MatchmakingQueue);
            }
            else if(Newer === undefined){
                MatchmakingQueueMap.set(QueueKey, MatchmakingQueue);
            }
            return;
        }
    }

    if (GameOnDeployServer.succeeded) MatchmakingRoster.assignAllocation(allocationId, GameOnDeployServer.sessionId, GameOnDeployServer.host, GameOnDeployServer.port);

    for(const Player of MatchmakingQueue.Players){
        const PlayerMatchmakingResultToUpdate = MatchmakingResultMap.get(Player);
        const CandidateId = MatchmakingQueue.CandidateIds.get(Player);

        if(PlayerMatchmakingResultToUpdate != undefined && CandidateId !== undefined && PlayerMatchmakingResultToUpdate.CandidateId === CandidateId && !PlayerMatchmakingResultToUpdate.Ready && !PlayerMatchmakingResultToUpdate.PartyCandidate && PlayerMatchmakingResultToUpdate.HuntId === HuntId){
            if (PartyNow() >= deadline || (MatchmakingQueue.CapacityDeadline !== undefined && PartyNow() >= MatchmakingQueue.CapacityDeadline))
                FailCandidate(CandidateId, 'allocation deadline expired');
            else ApplyLaunchResult(Player, PlayerMatchmakingResultToUpdate, GameOnDeployServer);
        }

        if(PlayerQueueMap.get(Player) === MatchmakingQueue){
            PlayerQueueMap.delete(Player);
        }
    }

    if(!GameOnDeployServer.succeeded){
        logger.warn(`mm: the ${HuntId} queue (${MatchmakingQueue.Players.join(",")}) got no game server; their status polls answer FAILED`);
    }
}

export async function CheckAndUpdateQueueStatus(PlayerId: string){
    const PlayerMatchmakingResult = MatchmakingResultMap.get(PlayerId);

    if(PlayerMatchmakingResult == undefined){
        return undefined;
    }

    MatchmakingRoster.touchCandidate(PlayerId, PlayerMatchmakingResult.CandidateId);
    if (!PlayerMatchmakingResult.Ready && !PlayerMatchmakingResult.Failed
        && PlayerMatchmakingResult.LaunchDeadline !== undefined && PartyNow() >= PlayerMatchmakingResult.LaunchDeadline) {
        FailCandidate(PlayerMatchmakingResult.CandidateId, 'allocation request timed out');
    }
    if (PlayerMatchmakingResult.Failed) return PlayerMatchmakingResult;
    if (PlayerMatchmakingResult.Ready) {
        await ReadyAllocationUsable(PlayerMatchmakingResult);
        return MatchmakingResultMap.get(PlayerId);
    }

    const waiting = CapacityWaits.get(PlayerMatchmakingResult.CandidateId);
    const queueCandidate = PlayerQueueMap.get(PlayerId)?.CandidateIds.values().next().value;
    CapacityOrder.touch(queueCandidate ?? PlayerMatchmakingResult.CandidateId, PartyNow());
    if (waiting) {
        if (PartyNow() >= waiting.deadline) {
            FailCandidate(PlayerMatchmakingResult.CandidateId, 'capacity wait expired');
        } else {
            waiting.deadline = PartyNow() + CAPACITY_WAIT_MS;
            if (PlayerMatchmakingResult.PartyCandidate) {
                const party = GetPartyOf(PlayerId);
                if (party?.Candidate?.CandidateId === PlayerMatchmakingResult.CandidateId) party.Candidate.CapacityDeadline = waiting.deadline;
            }
            if (!waiting.busy && PartyNow() >= waiting.next) {
                waiting.busy = true;
                waiting.next = PartyNow() + CAPACITY_RETRY_MS;
                await WaitBriefly(waiting.retry().then(Done => { if (Done) CapacityWaits.delete(PlayerMatchmakingResult.CandidateId); }).finally(() => { waiting.next = PartyNow() + waiting.retryMs(); waiting.busy = false; }));
            }
        }
        return MatchmakingResultMap.get(PlayerId);
    }

    // Party candidates have no queue: the leader's join started their server already
    if(!PlayerMatchmakingResult.Ready && !PlayerMatchmakingResult.Failed && !PlayerMatchmakingResult.PartyCandidate && PlayerMatchmakingResult.DirectKey === undefined){
        const MatchmakingQueue = PlayerQueueMap.get(PlayerId);

        if(MatchmakingQueue == undefined){
            logger.warn(`mm: ${PlayerId} waits for ${PlayerMatchmakingResult.HuntId} but no queue holds them; answering FAILED`);
            FailCandidate(PlayerMatchmakingResult.CandidateId, 'queue no longer exists');

            return PlayerMatchmakingResult;
        }
        if (MatchmakingQueue.CapacityDeadline !== undefined && PartyNow() >= MatchmakingQueue.CapacityDeadline) {
            for (const candidateId of MatchmakingQueue.CandidateIds.values()) FailCandidate(candidateId, 'capacity wait expired');
            return MatchmakingResultMap.get(PlayerId);
        }
        if (MatchmakingQueue.CapacityDeadline !== undefined) MatchmakingQueue.CapacityDeadline = PartyNow() + CAPACITY_WAIT_MS;

        if(PartyNow() - MatchmakingQueue.FirstPlayerAddedTime >= QUEUE_WAIT_MS || (MatchmakingQueue.RetryAfter !== undefined && PartyNow() >= MatchmakingQueue.RetryAfter)){
            await WaitBriefly(PopQueue(PlayerMatchmakingResult.HuntId, MatchmakingQueue));
        }
    }

    return MatchmakingResultMap.get(PlayerId);
}

async function QueuePlayer(HuntId: string, PlayerId: string, Private = false){
    const Region = PlayerRegion(PlayerId);
    const QueueKey = RegionQueueKey(HuntId, Region);
    const CurrentEntry = MatchmakingResultMap.get(PlayerId);
    const CurrentQueue = PlayerQueueMap.get(PlayerId);
    if (CurrentEntry) MatchmakingRoster.touchCandidate(PlayerId, CurrentEntry.CandidateId);

    if(CurrentEntry !== undefined && !CurrentEntry.PartyCandidate && !CurrentEntry.Failed && CurrentEntry.HuntId === HuntId && CurrentEntry.Region === Region && !!CurrentEntry.Private === Private){
        if(!CurrentEntry.Ready && CurrentQueue !== undefined && CurrentQueue.Players.includes(PlayerId) && (CurrentQueue.Resolved || CurrentQueue.RetryAfter !== undefined || PartyNow() - (CurrentEntry.QueuedAt ?? 0) <= SOLO_JOIN_DEDUPE_MS)){
            return true;
        }

        if(CurrentEntry.Ready){
            const usable = await ReadyAllocationUsable(CurrentEntry);
            if (MatchmakingResultMap.get(PlayerId) !== CurrentEntry) return true;
            if (usable) {
                if (CurrentEntry.FirstSentAt !== undefined) CurrentEntry.ParkedAt ??= PartyNow();
                return true;
            }
        }
    }

    const ExistingQueue = Private ? undefined : MatchmakingQueueMap.get(QueueKey);
    if (ExistingQueue) PruneWaitingQueue(ExistingQueue);

    if(ExistingQueue !== undefined && ExistingQueue.Players.length >= QUEUE_FULL_PLAYERS){
        void PopQueue(HuntId, ExistingQueue);
    }

    LeaveWaitingQueues(PlayerId, "their new join replaces it");

    let Queue = Private ? undefined : MatchmakingQueueMap.get(QueueKey);

    if(Queue !== undefined && Queue.Players.length >= QUEUE_FULL_PLAYERS){
        void PopQueue(HuntId, Queue);
        Queue = undefined;
    }

    const CandidateId = crypto.randomUUID();

    StoreEntry(PlayerId, {
        Ready: false,
        CandidateId: CandidateId,
        HuntId: HuntId,
        Host: "",
        Port: 0,
        Private,
        Region,
        QueuedAt: PartyNow()
    });

    if(Queue === undefined){
        Queue = {
            Private,
            Region,
            HuntId: HuntId,
            Players: [],
            CandidateIds: new Map<string, string>(),
            FirstPlayerAddedTime: PartyNow(),
            Resolved: false
        };
        if (!Private) MatchmakingQueueMap.set(QueueKey, Queue);
    }

    Queue.Players.push(PlayerId);
    Queue.CandidateIds.set(PlayerId, CandidateId);
    PlayerQueueMap.set(PlayerId, Queue);

    if(Private || Queue.Players.length >= QUEUE_FULL_PLAYERS){
        void PopQueue(HuntId, Queue);
    }

    return true;
}

// A player follows one candidate at a time (their entry in MatchmakingResultMap). Whatever replaces
// it takes them out of every queue still waiting, so no hunt server is told to expect a player who
// went elsewhere, or the same player twice. A queue already asking the deploy server is left alone.
function LeaveWaitingQueues(PlayerId: string, Why: string){
    const Queue = PlayerQueueMap.get(PlayerId);

    if(Queue === undefined || Queue.Resolved || !Queue.Players.includes(PlayerId)){
        return;
    }

    Queue.Players = Queue.Players.filter((Player) => Player !== PlayerId);
    Queue.CandidateIds.delete(PlayerId);
    PlayerQueueMap.delete(PlayerId);

    if(Queue.Players.length === 0 && MatchmakingQueueMap.get(RegionQueueKey(Queue.HuntId, Queue.Region)) === Queue){
        MatchmakingQueueMap.delete(RegionQueueKey(Queue.HuntId, Queue.Region));
    }

    logger.info(`mm: ${PlayerId} taken out of the ${Queue.HuntId} queue: ${Why}`);
}

// A player going somewhere on their own no longer counts in their party's candidate
function ForgetPartyCandidateEntry(PlayerId: string){
    const Entry = MatchmakingResultMap.get(PlayerId);

    if(Entry?.PartyCandidate){
        RemoveFromPartyCandidate(PlayerId, Entry.CandidateId);
    }
}

// DELETE /candidate, only with MATCHMAKING_CANCEL=1 (see routes/matchmaking.ts). Upstream
// had no route, so a cancel failed and the queue popped ~20 s later anyway. Takes the
// player out of an unresolved queue (dropping it when empty) and forgets their result,
// so /candidate/status can no longer answer IN_PROGRESS for them. A party leader's cancel
// clears the party's candidate and the members' entries that were not sent yet; another
// member's cancel only takes that member out of it.
export function CancelMatchmaking(PlayerId: string){
    const PlayerMatchmakingResult = MatchmakingResultMap.get(PlayerId);

    if(PlayerMatchmakingResult == undefined){
        return undefined;
    }

    if(PlayerMatchmakingResult.PartyCandidate){
        const TheParty = GetPartyOf(PlayerId);
        const CandidateId = PlayerMatchmakingResult.CandidateId;

        if(TheParty !== undefined && TheParty.LeaderId === PlayerId && TheParty.Candidate?.CandidateId === CandidateId){
            let Dropped = 0;

            for(const Member of TheParty.Candidate.MemberIds){
                const Entry = MatchmakingResultMap.get(Member);

                if(Member !== PlayerId && Entry !== undefined && Entry.CandidateId === CandidateId && Entry.FirstSentAt === undefined){
                    MatchmakingRoster.removePlayer(Member, CandidateId);
                    MatchmakingResultMap.delete(Member);
                    Dropped++;
                }
            }

            ClearPartyCandidate(TheParty, CandidateId, `cancelled by the leader ${PlayerId}`);
            if (TheParty.LastCandidate?.CandidateId === CandidateId) TheParty.LastCandidate = null;
            logger.info(`mm: cancel by=${PlayerId} party P=${TheParty.PartyId} candidate ${CandidateId}: ${Dropped} member entr${Dropped === 1 ? "y" : "ies"} not yet sent dropped`);
        }
        else{
            RemoveFromPartyCandidate(PlayerId, CandidateId);
            if (TheParty?.LastCandidate?.CandidateId === CandidateId)
                TheParty.LastCandidate.MemberIds = TheParty.LastCandidate.MemberIds.filter(member => member !== PlayerId);
            logger.info(`mm: cancel by=${PlayerId} candidate ${CandidateId}: left the party's candidate`);
        }

        MatchmakingRoster.removePlayer(PlayerId, CandidateId);
        if (![...MatchmakingResultMap.entries()].some(([id, entry]) => id !== PlayerId && entry.CandidateId === CandidateId)) CapacityWaits.delete(CandidateId);
        MatchmakingResultMap.delete(PlayerId);

        return PlayerMatchmakingResult;
    }

    const MatchmakingQueue = PlayerQueueMap.get(PlayerId);

    if(MatchmakingQueue != undefined && !MatchmakingQueue.Resolved){
        MatchmakingQueue.Players = MatchmakingQueue.Players.filter((Player) => Player !== PlayerId);
        MatchmakingQueue.CandidateIds.delete(PlayerId);

        const queueKey = RegionQueueKey(MatchmakingQueue.HuntId, MatchmakingQueue.Region);
        if(MatchmakingQueue.Players.length === 0 && MatchmakingQueueMap.get(queueKey) === MatchmakingQueue){
            MatchmakingQueueMap.delete(queueKey);
        }
    }

    PlayerQueueMap.delete(PlayerId);
    MatchmakingRoster.removePlayer(PlayerId, PlayerMatchmakingResult.CandidateId);
    CapacityWaits.delete(PlayerMatchmakingResult.CandidateId);
    MatchmakingResultMap.delete(PlayerId);

    return PlayerMatchmakingResult;
}

// DELETE /candidate/leave, also only with MATCHMAKING_CANCEL=1: the caller alone leaves
// their candidate (a party leader's candidate goes on for the others)
export function LeaveCandidate(PlayerId: string){
    const PlayerMatchmakingResult = MatchmakingResultMap.get(PlayerId);

    if(PlayerMatchmakingResult == undefined || !PlayerMatchmakingResult.PartyCandidate){
        return CancelMatchmaking(PlayerId);
    }

    RemoveFromPartyCandidate(PlayerId, PlayerMatchmakingResult.CandidateId);
    const party = GetPartyOf(PlayerId);
    if (party?.LastCandidate?.CandidateId === PlayerMatchmakingResult.CandidateId)
        party.LastCandidate.MemberIds = party.LastCandidate.MemberIds.filter(member => member !== PlayerId);
    MatchmakingRoster.removePlayer(PlayerId, PlayerMatchmakingResult.CandidateId);
    MatchmakingResultMap.delete(PlayerId);
    logger.info(`mm: leave by=${PlayerId} candidate ${PlayerMatchmakingResult.CandidateId}`);

    return PlayerMatchmakingResult;
}

// A member who leaves the party while its candidate is still starting does not go along
SetCandidateLeaveHook((UserId, CandidateId) => {
    const Entry = MatchmakingResultMap.get(UserId);

    if(Entry !== undefined && Entry.PartyCandidate && Entry.CandidateId === CandidateId && Entry.FirstSentAt === undefined){
        MatchmakingRoster.removePlayer(UserId, CandidateId);
        MatchmakingResultMap.delete(UserId);
        logger.info(`mm: ${UserId} left the party before candidate ${CandidateId} got its server; they stay behind`);
    }
});

// Read-only, for /undaunted/api/ServerStatus: the game server a player was last sent to.
// In memory like the rest of matchmaking, so it is empty after a metagame restart.
export function GetLastMatchmakingResult(PlayerId: string){
    const Result = MatchmakingResultMap.get(PlayerId);

    return Result == undefined ? undefined : {Ready: Result.Ready, HuntId: Result.HuntId, Port: Result.Port};
}

// GET /candidate/status: what to answer, with the bookkeeping of a player being sent
export type CandidateStatusDecision =
    | { Kind: "unknown" }
    | { Kind: "failed" | "matching" | "travel", Entry: MatchmakingResult, Parked?: boolean };

export async function DecideCandidateStatus(PlayerId: string): Promise<CandidateStatusDecision> {
    const Entry = await CheckAndUpdateQueueStatus(PlayerId);

    if(Entry == undefined){
        return { Kind: "unknown" };
    }

    // A player polling for their party's candidate is still there
    if(Entry.PartyCandidate){
        TouchPlayer(PlayerId);
    }

    if(Entry.Failed){
        return { Kind: "failed", Entry: Entry };
    }

    if(!Entry.Ready){
        return { Kind: "matching", Entry: Entry };
    }

    if(Entry.ParkedAt !== undefined){
        if(PartyNow() - Entry.ParkedAt < REJOIN_PARK_MS){
            return { Kind: "matching", Entry: Entry, Parked: true };
        }

        Entry.ParkedAt = undefined;
        logger.info(`mm: ${PlayerId} still asking ${REJOIN_PARK_MS / 1000} s after joining candidate ${Entry.CandidateId} again: sending them to ${Entry.Host}:${Entry.Port} again`);
    }

    Entry.FirstSentAt ??= PartyNow();
    MatchmakingRoster.served(PlayerId, Entry.CandidateId);
    LastSentMap.set(PlayerId, { Host: Entry.Host, Port: Entry.Port, HuntId: Entry.HuntId, SessionId: Entry.SessionId, At: PartyNow() });

    if(Entry.PartyCandidate){
        MarkPartyCandidateServed(PlayerId, Entry.CandidateId, Entry.FirstSentAt);
    }

    return { Kind: "travel", Entry: Entry };
}

// What a client may send to /candidate/join. Its input ends up on a game server's command
// line: the deploy server passes the map and the behemoth from the client's own game args
// (the tutorial), and the hunt id inside the expected-player list. So only the shapes the
// client really sends are let through; the deploy server checks the same again.
// Keep in sync with UndauntedDeployServer/src/controllers/matchmakinginput.ts.
const HUNT_ID = /^[A-Za-z0-9_+]{1,128}$/; // e.g. CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr, ..._Patrol_Heroic+_Gem
const GAME_ARGS_CHARACTERS = /^[A-Za-z0-9_/.?=+-]{1,2048}$/;
const GAME_MAP_PATH = /^\/Game(\/[A-Za-z0-9_]+)+(\.[A-Za-z0-9_]+)?$/; // /Game/Maps/islands/1705/dia_moss_triforce
const GAME_ASSET_PATH = /^\/Game(\/[A-Za-z0-9_]+)+\.[A-Za-z0-9_]+$/; // /Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C

// undefined when the input is acceptable, else what is wrong with it
export function CheckMatchmakingInput(GameMode: unknown, GameArgs: unknown, HuntId: unknown): string | undefined {
    if(HuntId != undefined && !(typeof HuntId === "string" && (HuntId.length === 0 || HUNT_ID.test(HuntId)))){
        return "the hunt id is not a hunt id";
    }

    if(GameArgs != undefined && typeof GameArgs !== "string"){
        return "the game args are not a string";
    }

    // Used only for ISLAND: map ? option ? MonsterClass=<behemoth> ? ... (deploy server's StartupGameserverWithArgs)
    if(GameMode === "ISLAND" && typeof GameArgs === "string" && GameArgs.trim().length > 0){
        const Parts = GameArgs.split("?");
        const Behemoth = Parts[2]?.split("=")[1];

        if(!GAME_ARGS_CHARACTERS.test(GameArgs) || !GAME_MAP_PATH.test(Parts[0]) || Behemoth === undefined || (Behemoth.length > 0 && !GAME_ASSET_PATH.test(Behemoth))){
            return "the game args are not a map and behemoth the deploy server can start";
        }
    }

    return undefined;
}

// ---- Parties (roadmap 1.9, parties plan section 4): the whole party on one server ----
//
// Only the leader starts the party's hunts. Its join makes one candidate for all members
// and asks the deploy server once, with every member expected (their objectives and
// rewards need that). The members never join: they see the candidate in their POST /party
// poll and then poll GET /candidate/status, which answers each of them from their own entry
// here. Going back to Ramsgate (CITY) or the Dojo (SHARED) takes the members who are on the
// leader's current server along. A party of one, and the tutorial, use the solo code above.

async function FinishPartyCandidate(TheParty: Party, CandidateId: string, Members: string[], Game: LaunchResult){
    let Updated = 0;

    for(const Member of Members){
        const Entry = MatchmakingResultMap.get(Member);

        if(Entry === undefined || Entry.CandidateId !== CandidateId || Entry.Failed){
            continue;
        }

        ApplyLaunchResult(Member, Entry, Game);

        Updated++;
    }

    if(Game.succeeded){
        MarkPartyCandidateReady(TheParty, CandidateId, Game.host, Game.port);
        if (TheParty.Candidate?.CandidateId === CandidateId) {
            TheParty.Candidate.SessionId = Game.sessionId;
            TheParty.Candidate.AllocatedAt = Game.allocatedAt ?? PartyNow();
            TheParty.Candidate.LaunchDeadline = undefined;
        }
        logger.info(`mm: party P=${TheParty.PartyId} candidate ${CandidateId} ready at ${Game.host}:${Game.port} for ${Updated} member(s)`);
    }
    else{
        ClearPartyCandidate(TheParty, CandidateId, "no game server could be started");
        logger.warn(`mm: party P=${TheParty.PartyId} candidate ${CandidateId} got no game server; ${Updated} member(s) will see FAILED`);
    }
}

async function StartPartyCandidate(TheParty: Party, GameMode: string, HuntId: string, Members: string[], LeaderId: string){
    const Region = PlayerRegion(LeaderId); // The inviter/leader chooses the region for the entire party.
    const CandidateId = crypto.randomUUID();

    for(const Member of Members){
        LeaveWaitingQueues(Member, "their party's hunt takes them");

        StoreEntry(Member, {
            Ready: false,
            CandidateId: CandidateId,
            HuntId: HuntId,
            Host: "",
            Port: 0,
            PartyCandidate: true,
            PartyMemberIds: [...Members]
        });
    }

    SetPartyCandidate(TheParty, {
        CandidateId: CandidateId,
        State: "MATCHING",
        Region,
        GameMode: GameMode,
        HuntId: HuntId,
        MemberIds: [...Members],
        Served: new Set<string>(),
        CreatedAt: PartyNow()
    });

    logger.info(`mm: party P=${TheParty.PartyId} candidate ${CandidateId} mode=${GameMode} hunt=${HuntId} members=${Members.join(",")} by=${LeaderId}`);

    let retryMs = CAPACITY_RETRY_MS;
    const TryLaunch = async () => {
        const Active = Members.filter(member => {
            const entry = MatchmakingResultMap.get(member);
            return entry?.CandidateId === CandidateId && !entry.Failed && TheParty.Members.includes(member);
        });
        if (TheParty.Candidate?.CandidateId !== CandidateId || Active.length === 0) {
            FailCandidate(CandidateId, 'party candidate no longer active');
            return true;
        }
        const deadline = PartyNow() + DeployTimeoutMs();
        TheParty.Candidate.LaunchDeadline = deadline;
        for (const member of Active) MatchmakingResultMap.get(member)!.LaunchDeadline = deadline;
        const allocationId = crypto.randomUUID();
        MatchmakingRoster.beginAllocation(allocationId, Active.map(playerId => ({playerId, candidateId: CandidateId})), deadline);
        const Game = await LaunchGameOnDeployserver(GameMode, '', HuntId, ['ISLAND','CITY'].includes(GameMode) ? Active : undefined, Region);
        if (PartyNow() >= deadline || (CapacityWaits.get(CandidateId)?.deadline ?? Infinity) <= PartyNow()) {
            FailCandidate(CandidateId, 'allocation deadline expired');
            return true;
        }
        if (Game.capacity) {
            retryMs = Game.retryAfterMs ?? CAPACITY_RETRY_MS;
            MatchmakingRoster.releaseAllocation(allocationId);
            if (TheParty.Candidate?.CandidateId === CandidateId) TheParty.Candidate.LaunchDeadline = undefined;
            for (const member of Active) {
                const entry = MatchmakingResultMap.get(member);
                if (entry?.CandidateId === CandidateId) entry.LaunchDeadline = undefined;
            }
            return false;
        }
        if (Game.succeeded) MatchmakingRoster.assignAllocation(allocationId, Game.sessionId, Game.host, Game.port);
        await FinishPartyCandidate(TheParty, CandidateId, Active, Game);
        return true;
    };
    const Launch = TryLaunch()
        .then(async done => { if (!done && !ParkCapacity(CandidateId, TryLaunch, () => retryMs)) await FinishPartyCandidate(TheParty, CandidateId, Members, {succeeded: false, readyNow: false, host: '', port: 0}); })
        .catch((error) => {
            FailCandidate(CandidateId, 'party allocation failed');
            logger.error(`mm: party candidate ${CandidateId} launch failed: ${(error as Error)?.message}`);
        });

    let Timer: NodeJS.Timeout | undefined;

    await Promise.race([Launch, new Promise<void>((Resolve) => { Timer = setTimeout(Resolve, PARTY_JOIN_WAIT_MS); })]);
    clearTimeout(Timer);
}

function FindRejoinCandidate(TheParty: Party, PlayerId: string, GameMode: string, HuntId: string, IsLeader: boolean){
    for (const candidate of [TheParty.Candidate, TheParty.LastCandidate]) {
        if (candidate?.State !== 'IN_PROGRESS') continue;
        const entry = MatchmakingResultMap.get(PlayerId);
        if (PartyNow() - (candidate.AllocatedAt ?? candidate.CreatedAt) >= LAST_CANDIDATE_REJOIN_MS
            || (entry?.CandidateId === candidate.CandidateId && entry.Failed)) {
            FailCandidate(candidate.CandidateId, 'party allocation reuse expired');
            ClearPartyCandidate(TheParty, candidate.CandidateId, 'party allocation reuse expired');
            if (TheParty.LastCandidate?.CandidateId === candidate.CandidateId) TheParty.LastCandidate = null;
        }
    }
    const Matches = (Candidate: PartyCandidate | null): Candidate is PartyCandidate =>
        Candidate != null && (Candidate.Region ?? 'main') === PlayerRegion(TheParty.LeaderId) && Candidate.GameMode === GameMode && Candidate.HuntId === HuntId && Candidate.MemberIds.includes(PlayerId);

    if(Matches(TheParty.Candidate)){
        return TheParty.Candidate;
    }

    const Last = TheParty.LastCandidate;

    if(!IsLeader && Matches(Last) && Last.State === "IN_PROGRESS" && PartyNow() - (Last.AllocatedAt ?? Last.CreatedAt) <= LAST_CANDIDATE_REJOIN_MS){
        return Last;
    }

    return undefined;
}

// The join is for the candidate the player is already part of: nothing new is started
function RejoinPartyCandidate(TheParty: Party, Candidate: PartyCandidate, PlayerId: string){
    const Entry = MatchmakingResultMap.get(PlayerId);
    const Where = `candidate ${Candidate.CandidateId} of P=${TheParty.PartyId} (${Candidate.HuntId})`;

    if(Entry !== undefined && Entry.PartyCandidate && Entry.CandidateId === Candidate.CandidateId && !Entry.Failed){
        MatchmakingRoster.touchCandidate(PlayerId, Candidate.CandidateId);
        if(!Entry.Ready){
            logger.info(`mm: ${PlayerId} joined again while ${Where} is still starting; nothing new started`);
        }
        else if(Candidate.Served.has(PlayerId) || Entry.ParkedAt !== undefined){
            Entry.ParkedAt ??= PartyNow();
            logger.info(`mm: ${PlayerId} joined again after being sent to ${Entry.Host}:${Entry.Port} for ${Where}; no second server, answering MATCHING`);
        }
        else{
            logger.info(`mm: ${PlayerId} joined again before being sent to ${Where}; nothing new started`);
        }

        return true;
    }

    const Ready = Candidate.State === "IN_PROGRESS" && Candidate.Host !== undefined && Candidate.Port !== undefined;
    const Served = Candidate.Served.has(PlayerId);

    if (Ready && !MatchmakingRoster.restoreAssigned(PlayerId, Candidate.CandidateId, Candidate.HuntId, Candidate.SessionId)) {
        logger.warn(`mm: ${PlayerId} cannot restore their original roster membership for ${Where}`);
        return false;
    }

    LeaveWaitingQueues(PlayerId, "their party's candidate takes them");

    StoreEntry(PlayerId, {
        Ready: Ready,
        CandidateId: Candidate.CandidateId,
        HuntId: Candidate.HuntId,
        Host: Ready ? Candidate.Host! : "",
        Port: Ready ? Candidate.Port! : 0,
        SessionId: Candidate.SessionId,
        AllocatedAt: Candidate.AllocatedAt,
        LaunchDeadline: Ready ? undefined : Candidate.LaunchDeadline,
        FirstSentAt: Candidate.ServedAt?.get(PlayerId) ?? (Served ? Candidate.LastServedAt : undefined),
        PartyCandidate: true,
        PartyMemberIds: [...Candidate.MemberIds],
        ParkedAt: Ready && Served ? PartyNow() : undefined
    });
    logger.info(`mm: ${PlayerId} joined ${Where} again; ${Ready ? (Served ? "already sent there once, answering MATCHING" : "its server is up") : "still starting"}; no second server`);
    return true;
}

// POST /candidate/join/:candidateId. The 1.4.4 client can join a given candidate ("CandidateJoin",
// "/candidate/join/" in the exe); a party member's client may follow the leader's candidate this
// way rather than only polling. Answered for a member of that candidate like their own join of
// it; anyone else gets undefined (404, as before the route existed).
export function JoinPartyCandidateById(PlayerId: string, CandidateId: string): PartyCandidate | undefined {
    const TheParty = PartyForMatchmaking(PlayerId);

    if(TheParty === undefined){
        return undefined;
    }

    const Current = TheParty.Candidate;
    const Last = TheParty.LastCandidate;
    let Candidate: PartyCandidate | undefined;

    if(Current != null && Current.CandidateId === CandidateId){
        Candidate = Current;
    }
    else if(Last != null && Last.CandidateId === CandidateId && Last.State === "IN_PROGRESS" && PartyNow() - (Last.AllocatedAt ?? Last.CreatedAt) <= LAST_CANDIDATE_REJOIN_MS){
        Candidate = Last;
    }

    const existing = MatchmakingResultMap.get(PlayerId);
    if(Candidate === undefined || (Candidate.Region ?? 'main') !== PlayerRegion(TheParty.LeaderId) || !Candidate.MemberIds.includes(PlayerId)
        || (existing?.CandidateId === Candidate.CandidateId && existing.Failed)
        || (Candidate.State === 'IN_PROGRESS' && PartyNow() - (Candidate.AllocatedAt ?? Candidate.CreatedAt) >= LAST_CANDIDATE_REJOIN_MS)){
        return undefined;
    }

    if (!RejoinPartyCandidate(TheParty, Candidate, PlayerId)) return undefined;

    return Candidate;
}

// "solo": not a party matter, use the solo code; true: handled; false: refused (400)
async function HandlePartyMatchmaking(GameMode: unknown, GameArgs: unknown, HuntId: unknown, PlayerId: string): Promise<"solo" | boolean> {
    const TheParty = PartyForMatchmaking(PlayerId);

    if(TheParty === undefined || TheParty.Members.length < 2){
        return "solo";
    }

    const Mode = typeof GameMode === "string" ? GameMode : "";
    const Hunt = typeof HuntId === "string" ? HuntId : "";
    const IsLeader = TheParty.LeaderId === PlayerId;

    if(Mode === "ISLAND" && typeof GameArgs === "string" && GameArgs.trim().length > 0){
        logger.info(`mm: ${PlayerId} (party P=${TheParty.PartyId}) starts a game from its own game args (the tutorial): alone`);
        return "solo";
    }

    let Rejoin = FindRejoinCandidate(TheParty, PlayerId, Mode, Hunt, IsLeader);
    if (Rejoin) {
        const entry = MatchmakingResultMap.get(PlayerId);
        if (entry?.CandidateId === Rejoin.CandidateId && entry.Ready) {
            const usable = await ReadyAllocationUsable(entry);
            if (MatchmakingResultMap.get(PlayerId) !== entry) return true;
            if (!usable) Rejoin = undefined;
        }
    }

    if(Rejoin !== undefined){
        return RejoinPartyCandidate(TheParty, Rejoin, PlayerId);
    }

    if(!IsLeader){
        if(Mode === "CITY" || Mode === "SHARED"){
            logger.info(`mm: ${PlayerId} is not the leader of P=${TheParty.PartyId}; goes to ${Hunt || Mode} alone`);
            return "solo";
        }

        logger.warn(`mm: refusing ${Mode} ${Hunt} for ${PlayerId}: only the leader of P=${TheParty.PartyId} (${TheParty.LeaderId}) starts the party's hunts`);
        return false;
    }

    let Members: string[];

    if(Mode === "ISLAND"){
        if(Hunt.trim().length === 0 || !HuntIdRequiresMatchmaking(Hunt)){
            return "solo";
        }

        Members = [...TheParty.Members];
    }
    else if(Mode === "CITY" || Mode === "SHARED"){
        const LeaderWas = LastSentMap.get(PlayerId);

        Members = TheParty.Members.filter((Member) => {
            const MemberWas = LastSentMap.get(Member);

            return Member === PlayerId || (LeaderWas !== undefined && MemberWas !== undefined
                && MemberWas.Host === LeaderWas.Host && MemberWas.Port === LeaderWas.Port
                && (!(LeaderWas.SessionId || MemberWas.SessionId) || MemberWas.SessionId === LeaderWas.SessionId));
        });

        if(Members.length < 2){
            logger.info(`mm: party leader ${PlayerId} goes to ${Hunt || Mode} alone: no member of P=${TheParty.PartyId} is on the leader's server`);
            return "solo";
        }
    }
    else{
        return "solo";
    }

    await StartPartyCandidate(TheParty, Mode, Hunt, Members, PlayerId);

    return true;
}

export async function HandlePlayerMatchmaking(GameMode: string, GameArgs: string, HuntId: string, PlayerId: string, Private = false){
    return HandlePlayerMatchmakingOnce(GameMode, GameArgs, HuntId, PlayerId, Private);
}

async function HandlePlayerMatchmakingOnce(GameMode: string, GameArgs: string, HuntId: string, PlayerId: string, Private = false){
    const BadInput = CheckMatchmakingInput(GameMode, GameArgs, HuntId);

    if(BadInput != undefined){
        logger.warn(`Refusing matchmaking for ${PlayerId}: ${BadInput}`);

        return false;
    }

    if(MATCHMAKING_MODE === "DISABLED"){
        logger.warn("Matchmaking is disabled, refusing MM!");

        return false;
    }
    else if(MATCHMAKING_MODE === "DEPLOYSERVER"){
        const PartyDecision = await HandlePartyMatchmaking(GameMode, GameArgs, HuntId, PlayerId);

        if(PartyDecision !== "solo"){
            return PartyDecision;
        }

        ForgetPartyCandidateEntry(PlayerId);

        if(HuntId == undefined || HuntId.trim().length == 0 || !HuntIdRequiresMatchmaking(HuntId)){
            // Ramsgate, the Dojo or the tutorial instead of a hunt the player was still queued for
            LeaveWaitingQueues(PlayerId, "their new join replaces it");

            // Accepted party members follow their leader for shared worlds too.
            // Keep the saved personal preference intact for when they leave.
            const WorldParty = (GameMode === 'CITY' || GameMode === 'SHARED') ? GetPartyOf(PlayerId) : undefined;
            const Region = PlayerRegion(WorldParty?.LeaderId ?? PlayerId);
            const key = JSON.stringify([GameMode, GameArgs ?? '', HuntId ?? '', Region]);
            const previous = MatchmakingResultMap.get(PlayerId);
            if (previous?.DirectKey === key && !previous.Failed) {
                MatchmakingRoster.touchCandidate(PlayerId, previous.CandidateId);
                if (!previous.Ready && (previous.LaunchDeadline !== undefined || CapacityWaits.has(previous.CandidateId))) return true;
                if (previous.Ready) {
                    const usable = await ReadyAllocationUsable(previous);
                    if (MatchmakingResultMap.get(PlayerId) !== previous) return true;
                    if (usable) return true;
                }
            }
            const Entry: MatchmakingResult = {
                Ready: false,
                CandidateId: crypto.randomUUID(),
                HuntId: HuntId,
                Host: '',
                Port: 0,
                Region,
                DirectKey: key,
                QueuedAt: PartyNow()
            };
            // Publish the attempt before any network wait. A cancellation or a new
            // hunt can replace it immediately; this request may only finish its own entry.
            StoreEntry(PlayerId, Entry);
            let retryMs = CAPACITY_RETRY_MS;
            const TryLaunch = async () => {
                if (MatchmakingResultMap.get(PlayerId) !== Entry || Entry.Failed) return true;
                const deadline = PartyNow() + DeployTimeoutMs();
                Entry.LaunchDeadline = deadline;
                const allocationId = crypto.randomUUID();
                MatchmakingRoster.beginAllocation(allocationId, [{playerId: PlayerId, candidateId: Entry.CandidateId}], deadline);
                const result = await LaunchGameOnDeployserver(GameMode, GameArgs, HuntId, GameMode === 'CITY' ? [PlayerId] : undefined, Region);
                if (result.succeeded) MatchmakingRoster.assignAllocation(allocationId, result.sessionId, result.host, result.port);
                if (result.capacity) MatchmakingRoster.releaseAllocation(allocationId);
                if (MatchmakingResultMap.get(PlayerId) !== Entry || Entry.Failed) return true;
                if (PartyNow() >= deadline || (CapacityWaits.get(Entry.CandidateId)?.deadline ?? Infinity) <= PartyNow()) {
                    FailCandidate(Entry.CandidateId, 'allocation deadline expired');
                    return true;
                }
                if (result.capacity) {
                    retryMs = result.retryAfterMs ?? CAPACITY_RETRY_MS;
                    Entry.LaunchDeadline = undefined;
                    return false;
                }
                ApplyLaunchResult(PlayerId, Entry, result);
                return true;
            };
            const launch = TryLaunch().then(done => {
                if (!done && MatchmakingResultMap.get(PlayerId) === Entry && !Entry.Failed && !ParkCapacity(Entry.CandidateId, TryLaunch, () => retryMs))
                    FailCandidate(Entry.CandidateId, 'capacity wait unavailable');
            }).catch(error => {
                FailCandidate(Entry.CandidateId, 'direct allocation failed');
                logger.error(`mm: direct candidate ${Entry.CandidateId} launch failed: ${(error as Error)?.message}`);
            });
            await WaitBriefly(launch);

            return true;
        }
        else{
            return await QueuePlayer(HuntId, PlayerId, Private);
        }
    }
    else{
        logger.fatal("Unsupported MATCHMAKING_MODE!");

        return false;
    }
}

// Tests only
export function ResetMatchmakingForTests(){
    MatchmakingRoster.reset();
    HealthCache = undefined;
    HealthRequest = undefined;
    CapacityWaits.clear();
    CapacityOrder.clear();
    MatchmakingQueueMap.clear();
    MatchmakingResultMap.clear();
    PlayerQueueMap.clear();
    LastSentMap.clear();
}
