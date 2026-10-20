import {CapacityUnavailable} from './capacity';
import { logger } from "../logger";
import { GetRamsgateConnectionDetails, GetTrainingDojoConnectionDetails, StartupGameserverWithArgs, StartupGameserverWithHuntIdAndPlayers } from "./gameservers";
import { Gameservers, KindOfGameserver, huntAdmission, DescribeGameservers, CityPoolEnabled } from './gameservers';
import { HuntRouter, RemoteLaunch, OverflowUrl, DescribeOverflow } from './overflow';
import { CityRouter } from './cityrouter';
import { NativeOccupancy } from './nativeoccupancy';
import { RegionalRouter, RegionChoice } from './regions';

const Regions = new RegionalRouter(async () => {
    const local = huntAdmission.status();
    const url = OverflowUrl();
    if (!url) return local;
    try {
        const response = await fetch(new URL('/gameservers', url), {signal:AbortSignal.timeout(2000),redirect:'error'});
        const body = await response.json() as any;
        if (!response.ok || !body.capacity) return local;
        return {running:local.running + body.capacity.running, pending:local.pending + body.capacity.pending,
            limit:local.limit !== null && body.capacity.limit !== null ? local.limit + body.capacity.limit : null};
    } catch { return local; }
});

const Cities = new CityRouter(async () => {
    const key = process.env.CITY_STATUS_KEY;
    if (!key) throw new Error('CITY_STATUS_KEY is required for Ramsgate balancing');
    const [response, worker] = await Promise.all([
        fetch('http://127.0.0.1:61000/undaunted/api/ServerStatus', {
            headers: {'x-undaunted-user-api-key': key}, signal: AbortSignal.timeout(5000)
        }), DescribeOverflow()
    ]);
    const status = await response.json() as any;
    if (!response.ok || status.limited || !Array.isArray(status.instances)) throw new Error('City occupancy unavailable');
    const count = (servers: {id:string,kind:string,connectedPlayers?:number}[]) => {
        const city = servers.find(s => s.kind === 'city');
        return city ? Number(city.connectedPlayers ?? status.instances.find((i:any) => i.id === city.id)?.players ?? 0) : 0;
    };
    const city=Gameservers.find(s=>s.isRamsgate);
    return {local: (city ? NativeOccupancy(city.processId,city.startTime) : undefined) ?? count(DescribeGameservers()), remote: count(worker)};
});

const Router = new HuntRouter(() => Gameservers.filter(server => ['hunt', 'tutorial'].includes(KindOfGameserver(server))).length, undefined, () => huntAdmission.status());

export async function HandleMatchmakingRequest(GameMode: string, GameArgs: string, HuntId: string, ExpectedPlayers: string[] | undefined, Region: RegionChoice = 'main', Overflow = false){
    if(Overflow && process.env.HUNT_WORKER === '1' && GameMode === 'ISLAND'){
        const {running,pending,limit}=huntAdmission.status();
        const reserved=Number(process.env.AUS_RESERVED_HUNTS??0);
        if(!Number.isSafeInteger(reserved)||reserved<0)throw new Error('Invalid AUS_RESERVED_HUNTS');
        if(limit!==null && running+pending>=Math.max(0,limit-reserved))throw new CapacityUnavailable('hunts');
    }
    if (process.env.HUNT_WORKER === '1' && GameMode !== 'ISLAND' && !(GameMode === 'CITY' && process.env.WORKER_RAMSGATE === '1') && !(GameMode === 'SHARED' && process.env.WORKER_DOJO === '1')) throw new Error('Hunt worker does not accept this world');
    const body = {GameMode, GameArgs, HuntId, ExpectedPlayers};
    return Regions.launch(body, Region, async () => {
        if (GameMode === 'CITY' && !CityPoolEnabled() && process.env.CITY_OVERFLOW === '1') {
            const url = OverflowUrl();
            if (!url) throw new Error('City overflow needs the worker tunnel');
            return Cities.launch(ExpectedPlayers?.length ?? 1,
                () => GetRamsgateConnectionDetails(ExpectedPlayers?.length ?? 1),
                () => RemoteLaunch(url, body),
                Number(process.env.RAMSGATE_PLAYER_LIMIT ?? 28));
        }
        return Router.launch(body, () => HandleLocalMatchmaking(GameMode, GameArgs, HuntId, ExpectedPlayers));
    });
}

async function HandleLocalMatchmaking(GameMode: string, GameArgs: string, HuntId: string, ExpectedPlayers: string[] | undefined){
    logger.info(`Handling matchmaking with GameMode: ${GameMode} HuntId: ${HuntId} and GameArgs: ${GameArgs} and ExpectedPlayers ${ExpectedPlayers}`);

    if(GameMode === "CITY"){
        return await GetRamsgateConnectionDetails(ExpectedPlayers?.length ?? 1);
    }
    else if(GameMode === "SHARED"){
        if (HuntId != undefined && HuntId.trim().length > 0){
            if(HuntId == "ShatteredIsles_TrainingDojo"){
                return await GetTrainingDojoConnectionDetails();
            }
        }
    }
    else if(GameMode === "ISLAND"){
        if(GameArgs != undefined && GameArgs.trim().length > 0){
            return await StartupGameserverWithArgs(GameArgs);
        }

        if(HuntId != undefined && HuntId.trim().length > 0 && ExpectedPlayers != undefined){
            return await StartupGameserverWithHuntIdAndPlayers(HuntId, ExpectedPlayers!);
        }
    }

    if (process.env.HUNT_WORKER === '1') throw new Error('No hunt could be resolved on worker');
    logger.error("Matchmaking failed, sending you to Ramsgate!");

    return await GetRamsgateConnectionDetails(ExpectedPlayers?.length ?? 1);
}
