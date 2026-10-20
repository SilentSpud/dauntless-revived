import os from 'node:os';
import { existsSync } from 'node:fs';

export class CapacityUnavailable extends Error {
    constructor(public readonly reason: 'memory' | 'ports' | 'hunts' | 'cpu') { super(`Game-server capacity unavailable: ${reason}`); }
}

export function HuntLimit() {
    const name = process.env.HUNT_WORKER === '1' ? 'MAX_HUNTS' : 'MAX_LOCAL_HUNTS';
    if (process.env[name] === 'auto') {
        if (process.env.GAMESERVER_CPU_GUARD !== '1' || process.env.GAMESERVER_MEMORY_GUARD === '0')
            throw new Error('Automatic hunt capacity requires CPU and memory guards');
        const first = setting('PORT_RANGE_BEGIN', 1), last = setting('PORT_RANGE_END', 1);
        if (last > 65535 || last - first < 2) throw new Error('Invalid automatic hunt port range');
        // Two ports remain reserved for Ramsgate and Training. CPU and RAM admission
        // decide how many of the remaining ports can actually run at any moment.
        return last - first - 1;
    }
    return process.env[name] === undefined ? null : setting(name, 1);
}

// Launches start one at a time, SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP apart, and the coordinator gives up
// on a launch after 120 seconds. Half of that is the budget for the line, so the last one in line still starts in time
const PENDING_BUDGET_SECONDS = 60;

// While this file exists no new hunt is admitted; running ones finish normally. Create it to drain a host
// before a restart, delete it to resume. Checked on every request, so no restart is needed to toggle it.
export function HuntsDraining() {
    return existsSync(process.env.DRAIN_FILE || 'drain.flag');
}

export function PendingHuntLimit() {
    const wanted = process.env.MAX_PENDING_HUNTS === undefined ? 8 : setting('MAX_PENDING_HUNTS', 8);
    const pacing = Math.max(1, Number(process.env.SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP) || 1);
    return Math.max(1, Math.min(wanted, Math.floor(PENDING_BUDGET_SECONDS / pacing)));
}

export class HuntAdmission {
    pending = 0;
    constructor(private running: () => number, private limit = HuntLimit) {}
    status() { return {running: this.running(), pending: this.pending, limit: this.limit()}; }
    reserve() {
        const {running, pending, limit} = this.status();
        // Keep paced launches within the caller's request timeout instead of spawning abandoned hunts.
        if (HuntsDraining()) throw new CapacityUnavailable('hunts');
        if (pending >= PendingHuntLimit()) throw new CapacityUnavailable('hunts');
        if (limit !== null && running + pending >= limit) throw new CapacityUnavailable('hunts');
        this.pending++;
        let active = true;
        return () => { if (active) {active = false; this.pending--;} };
    }
}

function setting(name: string, fallback: number) {
    const value = process.env[name];
    if (value === undefined) return fallback;
    const number = Number(value);
    if (!/^\d+$/.test(value) || !Number.isSafeInteger(number) || number < 1) throw new Error(`Invalid ${name}`);
    return number;
}

// Reserve estimated startup memory synchronously before spawn: concurrent requests cannot all
// claim the same free RAM before Windows has charged the new processes for their allocations.
export class MemoryAdmission {
    private reservations = new Map<symbol, {until: number, bytes: number}>();
    constructor(private free = () => os.freemem(), private now = () => Date.now()) {}
    reserve(temporaryHunt = false) {
        if (process.env.GAMESERVER_MEMORY_GUARD === '0') return () => {};
        const floor = setting('GAMESERVER_MIN_FREE_MB', 3072) * 1048576;
        const standardMB = setting('GAMESERVER_STARTUP_MB', 1536);
        const cost = (temporaryHunt ? setting('GAMESERVER_HUNT_STARTUP_MB', standardMB) : standardMB) * 1048576;
        const hold = setting('GAMESERVER_RESERVATION_SECONDS', 60) * 1000;
        const now = this.now();
        for (const [key, reservation] of this.reservations) if (reservation.until <= now) this.reservations.delete(key);
        const available = this.free();
        const reserved = [...this.reservations.values()].reduce((sum, reservation) => sum + reservation.bytes, 0);
        if (!Number.isFinite(available) || available - reserved < floor + cost) throw new CapacityUnavailable('memory');
        const token = Symbol();
        this.reservations.set(token, {until: now + hold, bytes: cost});
        return () => { this.reservations.delete(token); };
    }
}
export const memoryAdmission = new MemoryAdmission();
