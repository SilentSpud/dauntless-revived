import { randomBytes, randomInt, randomUUID } from "node:crypto";
import { and, eq, gt, ne, or, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { characters, invitecodes, userapikeys, users } from "../db/schema";
import { HashUserAPIKey } from "./auth";
import type { RegistrationMode } from "./undauntedapi";
import type { Tx } from "./savehistory";
import { logger } from "../logger";

// Roadmap 1.5/1.6: account names and invites for friends.
// Names are 3-16 letters, digits or underscores and unique regardless of case.
// Existing accounts keep whatever name they have, even one outside these rules;
// the rules apply to new names only (registering and renaming), after trimming
// surrounding whitespace (docs/setup/admin.md).

export const USERNAME_RULE = /^[A-Za-z0-9_]{3,16}$/;

export function IsValidUsername(Value: unknown): Value is string {
    return typeof Value === "string" && USERNAME_RULE.test(Value);
}

// The name as it will be stored: surrounding whitespace dropped. Non-strings stay as they are.
export function TrimUsername(Value: unknown){
    return typeof Value === "string" ? Value.trim() : Value;
}

// Error codes of the /undaunted/api account routes: {"error": <code>, "message": <text>}
export type AccountError = "username_invalid" | "registration_closed" | "bad_request" | "invite_invalid" | "username_taken" | "not_found";

const MESSAGES: Record<AccountError, string> = {
    username_invalid: "Usernames are 3-16 characters: letters, numbers and underscore.",
    registration_closed: "Registration is closed on this server.",
    bad_request: "The request body is not JSON, or a field is missing or of the wrong type.",
    invite_invalid: "That invite code is not valid or has already been used.",
    username_taken: "That username is taken.",
    not_found: "No such account."
};

export type AccountFailure = { ok: false, Status: 400 | 401 | 404 | 409, Error: AccountError };

export function Failure(Status: AccountFailure["Status"], Error: AccountError): AccountFailure {
    return { ok: false, Status: Status, Error: Error };
}

export function ErrorBody(Error: AccountError){
    return { error: Error, message: MESSAGES[Error] };
}

// Case-insensitive; ExceptUserId lets an account keep its own name in another case.
// lower() folds ASCII only, which covers every name the rules allow.
function IsUsernameTakenInTx(tx: Tx, Username: string, ExceptUserId?: string){
    const SameName = sql`lower(${users.name}) = lower(${Username})`;

    return tx.select({ userId: users.userId }).from(users)
        .where(ExceptUserId == undefined ? SameName : and(SameName, ne(users.userId, ExceptUserId)))
        .get() != undefined;
}

export function IsUsernameTaken(Username: string){
    return GetDb().transaction((tx) => IsUsernameTakenInTx(tx, Username));
}

function UsableInvite(InviteCode: string){
    return and(
        eq(invitecodes.inviteCode, InviteCode),
        or(eq(invitecodes.infiniteUses, true), gt(invitecodes.usesRemaining, 0))
    );
}

export type RegisterResult = { ok: true, UserId: string, Username: string, UUK: string } | AccountFailure;

// POST /undaunted/api/Register. Everything happens in one SQLite transaction: the
// invite is checked, then the name, and only then is a use of the invite spent, so a
// taken name never costs the friend their code; the account row and its key row are
// written together, so a crash can't leave an account without a key.
export function RegisterAccount(Mode: RegistrationMode, RequestedUsername: unknown, InviteCode: unknown): RegisterResult {
    const Username = TrimUsername(RequestedUsername);

    if(Mode === "NONE"){
        return Failure(400, "registration_closed");
    }

    if(typeof Username !== "string"){
        return Failure(400, "bad_request");
    }

    if(!IsValidUsername(Username)){
        return Failure(400, "username_invalid");
    }

    const Code = typeof InviteCode === "string" ? InviteCode.trim() : "";

    if(Mode === "INVITECODE" && Code.length === 0){
        return Failure(401, "invite_invalid");
    }

    const UserId = `UID-${randomUUID()}`;
    const UUK = `UUK_${randomBytes(24).toString("hex")}`;

    return GetDb().transaction((tx): RegisterResult => {
        if(Mode === "INVITECODE" && tx.select({ inviteCode: invitecodes.inviteCode }).from(invitecodes).where(UsableInvite(Code)).get() == undefined){
            return Failure(401, "invite_invalid");
        }

        if(IsUsernameTakenInTx(tx, Username)){
            return Failure(409, "username_taken");
        }

        if(Mode === "INVITECODE"){
            // Same statement as ValidateAndConsumeInviteCode: the decrement and its condition are one UPDATE
            const Spent = tx.update(invitecodes).set({
                usesRemaining: sql`case when ${invitecodes.infiniteUses} then ${invitecodes.usesRemaining} else ${invitecodes.usesRemaining} - 1 end`
            }).where(UsableInvite(Code)).returning({ inviteCode: invitecodes.inviteCode }).all();

            if(Spent.length !== 1){
                return Failure(401, "invite_invalid");
            }
        }

        tx.insert(users).values({ userId: UserId, name: Username, notes: 0 }).run();
        tx.insert(userapikeys).values({ userId: UserId, keyHash: HashUserAPIKey(UUK) }).run();

        return { ok: true, UserId: UserId, Username: Username, UUK: UUK };
    });
}

export type RenameResult = { ok: true, UserId: string, OldUsername: string, Username: string, Characters: number } | AccountFailure;

// Admin rename. users.name and the account's character names change together
// (a character is created with the account's name, and GET /character sends it).
export function RenameAccount(Who: { UserId?: unknown, Username?: unknown }, RequestedUsername: unknown): RenameResult {
    const NewUsername = TrimUsername(RequestedUsername);

    if(typeof NewUsername !== "string"){
        return Failure(400, "bad_request");
    }

    if(!IsValidUsername(NewUsername)){
        return Failure(400, "username_invalid");
    }

    const ByUserId = typeof Who.UserId === "string" && Who.UserId.length > 0;
    const ByUsername = typeof Who.Username === "string" && Who.Username.length > 0;

    if(!ByUserId && !ByUsername){
        return Failure(400, "bad_request");
    }

    return GetDb().transaction((tx): RenameResult => {
        const Account = ByUserId
            ? tx.select().from(users).where(eq(users.userId, Who.UserId as string)).get()
            : tx.select().from(users).where(sql`lower(${users.name}) = lower(${Who.Username as string})`).all();

        const Found = Array.isArray(Account) ? (Account.length === 1 ? Account[0] : undefined) : Account;

        if(Found == undefined){
            return Failure(404, "not_found");
        }

        if(IsUsernameTakenInTx(tx, NewUsername, Found.userId)){
            return Failure(409, "username_taken");
        }

        tx.update(users).set({ name: NewUsername }).where(eq(users.userId, Found.userId)).run();
        const Renamed = tx.update(characters).set({ name: NewUsername }).where(eq(characters.userId, Found.userId)).returning({ characterId: characters.characterId }).all();

        return { ok: true, UserId: Found.userId, OldUsername: Found.name, Username: NewUsername, Characters: Renamed.length };
    });
}

// Invite codes: three groups of four characters from Crockford's base32 alphabet
// (no I, L, O or U, so a code read aloud or retyped can't be misread), 60 random bits.
const INVITE_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function GenerateInviteCode(){
    const Groups: string[] = [];

    for(let Group = 0; Group < 3; Group++){
        let Part = "";

        for(let Index = 0; Index < 4; Index++){
            Part += INVITE_ALPHABET[randomInt(0, INVITE_ALPHABET.length)];
        }

        Groups.push(Part);
    }

    return Groups.join("-");
}

export const MAX_INVITE_USES = 1000;

// Codes are credentials: the log gets the first group only.
// A note for the host's log only ("for Alex"); not stored, never sent anywhere
export function CleanInviteNote(Value: unknown){
    if(typeof Value !== "string"){
        return undefined;
    }

    const Note = Value.replace(/[^\x20-\x7E]/g, "").trim().slice(0, 64);

    return Note.length > 0 ? Note : undefined;
}

export function LogInviteCreated(Code: string, Uses: number, Note: string | undefined, AdminUserId: string){
    logger.info(`Invite ${Code.slice(0, 4)}-... created by ${AdminUserId}: ${Uses} use(s)${Note != undefined ? `, note "${Note}"` : ""}`);
}
