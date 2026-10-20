import { and, eq, isNull } from "drizzle-orm";
import { GetDb } from "../db";
import { entitlements } from "../db/schema";
import { logger } from "../logger";
import { Tx } from "./savehistory";

// Entitlements of real-mode accounts (GET /entitlementsv2, POST /entitlementv2/:uid,
// DELETE /entitlement/:uid/:name). The client never checks expiry itself, so an
// expired timed entitlement is left out of every list. duration is in hours.

export type EntitlementRecord = { name: string, duration: number, activatedDate: string };

const HOUR_MS = 60 * 60 * 1000;

// season09b_premium is the Elite Hunt Pass (every account gets it); season_premium_any
// and season_free_any are what the Blueprints check. ENTITLEMENTS_DEFAULT replaces the list.
const DEFAULT_ENTITLEMENTS = "season09b_premium,season_premium_any,season_free_any";

export function GetDefaultEntitlements(){
    const Raw = process.env.ENTITLEMENTS_DEFAULT ?? DEFAULT_ENTITLEMENTS;

    return Raw.split(",").map((Name) => Name.trim()).filter((Name) => Name.length > 0);
}

type EntitlementRow = typeof entitlements.$inferSelect;

function IsActive(Row: EntitlementRow, Now: number){
    if(Row.revokedDate != null){
        return false;
    }

    if(Row.duration <= 0){
        return true;
    }

    const Activated = Date.parse(Row.activatedDate);

    return Number.isNaN(Activated) || Activated + Row.duration * HOUR_MS > Now;
}

// A default is added once per account; a revoked default stays revoked (the row is kept)
function EnsureDefaultEntitlements(tx: Tx, AccountId: string){
    const Now = new Date().toISOString();

    for(const Name of GetDefaultEntitlements()){
        tx.insert(entitlements).values({
            accountId: AccountId,
            name: Name,
            activatedDate: Now,
            duration: 0,
            source: "default",
            grantedDate: Now,
            revokedDate: null
        }).onConflictDoNothing().run();
    }
}

function ActiveRecords(tx: Tx, AccountId: string): EntitlementRecord[]{
    const Now = Date.now();

    return tx.select().from(entitlements).where(and(eq(entitlements.accountId, AccountId), isNull(entitlements.revokedDate))).all()
        .filter((Row) => IsActive(Row, Now))
        .sort((A, B) => A.name.localeCompare(B.name))
        .map((Row) => ({name: Row.name, duration: Row.duration, activatedDate: Row.activatedDate}));
}

export function ListEntitlements(AccountId: string): EntitlementRecord[]{
    return GetDb().transaction((tx) => {
        EnsureDefaultEntitlements(tx, AccountId);

        return ActiveRecords(tx, AccountId);
    });
}

export function HasActiveEntitlement(tx: Tx, AccountId: string, Name: string){
    EnsureDefaultEntitlements(tx, AccountId);

    const Row = tx.select().from(entitlements).where(and(eq(entitlements.accountId, AccountId), eq(entitlements.name, Name))).get();

    return Row != undefined && IsActive(Row, Date.now());
}

// Permanent grant of something owned: no change. Timed grant of something still
// running: the hours are added on (activatedDate kept, so the client computes the
// later expiry). Expired or revoked: starts again now. The original backend's
// stacking rule is unknown; this is the safe reading.
export function GrantEntitlementInTx(tx: Tx, AccountId: string, Name: string, Duration: number, Source: string){
    EnsureDefaultEntitlements(tx, AccountId);

    const Now = new Date();
    const Row = tx.select().from(entitlements).where(and(eq(entitlements.accountId, AccountId), eq(entitlements.name, Name))).get();

    if(Row != undefined && IsActive(Row, Now.getTime())){
        if(Row.duration === 0){
            logger.info(`Entitlement ${Name} of ${AccountId} is already permanent`);
        }
        else if(Duration === 0){
            tx.update(entitlements).set({duration: 0, source: Source, grantedDate: Now.toISOString()})
                .where(and(eq(entitlements.accountId, AccountId), eq(entitlements.name, Name))).run();
            logger.info(`Entitlement ${Name} of ${AccountId} is now permanent`);
        }
        else{
            tx.update(entitlements).set({duration: Row.duration + Duration, source: Source, grantedDate: Now.toISOString()})
                .where(and(eq(entitlements.accountId, AccountId), eq(entitlements.name, Name))).run();
            logger.info(`Entitlement ${Name} of ${AccountId} extended by ${Duration} h to ${Row.duration + Duration} h from ${Row.activatedDate}`);
        }
    }
    else{
        tx.insert(entitlements).values({
            accountId: AccountId,
            name: Name,
            activatedDate: Now.toISOString(),
            duration: Duration,
            source: Source,
            grantedDate: Now.toISOString(),
            revokedDate: null
        }).onConflictDoUpdate({
            target: [entitlements.accountId, entitlements.name],
            set: {activatedDate: Now.toISOString(), duration: Duration, source: Source, grantedDate: Now.toISOString(), revokedDate: null}
        }).run();
        logger.info(`Granted entitlement ${Name} to ${AccountId} (${Duration === 0 ? "permanent" : `${Duration} h`})`);
    }

    return ActiveRecords(tx, AccountId);
}

export function RevokeEntitlementInTx(tx: Tx, AccountId: string, Name: string){
    EnsureDefaultEntitlements(tx, AccountId);

    const Revoked = tx.update(entitlements).set({revokedDate: new Date().toISOString()})
        .where(and(eq(entitlements.accountId, AccountId), eq(entitlements.name, Name), isNull(entitlements.revokedDate))).returning().all();

    logger.info(Revoked.length > 0 ? `Revoked entitlement ${Name} of ${AccountId}` : `Revoke of entitlement ${Name} of ${AccountId}: not owned, nothing to do`);

    return Revoked.length > 0;
}
