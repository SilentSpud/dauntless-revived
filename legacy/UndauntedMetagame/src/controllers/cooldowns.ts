import { and, eq } from "drizzle-orm";
import { GetDb } from "../db";
import { cooldowns } from "../db/schema";
import { logger } from "../logger";
import { Caller, IsPlainObject, ReadField, RealReply, RecordProgressionEvent } from "./progressionevents";
import { Tx } from "./savehistory";

// Cooldowns of real-mode accounts. A cooldown is an opaque id (bounty_tokens_<hunt
// pass>, gatherable tracking GUIDs, Trials rewards, Blueprint ids) and an ABSOLUTE
// start time as an ISO-8601 string; durations and resets live in the client.

const GATHERABLE_GUID = /^[0-9A-F]{32}$/;
const GATHERABLE_KEEP_MS = 24 * 60 * 60 * 1000;

function CooldownMap(tx: Tx, AccountId: string){
    const Map: Record<string, string> = {};

    for(const Row of tx.select().from(cooldowns).where(eq(cooldowns.accountId, AccountId)).all()){
        Map[Row.cooldownId] = Row.startedDate;
    }

    return Map;
}

function Upsert(tx: Tx, AccountId: string, CooldownId: string, StartedDate: string){
    const Now = new Date().toISOString();

    tx.insert(cooldowns).values({accountId: AccountId, cooldownId: CooldownId, startedDate: StartedDate, updatedDate: Now})
        .onConflictDoUpdate({target: [cooldowns.accountId, cooldowns.cooldownId], set: {startedDate: StartedDate, updatedDate: Now}}).run();
}

// Harvest cooldowns are only read back for RespawnGatherableTimerMinutes; old ones just pile up
function PruneGatherables(tx: Tx, AccountId: string){
    const Cutoff = Date.now() - GATHERABLE_KEEP_MS;

    for(const Row of tx.select().from(cooldowns).where(eq(cooldowns.accountId, AccountId)).all()){
        const Started = Date.parse(Row.startedDate);

        if(GATHERABLE_GUID.test(Row.cooldownId) && !Number.isNaN(Started) && Started < Cutoff){
            tx.delete(cooldowns).where(and(eq(cooldowns.accountId, AccountId), eq(cooldowns.cooldownId, Row.cooldownId))).run();
        }
    }
}

// GET /cooldown/:uid: every cooldown of the account in one object (both game-server readers share it)
export function GetCooldownReply(AccountId: string){
    const Map = GetDb().transaction((tx) => CooldownMap(tx, AccountId));

    return {code: 200, message: "OK", payload: Map};
}

// PUT /cooldown/batch/:uid {"cooldowns": [{cooldown_id, cooldown_started_date}]}. Stored as sent.
export function SetCooldownBatch(AccountId: string, Body: unknown, Who: Caller): RealReply{
    const Route = "PUT /cooldown/batch/:uid";
    const Notes: string[] = [];
    let Entries: {Id: unknown, Started: unknown}[] | undefined;

    if(IsPlainObject(Body) && Array.isArray(Body.cooldowns)){
        Entries = Body.cooldowns.map((Entry: unknown) => ({
            Id: ReadField(Entry, ["cooldown_id", "cooldownId", "id"], "cooldown id", Notes),
            Started: ReadField(Entry, ["cooldown_started_date", "cooldown_start_date", "startedDate", "started_date"], "cooldown start", Notes)
        }));
    }
    else if(IsPlainObject(Body) && Body.cooldowns === undefined && Object.values(Body).every((Value) => typeof Value === "string")){
        Notes.push("body is an id -> date map, not {cooldowns: [...]}");
        Entries = Object.entries(Body).map(([Id, Started]) => ({Id: Id, Started: Started}));
    }

    return GetDb().transaction((tx) => {
        if(Entries == undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 400, Notes: [...Notes, "no cooldowns array"]});
            logger.warn(`Refusing cooldown batch for ${AccountId}: no cooldowns array`);
            return {Status: 400};
        }

        for(const Entry of Entries){
            if(typeof Entry.Id !== "string" || Entry.Id.length === 0){
                Notes.push(`skipped entry without an id: ${JSON.stringify(Entry)}`);
                continue;
            }

            let Started = Entry.Started;

            if(typeof Started !== "string" || Started.length === 0){
                Notes.push(`${Entry.Id} has no start date, stored now`);
                Started = new Date().toISOString();
            }
            else if(Number.isNaN(Date.parse(Started as string))){
                Notes.push(`${Entry.Id} start ${Started} does not parse as a date, stored as sent`);
            }

            Upsert(tx, AccountId, Entry.Id, Started as string);
        }

        const Reply = {code: 200, message: "OK", payload: CooldownMap(tx, AccountId)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Notes});

        if(Notes.length > 0){
            logger.warn(`Cooldown batch for ${AccountId}: ${Notes.join("; ")}`);
        }

        logger.info(`Stored ${Entries.length} cooldown(s) for ${AccountId}`);

        return {Status: 200, Body: Reply};
    });
}

// PUT /cooldown/:uid/:id, no body: the cooldown starts now
export function StartCooldown(AccountId: string, CooldownId: string, Who: Caller): RealReply{
    const Route = "PUT /cooldown/:uid/:id";

    return GetDb().transaction((tx) => {
        PruneGatherables(tx, AccountId);
        Upsert(tx, AccountId, CooldownId, new Date().toISOString());
        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 200, Notes: [`cooldown ${CooldownId}`]});
        logger.info(`Started cooldown ${CooldownId} for ${AccountId}`);

        return {Status: 200, Body: {}};
    });
}
