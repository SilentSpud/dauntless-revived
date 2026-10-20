// Serialize allocation attempts per region until the oldest candidate has a
// result. The worker still owns atomic capacity admission.
export class CapacityTurns {
    private turns = new Map<string, {region:string, queuedAt:number, order:number, seen:number, busy:boolean}>();
    private sequence = 0;
    constructor(private idleMs: number) {}
    touch(id: string, now: number) { const turn=this.turns.get(id); if(turn)turn.seen=now; }
    remove(id: string) { this.turns.delete(id); }
    clear() { this.turns.clear(); }
    start(id: string, region: string, now: number, active: Set<string>, queuedAt = now): boolean {
        for(const [key, turn] of this.turns) {
            if(!active.has(key) || (!turn.busy && now-turn.seen >= this.idleMs)) this.turns.delete(key);
        }
        let turn=this.turns.get(id);
        if(!turn) {turn={region,queuedAt,order:++this.sequence,seen:now,busy:false};this.turns.set(id,turn);}
        if(turn.busy)return false;
        turn.seen=now;
        if([...this.turns.values()].some(other=>other.region===region && other!==turn &&
            (other.busy || other.queuedAt<turn.queuedAt || (other.queuedAt===turn.queuedAt && other.order<turn.order))))return false;
        turn.busy=true;
        return true;
    }
    finish(id: string, capacity: boolean, now: number) {
        const turn=this.turns.get(id);
        if(!turn)return;
        if(capacity){turn.busy=false;turn.seen=now;}
        else this.turns.delete(id);
    }
}
