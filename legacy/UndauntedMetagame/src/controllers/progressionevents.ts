import { eq } from "drizzle-orm";
import { progressionevents, users } from "../db/schema";
import { Tx } from "./savehistory";

// Shared by the real-mode routes: the audit row, the reply type and the lenient
// number reading. Request formats come from the client binary and no body has
// been captured yet, so a request that deviates from the proven shape is still
// taken when the meaning is clear; the deviation goes into the audit note and a
// warning, for the body capture to settle.

export type RealReply = { Status: number, Body?: unknown };

export type Caller = "gameserver" | "client" | "admin";

export function CallerOf(req: any): Caller{
    return req.AuthData?.IsGameserver ? "gameserver" : "client";
}

export function RecordProgressionEvent(tx: Tx, Event: { AccountId: string, Caller: string, Route: string, Body?: unknown, Status: number, Reply?: unknown, Notes?: string[] }){
    tx.insert(progressionevents).values({
        time: new Date().toISOString(),
        accountId: Event.AccountId,
        caller: Event.Caller,
        route: Event.Route,
        body: Event.Body === undefined ? null : JSON.stringify(Event.Body),
        status: Event.Status,
        reply: Event.Reply === undefined ? null : JSON.stringify(Event.Reply),
        note: Event.Notes != undefined && Event.Notes.length > 0 ? Event.Notes.join("; ") : null
    }).run();
}

// An integer from a JSON number, or from a string of digits (noted)
export function ReadInteger(Value: unknown, What: string, Notes: string[]): number | undefined{
    if(typeof Value === "number" && Number.isFinite(Value)){
        if(!Number.isInteger(Value)){
            Notes.push(`${What} ${Value} is not a whole number, truncated`);
        }

        return Math.trunc(Value);
    }

    if(typeof Value === "string" && /^\s*-?\d+\s*$/.test(Value)){
        Notes.push(`${What} came as the string "${Value}"`);
        return Number(Value);
    }

    return undefined;
}

// First key present; using any but the first (the one the binary writes) is noted
export function ReadField(Source: any, Keys: string[], What: string, Notes: string[]){
    for(const [Index, Key] of Keys.entries()){
        if(Source != null && Source[Key] !== undefined){
            if(Index > 0){
                Notes.push(`${What} came as "${Key}", not "${Keys[0]}"`);
            }

            return Source[Key];
        }
    }

    return undefined;
}

export function IsPlainObject(Value: unknown): Value is Record<string, unknown>{
    return Value != null && typeof Value === "object" && !Array.isArray(Value);
}

export function DoesAccountExist(tx: Tx, AccountId: string){
    return tx.select({userId: users.userId}).from(users).where(eq(users.userId, AccountId)).get() != undefined;
}

// A decimal int32 from a URL segment
export function ParsePathInteger(Value: unknown, AllowNegative = false): number | undefined{
    if(typeof Value !== "string" || !(AllowNegative ? /^-?\d{1,10}$/ : /^\d{1,10}$/).test(Value)){
        return undefined;
    }

    const Parsed = Number(Value);

    return Parsed > 2147483647 || Parsed < -2147483648 ? undefined : Parsed;
}
