import {CapacityUnavailable} from './capacity';

export const CITY_PLAYER_LIMIT = 20;
// RAMSGATE_CITY_CAPS="10,10,8": at most that many cities, filled in the order they started (the oldest first),
// each admitting up to its own cap. Unset keeps the default: cities of CITY_PLAYER_LIMIT, started on demand.
export function CityCaps(): number[] | undefined {
    const raw = process.env.RAMSGATE_CITY_CAPS;
    if (raw === undefined || raw.trim() === '') return undefined;
    const caps = raw.split(',').map(v => Number(v.trim()));
    if (!caps.every(n => Number.isInteger(n) && n >= 1 && n <= CITY_PLAYER_LIMIT)) throw new Error('Invalid RAMSGATE_CITY_CAPS');
    return caps;
}
export type City = {id: string, players: number | undefined, startedAt: number};
// Serialize allocation through readiness so concurrent requests share a cold start.
export class CityPool<T extends {id:string}> {
    private retiring = new Set<string>();
    private lastPlayers = new Map<string, number>();
    private queue: Promise<unknown> = Promise.resolve();
    private reservations = new Map<string, {size:number, until:number}[]>();
    constructor(private list: () => City[], private start: () => Promise<T>, private get: (id:string) => T,
        private now = Date.now, private caps: () => number[] | undefined = CityCaps) {}
    allocate(size = 1): Promise<T> {
        if (!Number.isInteger(size) || size < 1 || size > CITY_PLAYER_LIMIT) return Promise.reject(new CapacityUnavailable('hunts'));
        const work = this.queue.catch(() => {}).then(async () => {
            const cities = this.list().slice().sort((a, b) => a.startedAt - b.startedAt);
            const caps = this.caps();
            let slot = 0;
            for (const id of this.reservations.keys()) if (!cities.some(c => c.id === id)) { this.reservations.delete(id); this.lastPlayers.delete(id); this.retiring.delete(id); }
            let chosen: T | undefined;
            for (const city of cities) {
                if (this.retiring.has(city.id)) continue;
                const leases = (this.reservations.get(city.id) ?? []).filter(r => r.until > this.now());
                if (city.players !== undefined) {
                    let arrivals = Math.max(0, city.players - (this.lastPlayers.get(city.id) ?? 0));
                    for (const lease of leases) {
                        const consumed = Math.min(arrivals, lease.size);
                        lease.size -= consumed;
                        arrivals -= consumed;
                    }
                    // A city that just emptied is being retired by its native watchdog.
                    const retiring = city.players === 0 && (this.lastPlayers.get(city.id) ?? 0) > 0;
                    this.lastPlayers.set(city.id, city.players);
                    this.reservations.set(city.id, leases.filter(r => r.size > 0));
                    if (retiring) { this.retiring.add(city.id); continue; }
                } else this.reservations.set(city.id, leases);
                // Missing telemetry is not proof of free seats. For a fresh launch, all
                // admissions are still reserved; otherwise leave an unknown city alone.
                const occupied = city.players ?? (this.now() - city.startedAt < 180000 && leases.length > 0 ? 0 : CITY_PLAYER_LIMIT);
                const limit = caps ? (caps[slot] ?? 0) : CITY_PLAYER_LIMIT;
                slot++;
                if (occupied + leases.reduce((n,r) => n+r.size, 0) + size <= limit) {
                    chosen = this.get(city.id);
                    break;
                }
            }
            if (!chosen && caps && slot >= caps.length) throw new CapacityUnavailable('hunts');
            chosen ??= await this.start();
            const leases = this.reservations.get(chosen.id) ?? [];
            leases.push({size, until:this.now() + 180000});
            this.reservations.set(chosen.id, leases);
            return chosen;
        });
        this.queue = work;
        return work;
    }
    reset() { this.queue = Promise.resolve(); this.reservations.clear(); this.lastPlayers.clear(); this.retiring.clear(); }
}
