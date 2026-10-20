import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import jwt from "jsonwebtoken";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { ResetPartiesForTests } from "../src/controllers/party";

// The two small social fixes taken from Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7): a
// repeated party accept answers the party instead of 404, and GET /account/api/oauth/verify names the
// caller's own account. The fork's account-lookup case is ported with our answers where they differ: verify
// never answers 401 (the fork did without a token), and the mapping reply is ours (accountType "phoenix", with
// the envelope keys the client ignores).

const STUB_ACCOUNT = "9626f441055349ce8cb7d7d5a483eaa2";
const BUILD = { buildId: "239827", featureOverrides: [] };

before(async () => {
    GetDb().insert(users).values([
        { userId: "UID-soc-john", name: "LookupJohn", notes: 0, isAdmin: false },
        { userId: "UID-soc-manda", name: "LookupManda", notes: 0, isAdmin: false },
        { userId: "UID-soc-todd", name: "LookupTodd", notes: 0, isAdmin: false }
    ]).run();
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    ResetPartiesForTests();
});

const John = "UID-soc-john", Manda = "UID-soc-manda", Todd = "UID-soc-todd";

const Poll = (Who: string) => Call("POST", "/party", { as: Who, body: {} });
const Invite = (From: string, To: string, PartyId: string) => Call("PUT", "/party/invite", { as: From, body: { recipientPlayerId: To, partyId: PartyId, ...BUILD } });
const Accept = (Who: string, Id: string) => Call("PUT", `/party/invite/accept/${Id}`, { as: Who, body: { recipientPlayerId: Who, partyId: Id, ...BUILD } });

function SignWith(Uid: string, Options: jwt.SignOptions){
    const Key = Buffer.from(process.env.AUTH_SIGNING_PRIVKEY_B64!, "base64").toString("utf8");
    return jwt.sign({ userId: Uid }, Key, { algorithm: "RS256", issuer: "undaunted-metagame", audience: "undaunted-metagame", ...Options });
}

const Verify = (Headers: Record<string, string> = {}) => Call("GET", "/account/api/oauth/verify", { headers: Headers });

describe("a repeated party accept", () => {
    it("answers 200 with the party the caller already joined, named by its party id or by another member", async () => {
        const PartyJohn = (await Poll(John)).json.partyId;

        assert.equal((await Invite(John, Manda, PartyJohn)).status, 200);

        const First = await Accept(Manda, PartyJohn);
        assert.equal(First.status, 200);
        assert.deepEqual(First.json.playerIds, [John, Manda]);

        const Again = await Accept(Manda, PartyJohn);
        assert.equal(Again.status, 200, "the fork's retry case: the invite is used up, the party is the answer");
        assert.deepEqual(Again.json, First.json);
        assert.deepEqual(Again.json, (await Poll(Manda)).json, "the same as the poll");

        assert.deepEqual((await Accept(Manda, John)).json.partyId, PartyJohn, "named by the sender, a member");
        assert.equal((await Accept(Manda, Manda)).status, 404, "not by the caller's own id");
        assert.equal((await Accept(Manda, "not-a-party")).status, 404);
        assert.equal((await Accept(Todd, PartyJohn)).status, 404, "a player who is not in it and has no invite");
        assert.deepEqual((await Poll(John)).json.playerIds, [John, Manda], "nothing changed");
    });

    it("stays 404 for a party of one and for a party the caller left", async () => {
        const PartyJohn = (await Poll(John)).json.partyId;
        const PartyTodd = (await Poll(Todd)).json.partyId;

        assert.equal((await Accept(Todd, PartyTodd)).status, 404, "alone in a party of one");

        assert.equal((await Invite(John, Manda, PartyJohn)).status, 200);
        assert.equal((await Accept(Manda, PartyJohn)).status, 200);
        assert.equal((await Call("DELETE", "/party/member", { as: Manda })).status, 200);
        assert.equal((await Accept(Manda, PartyJohn)).status, 404, "Manda left the party");
    });
});

describe("GET /account/api/oauth/verify", () => {
    // from Harmonicrain/Undaunted test/social-friends.test.js:57, with our answers
    it("names the caller's account, the lookups find the other player, and the mapping maps them", async () => {
        const Named = await Call("GET", "/account/api/public/account/displayName/lookupmanda", { as: John });
        assert.equal(Named.json.id, Manda);

        const ById = await Call("GET", `/account/api/public/account/${Manda}`, { as: John });
        assert.equal(ById.json.displayName, "LookupManda");

        const Verified = await Call("GET", "/account/api/oauth/verify", { as: John });
        assert.equal(Verified.status, 200);
        assert.equal(Verified.json.account_id, John);
        assert.equal(Verified.json.active, true);
        assert.equal(Verified.json.expires_at, "2085-09-09T01:01:01.703Z", "still far off");

        const Mapping = await Call("POST", "/account/mapping", { as: John, body: { srcAccountType: "epic", ids: [Manda] } });
        assert.deepEqual(Mapping.json.accountMappings, { [Manda]: { accountId: Manda, accountType: "phoenix" } });

        const PublicInfo = await Call("POST", "/accountinfo/public", { as: John, body: { accountId: Manda } });
        assert.equal(PublicInfo.json.accountId, Manda);
        assert.equal(PublicInfo.json.linkedAccounts[0].accountId, Manda);
    });

    it("never answers 401: no token, a bad one and an expired one get the old stub with 200", async () => {
        for(const [Label, Headers] of [
            ["no token", {}],
            ["not a token", { authorization: "bearer not-a-token" }],
            ["an expired token", { authorization: `bearer ${SignWith(John, { expiresIn: -60 })}` }],
            ["another key's token", { authorization: `bearer ${jwt.sign({ userId: John }, "not-the-key")}` }]
        ] as [string, Record<string, string>][]){
            const Reply = await Verify(Headers);

            assert.equal(Reply.status, 200, Label);
            assert.equal(Reply.json.account_id, STUB_ACCOUNT, Label);
            assert.equal(Reply.json.active, true, Label);
        }

        assert.equal((await Verify({ authorization: `bearer ${SignWith(Manda, { expiresIn: 60 })}` })).json.account_id, Manda, "a valid one");
    });

    it("VERIFY_STUB_ACCOUNT=1 answers the stub to everyone again", async () => {
        await WithEnv({ VERIFY_STUB_ACCOUNT: "1" }, async () => {
            assert.equal((await Call("GET", "/account/api/oauth/verify", { as: John })).json.account_id, STUB_ACCOUNT);
        });

        assert.equal((await Call("GET", "/account/api/oauth/verify", { as: John })).json.account_id, John);
    });
});
