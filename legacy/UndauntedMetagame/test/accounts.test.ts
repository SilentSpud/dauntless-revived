import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { invitecodes, userapikeys, users } from "../src/db/schema";
import { GenerateInviteCode, IsUsernameTaken, IsValidUsername, RegisterAccount, RenameAccount } from "../src/controllers/accounts";
import { RegisterInviteCode, GetUserInfoForApiKey } from "../src/controllers/undauntedapi";
import { CreateCharacterForUid } from "../src/controllers/character";
import { DisplayNameForUserId } from "../src/controllers/login";
import { Count, ReadCharacter } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

function UsesLeft(Code: string){
    return GetDb().select().from(invitecodes).where(eq(invitecodes.inviteCode, Code)).get()?.usesRemaining;
}

// An account from before the rules, as the owner's "Slayer" is
GetDb().insert(users).values({ userId: "UID-owner", name: "Slayer", notes: 0, isAdmin: true }).run();

describe("username rules", () => {
    it("takes 3-16 letters, digits and underscores", () => {
        for(const Name of ["abc", "Slayer_2", "A_B_C_D_E_F_G_H_", "0123456789abcdef", "___"]){
            assert.equal(IsValidUsername(Name), true, Name);
        }
    });

    it("refuses anything else", () => {
        for(const Name of ["ab", "a".repeat(17), "has space", " padded", "padded ", "dash-name", "dot.name", "émile", "名前名前", "", "tab\tname", "new\nline"]){
            assert.equal(IsValidUsername(Name), false, JSON.stringify(Name));
        }

        for(const Value of [undefined, null, 123, {}, ["abc"]]){
            assert.equal(IsValidUsername(Value), false, String(Value));
        }
    });
});

describe("RegisterAccount", () => {
    it("invite-only signup issues distinct personal keys, stores only hashes, and rejects altered keys", async () => {
        await RegisterInviteCode("TEST-AUTO-KEYS", 2, false);
        const A = RegisterAccount("INVITECODE", "AutoKey_A", "TEST-AUTO-KEYS");
        const B = RegisterAccount("INVITECODE", "AutoKey_B", "TEST-AUTO-KEYS");
        assert.ok(A.ok && B.ok);
        assert.notEqual(A.UUK, B.UUK);
        for (const Account of [A, B]) {
            const Stored = GetDb().select().from(userapikeys).where(eq(userapikeys.userId, Account.UserId)).get();
            assert.deepEqual(Stored, { userId: Account.UserId, keyHash: createHash("sha256").update(Account.UUK).digest("hex") });
            assert.deepEqual(await GetUserInfoForApiKey(Account.UUK), { UserId: Account.UserId, Username: Account.Username, IsAdmin: false });
            const Altered = Account.UUK.slice(0, -1) + (Account.UUK.endsWith("0") ? "1" : "0");
            assert.equal(await GetUserInfoForApiKey(Altered), undefined);
        }
        assert.equal(UsesLeft("TEST-AUTO-KEYS"), 0);
        assert.equal(RegisterAccount("INVITECODE", "AutoKey_NoInvite", undefined).ok, false);
    });

    it("registers a valid name with an account and its key in one go (OPEN)", async () => {
        const Result = RegisterAccount("OPEN", "Friend_One", undefined);

        assert.equal(Result.ok, true);

        if(Result.ok){
            assert.match(Result.UUK, /^UUK_[0-9a-f]{48}$/);
            assert.equal(Count("users", "userId = ?", Result.UserId), 1);
            assert.equal(Count("userapikeys", "userId = ?", Result.UserId), 1);
            assert.deepEqual(await GetUserInfoForApiKey(Result.UUK), { UserId: Result.UserId, Username: "Friend_One", IsAdmin: false });
        }
    });

    it("refuses a name that exists in any case, including the owner's older name", () => {
        const Before = Count("users");

        for(const Name of ["slayer", "SLAYER", "Slayer", "friend_one", "FRIEND_ONE"]){
            assert.deepEqual(RegisterAccount("OPEN", Name, undefined), { ok: false, Status: 409, Error: "username_taken" }, Name);
        }

        assert.equal(Count("users"), Before);
        assert.equal(IsUsernameTaken("sLaYeR"), true);
        assert.equal(IsUsernameTaken("Nobody"), false);
    });

    it("answers bad_request, username_invalid and registration_closed", () => {
        assert.deepEqual(RegisterAccount("OPEN", undefined, undefined), { ok: false, Status: 400, Error: "bad_request" });
        assert.deepEqual(RegisterAccount("OPEN", 42, undefined), { ok: false, Status: 400, Error: "bad_request" });
        assert.deepEqual(RegisterAccount("OPEN", "no", undefined), { ok: false, Status: 400, Error: "username_invalid" });
        assert.deepEqual(RegisterAccount("OPEN", "has space", undefined), { ok: false, Status: 400, Error: "username_invalid" });
        assert.deepEqual(RegisterAccount("NONE", "Valid_Name", "ANY"), { ok: false, Status: 400, Error: "registration_closed" });
    });

    it("INVITECODE: needs a live code, spends one use, and a taken name costs no use", async () => {
        await RegisterInviteCode("TEST-CODE-0001", 2, false);

        assert.deepEqual(RegisterAccount("INVITECODE", "Invited_A", undefined), { ok: false, Status: 401, Error: "invite_invalid" });
        assert.deepEqual(RegisterAccount("INVITECODE", "Invited_A", "  "), { ok: false, Status: 401, Error: "invite_invalid" });
        assert.deepEqual(RegisterAccount("INVITECODE", "Invited_A", "WRONG-CODE"), { ok: false, Status: 401, Error: "invite_invalid" });
        assert.deepEqual(RegisterAccount("INVITECODE", "SLAYER", "TEST-CODE-0001"), { ok: false, Status: 409, Error: "username_taken" });
        assert.equal(UsesLeft("TEST-CODE-0001"), 2);

        assert.equal(RegisterAccount("INVITECODE", "Invited_A", "TEST-CODE-0001").ok, true);
        assert.equal(UsesLeft("TEST-CODE-0001"), 1);
        assert.equal(RegisterAccount("INVITECODE", "Invited_B", " TEST-CODE-0001 ").ok, true);
        assert.equal(UsesLeft("TEST-CODE-0001"), 0);
        assert.deepEqual(RegisterAccount("INVITECODE", "Invited_C", "TEST-CODE-0001"), { ok: false, Status: 401, Error: "invite_invalid" });
        // A bad code is reported before the name is looked at
        assert.deepEqual(RegisterAccount("INVITECODE", "slayer", "TEST-CODE-0001"), { ok: false, Status: 401, Error: "invite_invalid" });
    });

    it("INVITECODE: an unlimited code stays usable", async () => {
        await RegisterInviteCode("TEST-OPEN-CODE", 0, true);

        assert.equal(RegisterAccount("INVITECODE", "Unlimited_1", "TEST-OPEN-CODE").ok, true);
        assert.equal(RegisterAccount("INVITECODE", "Unlimited_2", "TEST-OPEN-CODE").ok, true);
    });

    it("drops surrounding whitespace before checking and storing the name", () => {
        const Result = RegisterAccount("OPEN", "  Trim_Me \n", undefined);

        assert.equal(Result.ok, true);
        assert.equal(Result.ok && Result.Username, "Trim_Me");
        assert.equal(Result.ok && GetDb().select().from(users).where(eq(users.userId, Result.UserId)).get()?.name, "Trim_Me");
        assert.deepEqual(RegisterAccount("OPEN", " trim_me ", undefined), { ok: false, Status: 409, Error: "username_taken" });
        assert.deepEqual(RegisterAccount("OPEN", "   ", undefined), { ok: false, Status: 400, Error: "username_invalid" });
        assert.deepEqual(RegisterAccount("OPEN", "in side", undefined), { ok: false, Status: 400, Error: "username_invalid" });
    });

    it("lets only one of two registrations of the same name through", () => {
        const Results = [RegisterAccount("OPEN", "Twin_Name", undefined), RegisterAccount("OPEN", "TWIN_NAME", undefined)];

        assert.deepEqual(Results.map((Result) => Result.ok), [true, false]);
        assert.equal(Count("users", "lower(name) = 'twin_name'"), 1);
    });
});

describe("RenameAccount", () => {
    it("renames the account and its characters together", async () => {
        const Registered = RegisterAccount("OPEN", "Old_Name", undefined);
        assert.ok(Registered.ok);

        const Character = await CreateCharacterForUid(Registered.UserId, "Old_Name");
        const Result = RenameAccount({ UserId: Registered.UserId }, "New_Name");

        assert.deepEqual(Result, { ok: true, UserId: Registered.UserId, OldUsername: "Old_Name", Username: "New_Name", Characters: 1 });
        assert.equal(GetDb().select().from(users).where(eq(users.userId, Registered.UserId)).get()?.name, "New_Name");
        assert.equal(ReadCharacter(Character.id).name, "New_Name");
        assert.equal(await DisplayNameForUserId(Registered.UserId), "New_Name");
        assert.equal(RegisterAccount("OPEN", "new_name", undefined).ok, false);
        assert.equal(RegisterAccount("OPEN", "Old_Name", undefined).ok, true, "the old name is free again");
    });

    it("finds the account by its current name in any case, and allows a change of case", () => {
        const Result = RenameAccount({ Username: "new_NAME" }, " NEW_name ");

        assert.equal(Result.ok, true);
        assert.equal(Result.ok && Result.OldUsername, "New_Name");
        assert.equal(Result.ok && Result.Username, "NEW_name");
    });

    it("refuses a taken, invalid or missing name and an unknown account", () => {
        assert.deepEqual(RenameAccount({ Username: "NEW_name" }, "slayer"), { ok: false, Status: 409, Error: "username_taken" });
        assert.deepEqual(RenameAccount({ Username: "NEW_name" }, "x"), { ok: false, Status: 400, Error: "username_invalid" });
        assert.deepEqual(RenameAccount({ Username: "NEW_name" }, undefined), { ok: false, Status: 400, Error: "bad_request" });
        assert.deepEqual(RenameAccount({}, "Fine_Name"), { ok: false, Status: 400, Error: "bad_request" });
        assert.deepEqual(RenameAccount({ UserId: "UID-nobody" }, "Fine_Name"), { ok: false, Status: 404, Error: "not_found" });
        assert.deepEqual(RenameAccount({ Username: "Nobody_Here" }, "Fine_Name"), { ok: false, Status: 404, Error: "not_found" });
    });
});

describe("GenerateInviteCode", () => {
    it("makes three groups of four unambiguous characters that fit the invite string rules", () => {
        const Codes = new Set<string>();

        for(let Index = 0; Index < 200; Index++){
            const Code = GenerateInviteCode();

            assert.match(Code, /^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/);
            assert.match(Code, /^[A-Za-z0-9-]{4,64}$/);
            Codes.add(Code);
        }

        assert.equal(Codes.size, 200);
    });
});

describe("DisplayNameForUserId", () => {
    it("is the stored name, \"\" for an unknown account, and {} again with ACCOUNT_DISPLAY_NAME=0", async () => {
        assert.equal(await DisplayNameForUserId("UID-owner"), "Slayer");
        assert.equal(await DisplayNameForUserId("UID-nobody"), "");
        assert.equal(await DisplayNameForUserId(undefined), "");

        process.env.ACCOUNT_DISPLAY_NAME = "0";

        try{
            assert.deepEqual(await DisplayNameForUserId("UID-owner"), {});
        }
        finally{
            delete process.env.ACCOUNT_DISPLAY_NAME;
        }
    });

    it("keeps the key table and the account table in step", () => {
        assert.equal(Count("users", "userId like 'UID-%' and userId != 'UID-owner'"), GetDb().select().from(userapikeys).all().length);
    });
});
