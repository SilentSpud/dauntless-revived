import { and, asc, eq, ne } from "drizzle-orm";
import { GetDb } from "../db";
import { bounties, bountydraft } from "../db/schema";
import { logger } from "../logger";
import { Caller, IsPlainObject, ReadField, ReadInteger, RealReply, RecordProgressionEvent } from "./progressionevents";
import { Tx } from "./savehistory";

// Bounty board of real-mode accounts. POST /bounty/:uid is a PARTIAL upsert: each
// sender puts only the bounty that changed (or none) in "bounties", and draft_data
// only when it changed. Nothing is ever removed for being absent; claim and abandon
// go through POST /bounty/delete/:uid. Elements are stored and returned as sent.

const DEFAULT_DRAFT_DATA = {
    current_draft_choices: [],
    previous_draft_selections: [],
    bronze_count: 0,
    silver_count: 0,
    gold_count: 0
};

function Board(tx: Tx, AccountId: string){
    const Bounties = tx.select().from(bounties).where(eq(bounties.accountId, AccountId)).orderBy(asc(bounties.slotIndex), asc(bounties.bountyId)).all().map((Row) => {
        const Element = JSON.parse(Row.data);

        // The client zeroes every bounty member except drafted_timestamp
        if(typeof Element.drafted_timestamp !== "string"){
            Element.drafted_timestamp = Row.updatedDate;
        }

        return Element;
    });

    const Draft = tx.select().from(bountydraft).where(eq(bountydraft.accountId, AccountId)).get();

    return {
        bounties: Bounties,
        draft_data: Draft != undefined ? JSON.parse(Draft.data) : DEFAULT_DRAFT_DATA
    };
}

// GET /bounty/:uid
export function GetBountyReply(AccountId: string){
    return {code: "OK", message: "OK", payload: GetDb().transaction((tx) => Board(tx, AccountId))};
}

export function SetBounties(AccountId: string, Body: unknown, Who: Caller): RealReply{
    const Route = "POST /bounty/:uid";
    const Notes: string[] = [];

    return GetDb().transaction((tx) => {
        let List = IsPlainObject(Body) ? Body.bounties : undefined;

        if(IsPlainObject(Body) && List === undefined){
            Notes.push("no bounties key");
            List = [];
        }

        if(!Array.isArray(List)){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 400, Notes: [...Notes, "body is not {bounties: [...]}"]});
            logger.warn(`Refusing bounty update for ${AccountId}: body is not {bounties: [...]}`);
            return {Status: 400};
        }

        const Now = new Date().toISOString();
        const Saved: string[] = [];

        for(const Element of List){
            const BountyId = IsPlainObject(Element) ? Element.bounty_id : undefined;

            if(!IsPlainObject(Element) || typeof BountyId !== "string" || BountyId.length === 0){
                Notes.push(`skipped element without a bounty_id: ${JSON.stringify(Element)}`);
                continue;
            }

            const Version = ReadInteger(Element.update_version, `${BountyId} update_version`, Notes) ?? 0;
            const SlotIndex = ReadInteger(Element.slot_index, `${BountyId} slot_index`, Notes) ?? null;
            const Stored = tx.select().from(bounties).where(and(eq(bounties.accountId, AccountId), eq(bounties.bountyId, BountyId))).get();

            if(Stored != undefined && Stored.updateVersion > Version){
                Notes.push(`kept ${BountyId}: stored update_version ${Stored.updateVersion} is newer than ${Version}`);
                continue;
            }

            if(SlotIndex !== null){
                const Replaced = tx.delete(bounties).where(and(eq(bounties.accountId, AccountId), eq(bounties.slotIndex, SlotIndex), ne(bounties.bountyId, BountyId))).returning().all();

                for(const Row of Replaced){
                    Notes.push(`${BountyId} took slot ${SlotIndex} from ${Row.bountyId}`);
                }
            }

            tx.insert(bounties).values({accountId: AccountId, bountyId: BountyId, slotIndex: SlotIndex, updateVersion: Version, data: JSON.stringify(Element), updatedDate: Now})
                .onConflictDoUpdate({target: [bounties.accountId, bounties.bountyId], set: {slotIndex: SlotIndex, updateVersion: Version, data: JSON.stringify(Element), updatedDate: Now}}).run();
            Saved.push(`${BountyId} v${Version}`);
        }

        if(IsPlainObject(Body) && Body.draft_data !== undefined){
            if(IsPlainObject(Body.draft_data)){
                tx.insert(bountydraft).values({accountId: AccountId, data: JSON.stringify(Body.draft_data), updatedDate: Now})
                    .onConflictDoUpdate({target: bountydraft.accountId, set: {data: JSON.stringify(Body.draft_data), updatedDate: Now}}).run();
                Saved.push("draft data");
            }
            else{
                Notes.push("draft_data is not an object, ignored");
            }
        }

        const Reply = {code: "OK", message: "OK", payload: Board(tx, AccountId)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Notes});

        if(Notes.length > 0){
            logger.warn(`Bounty update for ${AccountId}: ${Notes.join("; ")}`);
        }

        logger.info(`Bounty update for ${AccountId}: ${Saved.length > 0 ? Saved.join(", ") : "nothing stored"}`);

        return {Status: 200, Body: Reply};
    });
}

// POST /bounty/delete/:uid {"bounty_ids": [...]}: claim, abandon and reset. Unknown ids are fine.
export function DeleteBounties(AccountId: string, Body: unknown, Who: Caller): RealReply{
    const Route = "POST /bounty/delete/:uid";
    const Notes: string[] = [];
    const Ids = ReadField(Body, ["bounty_ids", "bountyIds"], "bounty ids", Notes);

    return GetDb().transaction((tx) => {
        if(!Array.isArray(Ids)){
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 400, Notes: [...Notes, "no bounty_ids array"]});
            logger.warn(`Refusing bounty delete for ${AccountId}: no bounty_ids array`);
            return {Status: 400};
        }

        const Removed: string[] = [];

        for(const Id of Ids){
            if(typeof Id !== "string"){
                Notes.push(`skipped id ${JSON.stringify(Id)}`);
                continue;
            }

            if(tx.delete(bounties).where(and(eq(bounties.accountId, AccountId), eq(bounties.bountyId, Id))).returning().all().length > 0){
                Removed.push(Id);
            }
        }

        const Reply = {code: "OK", message: "OK", payload: Board(tx, AccountId)};

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route, Body, Status: 200, Notes});
        logger.info(`Bounty delete for ${AccountId}: asked ${Ids.length}, removed ${Removed.length > 0 ? Removed.join(", ") : "none"}`);

        return {Status: 200, Body: Reply};
    });
}
