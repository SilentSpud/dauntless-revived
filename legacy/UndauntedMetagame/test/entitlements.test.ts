import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { GrantEntitlementInTx, HasActiveEntitlement, ListEntitlements, RevokeEntitlementInTx } from "../src/controllers/entitlements";
import { MakePlayer } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

const Names = (UserId: string) => ListEntitlements(UserId).map((Entitlement) => Entitlement.name);
const Grant = (UserId: string, Name: string, Duration: number) => GetDb().transaction((tx) => GrantEntitlementInTx(tx, UserId, Name, Duration, "test"));
const Revoke = (UserId: string, Name: string) => GetDb().transaction((tx) => RevokeEntitlementInTx(tx, UserId, Name));

describe("entitlements", () => {
    it("every account owns the Elite Hunt Pass by default, permanently, as {name, duration, activatedDate}", async () => {
        const {UserId} = await MakePlayer();
        const List = ListEntitlements(UserId);

        assert.deepEqual(List.map((Entitlement) => Entitlement.name), ["season_free_any", "season_premium_any", "season09b_premium"]);
        for(const Entitlement of List){
            assert.deepEqual(Object.keys(Entitlement), ["name", "duration", "activatedDate"]);
            assert.equal(Entitlement.duration, 0);
            assert.match(Entitlement.activatedDate, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/);
        }
        assert.ok(GetDb().transaction((tx) => HasActiveEntitlement(tx, UserId, "season09b_premium")));
    });

    it("a grant is listed once, a second identical grant does not duplicate it", async () => {
        const {UserId} = await MakePlayer();

        assert.ok(Grant(UserId, "x", 0).some((Entitlement) => Entitlement.name === "x"));
        assert.equal(Grant(UserId, "x", 0).filter((Entitlement) => Entitlement.name === "x").length, 1);
    });

    it("a timed grant still running is extended; an expired one is left out of every list", async () => {
        const {UserId} = await MakePlayer();

        Grant(UserId, "escalation_xp_boost_entitlement", 2);
        const Extended = Grant(UserId, "escalation_xp_boost_entitlement", 24).find((Entitlement) => Entitlement.name === "escalation_xp_boost_entitlement")!;
        assert.equal(Extended.duration, 26);

        // A 2-hour boost activated 3 hours ago
        Grant(UserId, "boost_old", 2);
        GetDb().$client.prepare("update entitlements set activatedDate = ? where accountId = ? and name = 'boost_old'").run(new Date(Date.now() - 3 * 3600 * 1000).toISOString(), UserId);
        assert.ok(!Names(UserId).includes("boost_old"));
        assert.ok(!GetDb().transaction((tx) => HasActiveEntitlement(tx, UserId, "boost_old")));

        // Granting it again starts it over
        Grant(UserId, "boost_old", 2);
        assert.ok(Names(UserId).includes("boost_old"));
    });

    it("a revoked entitlement, default or not, stays revoked until granted again", async () => {
        const {UserId} = await MakePlayer();

        assert.equal(Revoke(UserId, "season09b_premium"), true);
        assert.equal(Revoke(UserId, "season09b_premium"), false);
        assert.ok(!Names(UserId).includes("season09b_premium"));

        Grant(UserId, "season09b_premium", 0);
        assert.ok(Names(UserId).includes("season09b_premium"));
    });
});
