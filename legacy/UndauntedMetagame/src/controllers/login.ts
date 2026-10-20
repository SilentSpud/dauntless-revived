import { eq, inArray, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { users } from "../db/schema";

export async function GetUsernameForUserId(userId: string){
    let UserFromDb = await GetDb().query.users.findFirst({where: eq(users.userId, userId)});

    return UserFromDb!.name;
}

// displayName in the account (GET /account/api/public/account) and party replies.
// Upstream passed the unawaited promise of GetUsernameForUserId there, which JSON
// turns into {} (roadmap 1.6); ACCOUNT_DISPLAY_NAME=0 puts that {} back. An account
// that no longer exists gets "" (the promise used to reject unhandled).
export async function DisplayNameForUserId(userId: unknown): Promise<string | {}>{
    if(process.env.ACCOUNT_DISPLAY_NAME === "0"){
        return {};
    }

    if(typeof userId !== "string"){
        return "";
    }

    const UserFromDb = await GetDb().query.users.findFirst({where: eq(users.userId, userId)});

    return UserFromDb?.name ?? "";
}

// Lookups that never throw (roadmap 1.9, parties plan phase 0): undefined for an unknown
// account, where GetUsernameForUserId dereferences a missing row.

// Account ids as the client and the deploy server's expected-player list carry them:
// UID-<uuid>. Anything else is not looked up.
export const ACCOUNT_ID_RULE = /^[A-Za-z0-9_-]{1,64}$/;

export function IsAccountIdShape(Value: unknown): Value is string {
    return typeof Value === "string" && ACCOUNT_ID_RULE.test(Value);
}

export function FindUsernameForUserId(UserId: unknown): string | undefined {
    if(!IsAccountIdShape(UserId)){
        return undefined;
    }

    return GetDb().select({ name: users.name }).from(users).where(eq(users.userId, UserId)).get()?.name;
}

// Names of several accounts at once; unknown ids are left out of the map
export function FindUsernames(UserIds: string[]): Map<string, string> {
    const Ids = [...new Set(UserIds.filter(IsAccountIdShape))];
    const Names = new Map<string, string>();

    if(Ids.length === 0){
        return Names;
    }

    for(const Row of GetDb().select({ userId: users.userId, name: users.name }).from(users).where(inArray(users.userId, Ids)).all()){
        Names.set(Row.userId, Row.name);
    }

    return Names;
}

// By name, regardless of case (names are unique that way since roadmap 1.6). Two older
// accounts whose names differ only in case are ambiguous: neither is returned.
export function FindAccountByUsername(Username: unknown): { UserId: string, Username: string } | undefined {
    if(typeof Username !== "string"){
        return undefined;
    }

    const Name = Username.trim();

    if(Name.length === 0 || Name.length > 64){
        return undefined;
    }

    const Rows = GetDb().select({ userId: users.userId, name: users.name }).from(users).where(sql`lower(${users.name}) = lower(${Name})`).limit(2).all();

    return Rows.length === 1 ? { UserId: Rows[0].userId, Username: Rows[0].name } : undefined;
}

// An account named either by id or by name (the /undaunted/api fallbacks take both)
export function FindAccount(Who: unknown): { UserId: string, Username: string } | undefined {
    if(IsAccountIdShape(Who)){
        const Name = FindUsernameForUserId(Who);

        if(Name !== undefined){
            return { UserId: Who, Username: Name };
        }
    }

    return FindAccountByUsername(Who);
}
