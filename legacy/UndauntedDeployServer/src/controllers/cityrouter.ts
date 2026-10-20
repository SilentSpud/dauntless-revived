import { CapacityUnavailable } from './capacity';

type Connection = {host: string, port: number};
type Side = 'local' | 'remote';
// Short reservations cover the status cache and players still travelling.
export class CityRouter {
    private reservations: {side: Side, size: number, until: number}[] = [];
    constructor(private load: () => Promise<Record<Side, number>>, private now = Date.now) {}
    async launch(size: number, local: () => Promise<Connection>, remote: () => Promise<Connection | undefined>, limit = 28) {
        if (!Number.isInteger(limit) || limit < 1 || limit > 32) throw new Error('Invalid RAMSGATE_PLAYER_LIMIT');
        size = Math.max(1, size);
        const counts = {...await this.load()};
        if (![counts.local, counts.remote].every(n => Number.isFinite(n) && n >= 0)) throw new Error('Invalid city occupancy');
        this.reservations = this.reservations.filter(r => r.until > this.now());
        for (const r of this.reservations) counts[r.side] += r.size;
        const order: Side[] = counts.local <= counts.remote ? ['local', 'remote'] : ['remote', 'local'];
        for (const side of order) {
            if (counts[side] + size > limit) continue;
            const reservation = {side, size, until: Infinity};
            this.reservations.push(reservation);
            try {
                const result = await (side === 'local' ? local() : remote());
                if (result) { reservation.until = this.now() + 45000; return result; }
            } catch (error) {
                if (!(error instanceof CapacityUnavailable)) throw error;
            } finally {
                if (reservation.until === Infinity) this.reservations = this.reservations.filter(r => r !== reservation);
            }
        }
        throw new CapacityUnavailable('hunts');
    }
}
