// Client activity keeps a waiting place alive. A game server asking who is expected
// does not: otherwise an abandoned player would renew their own place forever.
export const QUEUED_PLAYER_LEASE_MS = 45_000;
export const PLAYER_LOADING_GRACE_MS = 180_000;
export const CONNECTED_PLAYER_ABSENCE_GRACE_MS = 45_000;
const MAX_ALLOCATION_WAIT_MS = 300_000;
const ROSTER_RETENTION_MS = 25 * 60 * 60 * 1000;

export type RosterMemberInput = { playerId: string; candidateId: string; huntId?: string };
export type RosterContext = {
    sessionId?: string;
    allocationId?: string;
    // Undefined means no complete, authenticated snapshot was provided. An empty
    // array is different: the native game thread confirmed there are no players.
    connectedPlayerIds?: readonly string[];
};

type RosterMember = RosterMemberInput & {
    lastActivity: number;
    phase: 'queued' | 'allocating' | 'assigned' | 'served' | 'removed';
    allocationId?: string;
    servedAt?: number;
    removalReason?: 'cancelled' | 'replaced';
};

type Allocation = {
    id: string;
    members: Map<string, RosterMember>;
    deadline: number;
    assignedAt?: number;
    sessionId?: string;
    host?: string;
    port?: number;
    lastSeen: number;
    arrived: Set<string>;
    missingSince: Map<string, number>;
};

export class CandidateRoster {
    private current = new Map<string, RosterMember>();
    private allocations = new Map<string, Allocation>();
    private candidateAllocations = new Map<string, Set<string>>();
    private sessions = new Map<string, string>();
    private lastSweep = -Infinity;

    constructor(private now: () => number = Date.now) {}

    setClock(now: () => number) { this.now = now; }

    recordQueued(playerId: string, candidateId: string, huntId = '') {
        this.prune();
        const previous = this.current.get(playerId);
        if (previous?.candidateId === candidateId && previous.phase !== 'removed') {
            previous.lastActivity = this.now();
            return;
        }
        if (previous) {
            previous.phase = 'removed';
            previous.removalReason = 'replaced';
        }
        this.current.set(playerId, { playerId, candidateId, huntId, lastActivity: this.now(), phase: 'queued' });
    }

    touchCandidate(playerId: string, candidateId: string) {
        const member = this.current.get(playerId);
        if (!member || member.candidateId !== candidateId || member.phase === 'removed') return false;
        member.lastActivity = this.now();
        return true;
    }

    queueMemberActive(playerId: string, candidateId: string) {
        const member = this.current.get(playerId);
        return !!member && member.candidateId === candidateId && member.phase === 'queued'
            && this.now() - member.lastActivity <= QUEUED_PLAYER_LEASE_MS;
    }

    beginAllocation(allocationId: string, members: RosterMemberInput[], deadline = this.now() + MAX_ALLOCATION_WAIT_MS) {
        this.prune();
        const roster = new Map<string, RosterMember>();
        for (const input of members) {
            const member = this.current.get(input.playerId);
            if (!member || member.candidateId !== input.candidateId || member.phase === 'removed') continue;
            member.phase = 'allocating';
            member.allocationId = allocationId;
            roster.set(input.playerId, member);
        }
        this.allocations.set(allocationId, {
            id: allocationId, members: roster, deadline, lastSeen: this.now(),
            arrived: new Set(), missingSince: new Map()
        });
        for (const member of roster.values()) {
            const ids = this.candidateAllocations.get(member.candidateId) ?? new Set<string>();
            ids.add(allocationId);
            this.candidateAllocations.set(member.candidateId, ids);
        }
    }

    assignAllocation(allocationId: string, sessionId?: string, host?: string, port?: number) {
        const allocation = this.allocations.get(allocationId);
        if (!allocation) return;
        allocation.assignedAt = this.now();
        allocation.lastSeen = this.now();
        allocation.sessionId = sessionId;
        allocation.host = host;
        allocation.port = port;
        if (sessionId) this.sessions.set(sessionId, allocationId);
        for (const member of allocation.members.values()) {
            if (this.current.get(member.playerId) === member && member.allocationId === allocationId && member.phase !== 'removed')
                member.phase = 'assigned';
        }
    }

    // An explicit capacity refusal did not start a server. Restore its candidates
    // without granting new activity to members who stopped polling during the wait.
    releaseAllocation(allocationId: string) {
        const allocation = this.allocations.get(allocationId);
        if (!allocation) return;
        for (const member of allocation.members.values()) {
            if (this.current.get(member.playerId) === member && member.allocationId === allocationId && member.phase !== 'removed') {
                member.phase = 'queued';
                delete member.allocationId;
            }
        }
        this.deleteAllocation(allocation);
    }

    served(playerId: string, candidateId: string) {
        const member = this.current.get(playerId);
        if (!member || member.candidateId !== candidateId || member.phase === 'removed') return;
        member.servedAt ??= this.now();
        member.phase = 'served';
    }

    removePlayer(playerId: string, candidateId: string, reason: 'cancelled' | 'replaced' = 'cancelled') {
        const member = this.current.get(playerId);
        const remove = (entry: RosterMember) => {
            entry.phase = 'removed';
            if (entry.removalReason !== 'cancelled') entry.removalReason = reason;
        };
        if (member?.candidateId === candidateId) {
            remove(member);
            this.current.delete(playerId);
        }
        for (const id of this.candidateAllocations.get(candidateId) ?? []) {
            const original = this.allocations.get(id)?.members.get(playerId);
            if (original?.candidateId === candidateId) remove(original);
        }
    }

    // Reconstructing a ready party candidate must restore its original membership,
    // not create an unrelated queued record. A cancelled membership cannot return
    // through the party's old candidate. Session identity prevents port-reuse mixups.
    restoreAssigned(playerId: string, candidateId: string, huntId: string, sessionId?: string) {
        // A pooled city session can host several different candidate allocations.
        // Restore by candidate provenance before narrowing by native session.
        const matches = [...(this.candidateAllocations.get(candidateId) ?? [])]
            .map(id => this.allocations.get(id)!)
            .filter(allocation => {
                const member = allocation.members.get(playerId);
                return member?.candidateId === candidateId && member.huntId === huntId
                    && member.allocationId === allocation.id && allocation.assignedAt !== undefined
                    && (sessionId === undefined || allocation.sessionId === sessionId);
            });
        if (matches.length !== 1) return false;
        const allocation = matches[0];
        const member = allocation.members.get(playerId);
        if (allocation.assignedAt === undefined || !member || member.candidateId !== candidateId || member.huntId !== huntId
            || member.allocationId !== allocation.id || member.removalReason === 'cancelled') return false;
        const previous = this.current.get(playerId);
        if (previous && previous !== member) {
            previous.phase = 'removed';
            previous.removalReason = 'replaced';
        }
        member.phase = member.servedAt === undefined ? 'assigned' : 'served';
        delete member.removalReason;
        member.lastActivity = this.now();
        allocation.lastSeen = this.now();
        this.current.set(playerId, member);
        return true;
    }

    cancelCandidate(candidateId: string) {
        for (const member of this.current.values()) {
            if (member.candidateId === candidateId) this.removePlayer(member.playerId, candidateId);
        }
        // A replaced member can still belong to an in-flight or previously assigned
        // snapshot; cancelling that candidate must invalidate it as well.
        for (const id of this.candidateAllocations.get(candidateId) ?? []) {
            for (const member of this.allocations.get(id)?.members.values() ?? []) {
                if (member.candidateId === candidateId) {
                    member.phase = 'removed';
                    member.removalReason = 'cancelled';
                }
            }
        }
    }

    reconcileExpected(playerIds: string[], context: RosterContext = {}) {
        const allocation = this.findAllocation(context);
        // Legacy workers and workers surviving a metagame restart cannot be
        // identified safely. Missing memory is not evidence that players left.
        if (!allocation) return playerIds;
        allocation.lastSeen = this.now();
        const connected = context.connectedPlayerIds === undefined ? undefined : new Set(context.connectedPlayerIds);
        // An incomplete snapshot says nothing about absence. Require fresh,
        // continuous complete evidence before removing a previously arrived player.
        if (!connected) allocation.missingSince.clear();
        return playerIds.filter(playerId => {
            const member = allocation.members.get(playerId);
            if (!member) return true;
            if (connected?.has(playerId)) {
                allocation.arrived.add(playerId);
                allocation.missingSince.delete(playerId);
                return true;
            }
            if (allocation.arrived.has(playerId)) {
                if (!connected) return true;
                const missingSince = allocation.missingSince.get(playerId) ?? this.now();
                allocation.missingSince.set(playerId, missingSince);
                if (this.now() - missingSince <= CONNECTED_PLAYER_ABSENCE_GRACE_MS) return true;
            }
            if (member.phase === 'removed' || this.current.get(playerId) !== member || member.allocationId !== allocation.id) return false;
            // ExpectedPlayerStatus sends the entire original roster, including
            // players already connected. Silence alone cannot identify a no-show.
            if (!connected) return true;
            if (allocation.assignedAt === undefined) return this.now() <= allocation.deadline;
            const loadingStarted = member.servedAt ?? allocation.assignedAt;
            return this.now() - loadingStarted <= PLAYER_LOADING_GRACE_MS
                || this.now() - member.lastActivity <= QUEUED_PLAYER_LEASE_MS;
        });
    }

    reset() {
        this.current.clear();
        this.allocations.clear();
        this.candidateAllocations.clear();
        this.sessions.clear();
        this.lastSweep = -Infinity;
    }

    private findAllocation(context: RosterContext) {
        const id = context.sessionId ? this.sessions.get(context.sessionId) : context.allocationId;
        return id ? this.allocations.get(id) : undefined;
    }

    private deleteAllocation(allocation: Allocation) {
        this.allocations.delete(allocation.id);
        for (const member of allocation.members.values()) {
            const ids = this.candidateAllocations.get(member.candidateId);
            ids?.delete(allocation.id);
            if (ids?.size === 0) this.candidateAllocations.delete(member.candidateId);
        }
        if (allocation.sessionId && this.sessions.get(allocation.sessionId) === allocation.id)
            this.sessions.delete(allocation.sessionId);
    }

    private prune() {
        if (this.now() - this.lastSweep < 30_000) return;
        this.lastSweep = this.now();
        for (const allocation of this.allocations.values()) {
            if (this.now() - allocation.lastSeen > ROSTER_RETENTION_MS) this.deleteAllocation(allocation);
        }
        for (const member of this.current.values()) {
            if (this.now() - member.lastActivity > ROSTER_RETENTION_MS && (!member.allocationId || !this.allocations.has(member.allocationId)))
                this.current.delete(member.playerId);
        }
    }
}

export const MatchmakingRoster = new CandidateRoster();
