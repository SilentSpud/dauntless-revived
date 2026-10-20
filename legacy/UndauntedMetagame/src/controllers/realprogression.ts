import { and, asc, desc, eq, exists, inArray, notExists, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { characters, huntpassselection, objectives, progressionevents, progresstracks, users } from "../db/schema";
import { logger } from "../logger";
import { HasActiveEntitlement } from "./entitlements";
import { IsProgressionModeStub } from "./progressionmode";
import { Caller, DoesAccountExist, IsPlainObject, ParsePathInteger, ReadField, ReadInteger, RealReply, RecordProgressionEvent } from "./progressionevents";
import { ComputeEarnedRanks, GetProgressionPath, GetProgressionPaths, INT32_MAX, MaxRankId, PremiumGatingEntitlement, ProgressionPath, TotalXpToMaxRank } from "./progressionrank";
import { Tx } from "./savehistory";
import { GetActiveHuntPass } from "./progressionconfig";
import { ProgressionConfirmEntitlements, ProgressionReplayWindow } from "../features";
import { GrantEntitlementInTx } from "./entitlements";

// Progression of real-mode accounts. Wire shapes are from the 1.4.4 client
// (C:\dr\data\plans\m2\progression.md). The rules that matter:
// - phx_account_id is always the account in the URL; the game server keys its
//   cache by it, and a wrong one re-grants every rank reward from rank 0.
// - Track progress in a grant request is an increment; every reply carries the new total.
// - The backend grants no items. The game server grants rank rewards itself through
//   /inventory from the difference between its cached track and our reply, and
//   /confirm only records that ranks were claimed.

export type TrackRecord = {
    phx_account_id: string,
    progression_id: string,
    progress: number,
    confirmed_fremium_rank: number,
    confirmed_premium_rank: number,
    confirmed_date: string
};

export type ObjectiveRecord = {
    phx_account_id: string,
    objective_id: string,
    progress: number,
    completed_count: number,
    created_date: string,
    last_modified_date: string
};

export type SeedMode = "grandfather" | "fresh";

const DEFAULT_CONFIRMED_DATE = "1970-01-01T00:00:00.000Z";
const DEFAULT_GRANT_CAP = 5000;

type TrackRow = typeof progresstracks.$inferSelect;
type ObjectiveRow = typeof objectives.$inferSelect;

// Largest amount one request may add to one track; anything above is cut and logged
export function GetGrantCap(){
    const Cap = Number(process.env.PROGRESSION_GRANT_CAP);

    return Number.isInteger(Cap) && Cap > 0 ? Cap : DEFAULT_GRANT_CAP;
}

function ToTrackRecord(AccountId: string, ProgressionId: string, Row: TrackRow | undefined): TrackRecord{
    return {
        phx_account_id: AccountId,
        progression_id: ProgressionId,
        progress: Row?.progress ?? 0,
        confirmed_fremium_rank: Row?.confirmedFreeRank ?? 0,
        confirmed_premium_rank: Row?.confirmedPremiumRank ?? 0,
        confirmed_date: Row?.confirmedDate ?? DEFAULT_CONFIRMED_DATE
    };
}

function ToObjectiveRecord(AccountId: string, Row: ObjectiveRow): ObjectiveRecord{
    return {
        phx_account_id: AccountId,
        objective_id: Row.objectiveId,
        progress: Row.progress,
        completed_count: Row.completedCount,
        created_date: Row.createdDate,
        last_modified_date: Row.lastModifiedDate
    };
}

function TrackFor(AccountId: string, ProgressionId: string){
    return and(eq(progresstracks.accountId, AccountId), eq(progresstracks.progressionId, ProgressionId));
}

function ReadTrack(tx: Tx, AccountId: string, ProgressionId: string){
    return tx.select().from(progresstracks).where(TrackFor(AccountId, ProgressionId)).get();
}

function WriteTrack(tx: Tx, AccountId: string, ProgressionId: string, Values: {progress: number, confirmedFreeRank: number, confirmedPremiumRank: number, confirmedDate: string}){
    const UpdatedDate = new Date().toISOString();

    return tx.insert(progresstracks).values({accountId: AccountId, progressionId: ProgressionId, ...Values, updatedDate: UpdatedDate})
        .onConflictDoUpdate({target: [progresstracks.accountId, progresstracks.progressionId], set: {...Values, updatedDate: UpdatedDate}})
        .returning().get();
}

function EarnedFree(ProgressionId: string, Progress: number){
    const Path = GetProgressionPath(ProgressionId);

    return Path == undefined ? undefined : ComputeEarnedRanks(Path, Progress, false).EarnedFreeRank;
}

function WarnNotes(What: string, Notes: string[]){
    if(Notes.length > 0){
        logger.warn(`${What}: ${Notes.join("; ")}`);
    }
}

// Every configured track (stored values, or progress 0 and ranks 0), then any
// stored track the config doesn't know
export function GetTrackRecords(AccountId: string): TrackRecord[]{
    const Rows = GetDb().select().from(progresstracks).where(eq(progresstracks.accountId, AccountId)).all();
    const ByTrack = new Map(Rows.map((Row) => [Row.progressionId, Row]));

    const Records = GetProgressionPaths().map((Path) => ToTrackRecord(AccountId, Path.progression_id, ByTrack.get(Path.progression_id)));

    for(const Row of Rows){
        if(GetProgressionPath(Row.progressionId) == undefined){
            Records.push(ToTrackRecord(AccountId, Row.progressionId, Row));
        }
    }

    return Records;
}

// undefined for a track that is neither configured nor stored
export function GetTrackRecord(AccountId: string, ProgressionId: string): TrackRecord | undefined{
    const Row = GetDb().select().from(progresstracks).where(TrackFor(AccountId, ProgressionId)).get();

    if(Row == undefined && GetProgressionPath(ProgressionId) == undefined){
        return undefined;
    }

    return ToTrackRecord(AccountId, ProgressionId, Row);
}

export function GetObjectiveRecords(AccountId: string): ObjectiveRecord[]{
    return GetDb().select().from(objectives).where(eq(objectives.accountId, AccountId)).orderBy(asc(objectives.objectiveId)).all()
        .map((Row) => ToObjectiveRecord(AccountId, Row));
}

// The stored objective, or zeros (never the 9,999,999 of the stub)
export function GetObjectiveRecord(AccountId: string, ObjectiveId: string): ObjectiveRecord{
    const Row = GetDb().select().from(objectives).where(and(eq(objectives.accountId, AccountId), eq(objectives.objectiveId, ObjectiveId))).get();

    if(Row != undefined){
        return ToObjectiveRecord(AccountId, Row);
    }

    const Now = new Date().toISOString();

    return {phx_account_id: AccountId, objective_id: ObjectiveId, progress: 0, completed_count: 0, created_date: Now, last_modified_date: Now};
}

type ParsedGrant = { Tracks: Map<string, number>, Objectives: Map<string, {Value: number, CompletedCount: number | undefined}> };

// Binary-proven body: {progress_tracks: [{progression_id, progress}], objectives:
// [{objective_id, value, completed_count}]}, both arrays always present. Track
// progress is an increment (two queued grants for one track are added together);
// objective values are absolute (the last one for an id wins).
function ParseGrantBody(Body: unknown, Notes: string[]): ParsedGrant | undefined{
    if(!IsPlainObject(Body)){
        Notes.push("body is not a JSON object");
        return undefined;
    }

    let TrackList = Body.progress_tracks;
    let ObjectiveList = Body.objectives;

    if(TrackList === undefined){
        Notes.push("no progress_tracks key");
        TrackList = [];
    }

    if(ObjectiveList === undefined){
        Notes.push("no objectives key");
        ObjectiveList = [];
    }

    if(!Array.isArray(TrackList) || !Array.isArray(ObjectiveList)){
        Notes.push("progress_tracks or objectives is not an array");
        return undefined;
    }

    const Cap = GetGrantCap();
    const Tracks = new Map<string, number>();

    for(const Entry of TrackList){
        const ProgressionId = ReadField(Entry, ["progression_id", "progressionId"], "track id", Notes);
        const Amount = ReadInteger(ReadField(Entry, ["progress", "amount", "value"], "track amount", Notes), "track amount", Notes);

        if(typeof ProgressionId !== "string" || ProgressionId.length === 0 || Amount === undefined){
            Notes.push(`skipped track entry ${JSON.stringify(Entry)}`);
            continue;
        }

        let Clamped = Amount;

        if(Clamped < 0){
            Notes.push(`${ProgressionId} amount ${Amount} is negative, counted as 0`);
            Clamped = 0;
        }

        Tracks.set(ProgressionId, (Tracks.get(ProgressionId) ?? 0) + Clamped);
    }

    // The cap is per track and request, so entries repeating one track can't add it twice
    for(const [ProgressionId, Total] of Tracks){
        if(Total > Cap){
            Notes.push(`${ProgressionId} total ${Total} is over the cap of ${Cap}, cut to ${Cap}`);
            Tracks.set(ProgressionId, Cap);
        }
    }

    const Objectives = new Map<string, {Value: number, CompletedCount: number | undefined}>();

    for(const Entry of ObjectiveList){
        const ObjectiveId = ReadField(Entry, ["objective_id", "objectiveId"], "objective id", Notes);
        const Value = ReadInteger(ReadField(Entry, ["value", "progress"], "objective value", Notes), "objective value", Notes);
        const CompletedCountRaw = ReadField(Entry, ["completed_count", "completedCount"], "objective completed_count", Notes);
        const CompletedCount = ReadInteger(CompletedCountRaw, "objective completed_count", Notes);

        if(typeof ObjectiveId !== "string" || ObjectiveId.length === 0 || Value === undefined){
            Notes.push(`skipped objective entry ${JSON.stringify(Entry)}`);
            continue;
        }

        if(CompletedCount === undefined){
            Notes.push(`objective ${ObjectiveId} has no usable completed_count, kept the stored one`);
        }

        Objectives.set(ObjectiveId, {Value: Value, CompletedCount: CompletedCount});
    }

    return {Tracks: Tracks, Objectives: Objectives};
}

function Clamp32(Value: number){
    return Math.max(-2147483648, Math.min(INT32_MAX, Value));
}

const RETRY_NOTE = "retry of event";

// The writes of an account's tracks, in progression_events
const TRACK_WRITE_ROUTES = ["POST /progression/:uid", "POST /progression/:uid/:track/:amount", "POST /progression/:uid/:track/:rank/confirm/:kind", "DELETE /progression/:uid/:track", "admin SeedProgression"];

// The retry guard of POST /progression/:uid (PROGRESSION_REPLAY_WINDOW_S, 5 s by default, 0 = off). The
// game server retries a request whose answer it did not get (HTTPRetryCount 5), and a retried grant would
// add its XP twice. A grant is taken for such a retry when the account's last track write (a grant, a
// confirm, a reset) was a grant with the same body (the same JSON), applied less than the window ago, or a
// retry of one: it gets that grant's stored reply and adds nothing. A confirm or any other grant in between
// means the game server had the answer, so the same body after it is a new grant. The idea is Harmonic's
// (github.com/Harmonicrain/Undaunted 895f7c7); that fork skipped track amounts whose objectives had not
// moved, which could drop real XP. The window stays clearly under the game server's grant flush:
// UProgressionComponent sends its queued grants at most once per QueuedGrantTimeout, 10 s (DefaultGame.ini;
// one-shot timer armed at 0x14145a4a9), so two grants it meant are normally 10 s or more apart, while a
// retry after a connection error comes within a few seconds (a lost answer takes HTTPTimeoutSeconds, 600 s,
// and is never inside any window). Two identical grants closer than the window, with nothing in between,
// that were both meant would still lose the second (logged: "repeats the grant of").
function FindRetriedGrant(tx: Tx, AccountId: string, Route: string, Body: unknown){
    const Window = ProgressionReplayWindow();

    if(Window === 0){
        return undefined;
    }

    const Columns = {id: progressionevents.id, time: progressionevents.time, route: progressionevents.route, status: progressionevents.status, body: progressionevents.body, reply: progressionevents.reply, note: progressionevents.note};

    const Last = tx.select(Columns).from(progressionevents)
        .where(and(eq(progressionevents.accountId, AccountId), inArray(progressionevents.route, TRACK_WRITE_ROUTES)))
        .orderBy(desc(progressionevents.id)).limit(1).get();

    if(Last == undefined || Last.route !== Route || Last.status !== 200 || Last.reply == null || Last.body !== JSON.stringify(Body)){
        return undefined;
    }

    // An answered retry names the grant it repeated; the window runs from that grant
    const RetryOf = new RegExp(`^${RETRY_NOTE} (\\d+) `).exec(Last.note ?? "");
    const Original = RetryOf == null ? Last : tx.select(Columns).from(progressionevents).where(and(eq(progressionevents.id, Number(RetryOf[1])), eq(progressionevents.accountId, AccountId))).get();

    if(Original == undefined || Original.reply == null){
        return undefined;
    }

    const Age = Date.now() - Date.parse(Original.time);

    if(!(Age >= 0 && Age < Window * 1000)){
        return undefined;
    }

    return {Reply: JSON.parse(Original.reply) as unknown, Seconds: Math.round(Age / 100) / 10, Note: `${RETRY_NOTE} ${Original.id} within ${Window} s: its reply, nothing added`};
}

// POST /progression/:uid (game server only). Reply: one track per distinct requested
// track with the NEW TOTAL and the stored confirmed ranks, and every requested
// objective as stored. Confirmed ranks are not raised here: the game server
// confirms them itself after granting the rewards.
export function GrantProgression(AccountId: string, Body: unknown, Who: Caller): RealReply{
    const Route = "POST /progression/:uid";
    const Notes: string[] = [];
    const Parsed = ParseGrantBody(Body, Notes);

    return GetDb().transaction((tx) => {
        if(!DoesAccountExist(tx, AccountId)){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 404, Notes: [...Notes, "unknown account"]});
            logger.warn(`Refusing progression grant for unknown account ${AccountId}`);
            return {Status: 404};
        }

        if(Parsed == undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 400, Notes});
            WarnNotes(`Refusing progression grant for ${AccountId}`, Notes);
            return {Status: 400};
        }

        const Retry = FindRetriedGrant(tx, AccountId, Route, Body);

        if(Retry != undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Reply: Retry.Reply, Notes: [Retry.Note, ...Notes]});
            logger.warn(`Progression grant for ${AccountId} repeats the grant of ${Retry.Seconds} s ago: answered its stored reply, nothing added (PROGRESSION_REPLAY_WINDOW_S=${ProgressionReplayWindow()})`);

            return {Status: 200, Body: Retry.Reply};
        }

        const Now = new Date().toISOString();
        const TrackReplies: TrackRecord[] = [];
        const Summary: string[] = [];

        for(const [ProgressionId, Amount] of Parsed.Tracks){
            const Current = ReadTrack(tx, AccountId, ProgressionId);
            const OldProgress = Current?.progress ?? 0;
            const NewProgress = Math.min(OldProgress + Amount, INT32_MAX);

            if(GetProgressionPath(ProgressionId) == undefined){
                Notes.push(`${ProgressionId} is not in the progression config, stored anyway`);
            }

            const Written = WriteTrack(tx, AccountId, ProgressionId, {
                progress: NewProgress,
                confirmedFreeRank: Current?.confirmedFreeRank ?? 0,
                confirmedPremiumRank: Current?.confirmedPremiumRank ?? 0,
                confirmedDate: Current?.confirmedDate ?? DEFAULT_CONFIRMED_DATE
            });

            TrackReplies.push(ToTrackRecord(AccountId, ProgressionId, Written));

            const OldRank = EarnedFree(ProgressionId, OldProgress);
            const NewRank = EarnedFree(ProgressionId, NewProgress);
            Summary.push(`${ProgressionId} +${Amount} -> ${NewProgress}${NewRank !== undefined && OldRank !== undefined && NewRank > OldRank ? ` (rank ${OldRank} -> ${NewRank})` : ""}`);
        }

        const ObjectiveReplies: ObjectiveRecord[] = [];

        for(const [ObjectiveId, State] of Parsed.Objectives){
            const Current = tx.select().from(objectives).where(and(eq(objectives.accountId, AccountId), eq(objectives.objectiveId, ObjectiveId))).get();
            const CompletedCount = Clamp32(State.CompletedCount ?? Current?.completedCount ?? 0);
            const Progress = Clamp32(State.Value);

            // Only noted: objective values are absolute and stored as sent. A repeatable objective starts
            // again at a lower value when its completed_count goes up, which is not "backwards".
            if(Current != undefined && Progress < Current.progress && CompletedCount <= Current.completedCount){
                Notes.push(`objective ${ObjectiveId} went backwards (${Current.progress}/${Current.completedCount} -> ${Progress}/${CompletedCount}), stored as sent`);
                logger.warn(`progression: objective went backwards: ${ObjectiveId} of ${AccountId} from ${Current.progress} (completed ${Current.completedCount}) to ${Progress} (completed ${CompletedCount}); stored as sent`);
            }

            const Written = tx.insert(objectives).values({
                accountId: AccountId,
                objectiveId: ObjectiveId,
                progress: Progress,
                completedCount: CompletedCount,
                createdDate: Now,
                lastModifiedDate: Now
            }).onConflictDoUpdate({
                target: [objectives.accountId, objectives.objectiveId],
                set: {progress: Progress, completedCount: CompletedCount, lastModifiedDate: Now}
            }).returning().get();

            ObjectiveReplies.push(ToObjectiveRecord(AccountId, Written));
        }

        const Reply = {
            code: "OK",
            message: "OK",
            payload: {
                progress_tracks: TrackReplies,
                objectives: ObjectiveReplies
            }
        };

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Reply, Notes});
        WarnNotes(`Progression grant for ${AccountId}`, Notes);
        logger.info(`Progression grant for ${AccountId}: ${Summary.length > 0 ? Summary.join(", ") : "no tracks"}; ${ObjectiveReplies.length} objective(s)`);

        return {Status: 200, Body: Reply};
    });
}

// POST /progression/:uid/:track/:amount (game server; quest and tutorial rewards). No body.
export function GrantProgressionInTrack(AccountId: string, ProgressionId: string, AmountRaw: string, Who: Caller): RealReply{
    const Route = "POST /progression/:uid/:track/:amount";
    const Notes: string[] = [];
    const Amount = ParsePathInteger(AmountRaw, true);

    return GetDb().transaction((tx) => {
        if(!DoesAccountExist(tx, AccountId)){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 404, Notes: [`track ${ProgressionId} amount ${AmountRaw}`, "unknown account"]});
            logger.warn(`Refusing progression grant for unknown account ${AccountId}`);
            return {Status: 404};
        }

        if(Amount === undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 400, Notes: [`track ${ProgressionId}`, `amount ${AmountRaw} is not an integer`]});
            logger.warn(`Refusing progression grant for ${AccountId} in ${ProgressionId}: amount ${AmountRaw} is not an integer`);
            return {Status: 400};
        }

        const Cap = GetGrantCap();
        let Clamped = Amount;

        if(Clamped < 0){
            Notes.push(`amount ${Amount} is negative, counted as 0`);
            Clamped = 0;
        }

        if(Clamped > Cap){
            Notes.push(`amount ${Amount} is over the cap of ${Cap}, cut to ${Cap}`);
            Clamped = Cap;
        }

        if(GetProgressionPath(ProgressionId) == undefined){
            Notes.push(`${ProgressionId} is not in the progression config, stored anyway`);
        }

        const Current = ReadTrack(tx, AccountId, ProgressionId);
        const OldProgress = Current?.progress ?? 0;

        const Written = WriteTrack(tx, AccountId, ProgressionId, {
            progress: Math.min(OldProgress + Clamped, INT32_MAX),
            confirmedFreeRank: Current?.confirmedFreeRank ?? 0,
            confirmedPremiumRank: Current?.confirmedPremiumRank ?? 0,
            confirmedDate: Current?.confirmedDate ?? DEFAULT_CONFIRMED_DATE
        });

        const Reply = {code: "OK", message: "OK", payload: ToTrackRecord(AccountId, ProgressionId, Written)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 200, Reply, Notes: [`track ${ProgressionId} amount ${AmountRaw}`, ...Notes]});
        WarnNotes(`Progression grant in ${ProgressionId} for ${AccountId}`, Notes);
        logger.info(`Progression grant for ${AccountId}: ${ProgressionId} +${Clamped} -> ${Written.progress}`);

        return {Status: 200, Body: Reply};
    });
}

// PROGRESSION_CONFIRM_ENTITLEMENTS=1 only: the permanent entitlements the config lists for the ranks a
// confirm has just raised (above From, up to To) on one reward list. Items, currencies and timed
// entitlements are never granted here: the game server pays rank rewards through /inventory (and
// grants entitlements itself, if it does; the in-game test of the Elite ranks decides).
function GrantRankEntitlements(tx: Tx, AccountId: string, Path: ProgressionPath, List: "free_rewards" | "premium_rewards", From: number, To: number, Notes: string[]){
    for(const Reward of (Path[List] ?? []) as any[]){
        if(!Number.isSafeInteger(Reward?.rank_id) || Reward.rank_id <= From || Reward.rank_id > To || !Array.isArray(Reward.entitlements)){
            continue;
        }

        for(const Entitlement of Reward.entitlements){
            const Name = Entitlement?.entitlement;

            if(typeof Name !== "string" || Name.length === 0){
                continue;
            }

            if(Entitlement.duration !== 0){
                Notes.push(`rank ${Reward.rank_id} ${List}: ${Name} (${Entitlement.duration} h) is timed, not granted on confirm`);
                continue;
            }

            GrantEntitlementInTx(tx, AccountId, Name, 0, `confirm:${Path.progression_id}:${Reward.rank_id}`);
            Notes.push(`rank ${Reward.rank_id} ${List}: granted entitlement ${Name} (PROGRESSION_CONFIRM_ENTITLEMENTS=1)`);
        }
    }
}

// POST /progression/:uid/:track/:rank/confirm/{public|premium} (game server, no body).
// Raises the confirmed rank to min(rank, earned); never lowers it, never grants items.
// Premium needs the track's premium_gating_entitlement (season09b_premium).
export function ConfirmRank(AccountId: string, ProgressionId: string, RankRaw: string, Kind: string, Who: Caller): RealReply{
    const Route = "POST /progression/:uid/:track/:rank/confirm/:kind";
    const Context = `track ${ProgressionId} rank ${RankRaw} ${Kind}`;
    const Rank = ParsePathInteger(RankRaw);
    const Path = GetProgressionPath(ProgressionId);

    return GetDb().transaction((tx) => {
        const Refuse = (Status: number, Why: string) => {
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status, Notes: [Context, Why]});
            logger.warn(`Refusing confirm of ${Context} for ${AccountId}: ${Why}`);
            return {Status: Status};
        };

        if(Kind !== "public" && Kind !== "premium"){
            return Refuse(400, "kind is not public or premium");
        }

        if(Rank === undefined){
            return Refuse(400, "rank is not a non-negative integer");
        }

        if(!DoesAccountExist(tx, AccountId)){
            return Refuse(404, "unknown account");
        }

        if(Path == undefined){
            return Refuse(404, "track is not in the progression config");
        }

        const Current = ReadTrack(tx, AccountId, ProgressionId);
        const Progress = Current?.progress ?? 0;
        let ConfirmedFree = Current?.confirmedFreeRank ?? 0;
        let ConfirmedPremium = Current?.confirmedPremiumRank ?? 0;
        const Notes: string[] = [Context];

        if(Kind === "public"){
            const Earned = ComputeEarnedRanks(Path, Progress, false).EarnedFreeRank;

            if(Rank > Earned){
                Notes.push(`rank ${Rank} is above the earned rank ${Earned} at progress ${Progress}, clamped`);
            }

            ConfirmedFree = Math.max(ConfirmedFree, Math.min(Rank, Earned));
        }
        else{
            const Gate = PremiumGatingEntitlement(Path);

            if(Gate === ""){
                Notes.push("premium confirm on a track without premium, nothing changed");
            }
            else if(!HasActiveEntitlement(tx, AccountId, Gate)){
                return Refuse(403, `account does not own ${Gate}`);
            }
            else{
                const Earned = ComputeEarnedRanks(Path, Progress, true).EarnedPremiumRank;

                if(Rank > Earned){
                    Notes.push(`rank ${Rank} is above the earned premium rank ${Earned} at progress ${Progress}, clamped`);
                }

                ConfirmedPremium = Math.max(ConfirmedPremium, Math.min(Rank, Earned));
            }
        }

        const Changed = ConfirmedFree !== (Current?.confirmedFreeRank ?? 0) || ConfirmedPremium !== (Current?.confirmedPremiumRank ?? 0);
        let Row = Current;

        if(Changed){
            Row = WriteTrack(tx, AccountId, ProgressionId, {
                progress: Progress,
                confirmedFreeRank: ConfirmedFree,
                confirmedPremiumRank: ConfirmedPremium,
                confirmedDate: new Date().toISOString()
            });

            if(ProgressionConfirmEntitlements()){
                GrantRankEntitlements(tx, AccountId, Path, "free_rewards", Current?.confirmedFreeRank ?? 0, ConfirmedFree, Notes);
                GrantRankEntitlements(tx, AccountId, Path, "premium_rewards", Current?.confirmedPremiumRank ?? 0, ConfirmedPremium, Notes);
            }
        }
        else{
            Notes.push("already confirmed");
        }

        const Reply = {code: "OK", message: "OK", payload: ToTrackRecord(AccountId, ProgressionId, Row)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 200, Reply, Notes});

        if(Notes.some((Note) => Note.includes("clamped") || Note.includes("without premium"))){
            WarnNotes(`Confirm for ${AccountId}`, Notes);
        }

        logger.info(`Confirmed ${Context} for ${AccountId}: confirmed free ${Reply.payload.confirmed_fremium_rank}, premium ${Reply.payload.confirmed_premium_rank}${Changed ? "" : " (unchanged)"}`);

        return {Status: 200, Body: Reply};
    });
}

// DELETE /progression/:uid/:track: back to progress 0 and ranks 0
export function ResetTrack(AccountId: string, ProgressionId: string, Who: Caller): RealReply{
    const Route = "DELETE /progression/:uid/:track";

    return GetDb().transaction((tx) => {
        const Current = ReadTrack(tx, AccountId, ProgressionId);

        if(Current == undefined && GetProgressionPath(ProgressionId) == undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 404, Notes: [`track ${ProgressionId} is unknown`]});
            return {Status: 404};
        }

        tx.delete(progresstracks).where(TrackFor(AccountId, ProgressionId)).run();

        const Reply = {code: "OK", message: "OK", payload: ToTrackRecord(AccountId, ProgressionId, undefined)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Status: 200, Reply, Notes: [`track ${ProgressionId} was at ${Current?.progress ?? 0}, confirmed ${Current?.confirmedFreeRank ?? 0}/${Current?.confirmedPremiumRank ?? 0}`]});
        logger.warn(`Reset track ${ProgressionId} of ${AccountId} (was progress ${Current?.progress ?? 0}) by ${Who}`);

        return {Status: 200, Body: Reply};
    });
}

// The selected Hunt Pass track; ACTIVE_HUNT_PASS (season09b, the only one with reward data) by default
export function GetSelectedHuntPass(AccountId: string){
    const Row = GetDb().select().from(huntpassselection).where(eq(huntpassselection.accountId, AccountId)).get();

    return Row != undefined && GetProgressionPath(Row.progressionId) != undefined ? Row.progressionId : GetActiveHuntPass();
}

// POST /huntpass/:uid {"progression_id": "..."} (game server, debug only in the binary)
export function SetSelectedHuntPass(AccountId: string, Body: unknown, Who: Caller): RealReply{
    const Route = "POST /huntpass/:uid";
    const Notes: string[] = [];
    const ProgressionId = ReadField(Body, ["progression_id", "progressionId"], "hunt pass id", Notes);

    return GetDb().transaction((tx) => {
        if(typeof ProgressionId !== "string" || GetProgressionPath(ProgressionId) == undefined){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 400, Notes: [...Notes, "not a configured track"]});
            logger.warn(`Refusing hunt pass selection ${JSON.stringify(ProgressionId)} for ${AccountId}: not a configured track`);
            return {Status: 400};
        }

        tx.insert(huntpassselection).values({accountId: AccountId, progressionId: ProgressionId, updatedDate: new Date().toISOString()})
            .onConflictDoUpdate({target: huntpassselection.accountId, set: {progressionId: ProgressionId, updatedDate: new Date().toISOString()}}).run();

        const Reply = {code: "OK", message: "OK", payload: ProgressionId};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Reply, Notes});
        WarnNotes(`Hunt pass selection for ${AccountId}`, Notes);
        logger.info(`Hunt pass of ${AccountId} set to ${ProgressionId}`);

        return {Status: 200, Body: Reply};
    });
}

function SeedTrack(Path: ProgressionPath, Current: TrackRow | undefined, Mode: SeedMode){
    if(Mode === "fresh"){
        return {progress: 0, confirmedFreeRank: 0, confirmedPremiumRank: 0, confirmedDate: DEFAULT_CONFIRMED_DATE};
    }

    // Grandfather: at least the max rank, fully confirmed, so nothing is pending or pops.
    // Never lowers anything already stored (season09b may be past max through prestige).
    const Progress = Math.max(Current?.progress ?? 0, TotalXpToMaxRank(Path));
    const Earned = Math.max(ComputeEarnedRanks(Path, Progress, false).EarnedFreeRank, MaxRankId(Path));

    return {
        progress: Progress,
        confirmedFreeRank: Math.max(Current?.confirmedFreeRank ?? 0, Earned),
        confirmedPremiumRank: Math.max(Current?.confirmedPremiumRank ?? 0, Earned),
        confirmedDate: new Date().toISOString()
    };
}

// Admin seed (roadmap 2.13), per account. grandfather: every configured track at its
// max rank and confirmed there, which looks like the stub and grants nothing.
// fresh: every configured track at 0 and the stored objectives cleared. Does not
// change the account's mode: real is the default, and with PROGRESSION_MODE=stub only
// the accounts in PROGRESSION_REAL_ACCOUNTS read these rows.
export function SeedProgression(AccountId: string, Mode: SeedMode, Admin: string): RealReply{
    const Route = "admin SeedProgression";

    return GetDb().transaction((tx) => {
        if(!DoesAccountExist(tx, AccountId)){
            return {Status: 404};
        }

        const Before = tx.select().from(progresstracks).where(eq(progresstracks.accountId, AccountId)).all();

        if(Mode === "fresh"){
            tx.delete(progresstracks).where(eq(progresstracks.accountId, AccountId)).run();
            tx.delete(objectives).where(eq(objectives.accountId, AccountId)).run();
        }

        for(const Path of GetProgressionPaths()){
            WriteTrack(tx, AccountId, Path.progression_id, SeedTrack(Path, Before.find((Row) => Row.progressionId === Path.progression_id), Mode));
        }

        const Tracks = tx.select().from(progresstracks).where(eq(progresstracks.accountId, AccountId)).all()
            .map((Row) => ToTrackRecord(AccountId, Row.progressionId, Row));

        RecordProgressionEvent(tx, {
            AccountId,
            Caller: "admin",
            Route,
            Body: {Mode: Mode, Admin: Admin},
            Status: 200,
            Reply: Tracks,
            Notes: [`before: ${Before.map((Row) => `${Row.progressionId}=${Row.progress}/${Row.confirmedFreeRank}/${Row.confirmedPremiumRank}`).join(", ") || "no stored tracks"}`]
        });
        logger.warn(`Admin ${Admin} seeded the progression of ${AccountId} (${Mode})`);

        return {Status: 200, Body: Tracks};
    });
}

// Accounts that have played (they own a character) but have never been served by real
// progression: no stored track and no progression event. On a server that ran in stub
// mode before real progression became the default, these are the players who drop from
// upstream's fake max ranks to Slayer level 1. Seeding an account stores its tracks, so
// it is no longer counted.
export function CountPlayersWithoutRealProgression(): number{
    const Db = GetDb();
    const Row = Db.select({Count: sql<number>`count(*)`}).from(users).where(and(
        exists(Db.select({One: sql`1`}).from(characters).where(eq(characters.userId, users.userId))),
        notExists(Db.select({One: sql`1`}).from(progresstracks).where(eq(progresstracks.accountId, users.userId))),
        notExists(Db.select({One: sql`1`}).from(progressionevents).where(eq(progressionevents.accountId, users.userId)))
    )).get();

    return Number(Row?.Count ?? 0);
}

// Startup warning for the upgrade from stub mode (roadmap 2.13). Nothing is migrated
// here: the admin decides per account (SeedProgression), or keeps PROGRESSION_MODE=stub.
export function ProgressionUpgradeNotice(): string | undefined{
    if(IsProgressionModeStub()){
        return undefined;
    }

    const Count = CountPlayersWithoutRealProgression();

    if(Count === 0){
        return undefined;
    }

    return `${Count} player account(s) have no stored progression yet: they start at Slayer level 1 with an empty Hunt Pass, not upstream's fake max ranks. Nothing was migrated. To keep max ranks for a player, seed them before they play (admin POST /undaunted/api/SeedProgression with Mode "grandfather"); PROGRESSION_MODE=stub brings the fake ranks back for everyone. See the upgrade notes (docs/setup/upgrading.md).`;
}
