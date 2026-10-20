import os from 'node:os';
import { CapacityUnavailable } from './capacity';

// Smoothed CPU with hysteresis: refuse new processes, never evict active hunts.
export class CpuAdmission {
    private previous?: {idle:number,total:number};
    private percent: number | null = null;
    private blocked = false;
    constructor(private limit=80, private resume=65) {
        if (!(limit > 0 && limit <= 100 && resume >= 0 && resume < limit)) throw new Error('Invalid CPU admission thresholds');
    }
    sample(idle:number,total:number) {
        const old=this.previous; this.previous={idle,total};
        if (!old || total<=old.total || idle<old.idle) return;
        const used=Math.max(0,Math.min(100,100*(1-(idle-old.idle)/(total-old.total))));
        this.percent=this.percent===null?used:this.percent*0.5+used*0.5;
        if(this.percent>=this.limit)this.blocked=true;
        else if(this.percent<=this.resume)this.blocked=false;
    }
    status(){return {percent:this.percent,blocked:this.blocked,limit:this.limit,resume:this.resume};}
    assertAvailable(){if(this.blocked)throw new CapacityUnavailable('cpu');}
}
export const cpuAdmission=new CpuAdmission(Number(process.env.GAMESERVER_CPU_LIMIT_PERCENT??80),Number(process.env.GAMESERVER_CPU_RESUME_PERCENT??65));
function sample(){let idle=0,total=0;for(const cpu of os.cpus()){idle+=cpu.times.idle;total+=Object.values(cpu.times).reduce((a,b)=>a+b,0);}cpuAdmission.sample(idle,total);}
sample();setInterval(sample,2000).unref();
export function CheckCpuAdmission(){if(process.env.GAMESERVER_CPU_GUARD==='1')cpuAdmission.assertAvailable();}
