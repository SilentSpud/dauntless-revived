import { RemoveTestDb } from "./setup";
import "./authenv";
import { PARTY_DEPLOYSERVER_PORT, RecordingDeploy, StartRecordingDeploy, UsePartyEnv } from "./partyenv";
UsePartyEnv(PARTY_DEPLOYSERVER_PORT);
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import {
    AcceptPartyInvite, DeclinePartyInvites, EvictPartyLeader, GetPartyOf, InviteToParty, KickPartyMember, LeaveParty, ListPartyInvites,
    PartiesOfPlayers, PollParty, PromotePartyMember, ResetPartiesForTests, SetPartyClockForTests, TouchPlayer
} from "../src/controllers/party";
import { CancelMatchmaking, CheckAndUpdateQueueStatus, DecideCandidateStatus, HandlePlayerMatchmaking, JoinPartyCandidateById, LeaveCandidate, ResetMatchmakingForTests } from "../src/controllers/matchmaking";
import { BlockPlayer, UnblockPlayer } from "../src/controllers/friends";

// Parties (roadmap 1.9) at the controller level: the party and invite store with a
// controllable clock, and the party matchmaking against a recording fake deploy server on a
// spare loopback port. partyhttp.test.ts walks the same through the HTTP routes.

const A = "UID-a", B = "UID-b", C = "UID-c", D = "UID-d", E = "UID-e";
const NAMES: Record<string, string> = { [A]: "Alpha", [B]: "Bravo", [C]: "Charlie", [D]: "Delta", [E]: "Echo" };
const HUNT = "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr";
const TUTORIAL = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

let Now = Date.parse("2026-09-21T12:00:00.000Z");
let Deploy: RecordingDeploy;

function Advance(Seconds: number){
    Now += Seconds * 1000;
}

before(async () => {
    for(const [UserId, Name] of Object.entries(NAMES)){
        GetDb().insert(users).values({ userId: UserId, name: Name, notes: 0 }).run();
    }

    SetPartyClockForTests(() => Now);
    Deploy = await StartRecordingDeploy(PARTY_DEPLOYSERVER_PORT);
});

after(async () => {
    SetPartyClockForTests();
    await Deploy?.Close();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    ResetPartiesForTests();
    ResetMatchmakingForTests();
    Deploy.Calls.length = 0;
    Deploy.Fail = false;
    Deploy.Capacity = false;
    Deploy.DelayMs = 0;
    Deploy.NextHuntPort = 8775;
});

// B leads, A (and more) accepted
async function FormParty(Leader: string, ...Members: string[]){
    const Party = await PollParty(Leader) as any;

    for(const Member of Members){
        await PollParty(Member);
        assert.equal(InviteToParty(Leader, Member, Party.partyId).Status, 200, `invite ${Member}`);
        assert.equal((await AcceptPartyInvite(Member, Party.partyId)).Status, 200, `accept ${Member}`);
    }

    return Party.partyId as string;
}

async function WaitForDeployCalls(Count: number){
    const Deadline = Date.now() + 2000;

    while(Deploy.Calls.length < Count && Date.now() < Deadline){
        await new Promise<void>((Resolve) => setTimeout(Resolve, 5));
    }

    assert.equal(Deploy.Calls.length, Count);
}

async function WaitForCandidateKind(Player: string, Kind: "unknown" | "failed" | "matching" | "travel"){
    const Deadline = Date.now() + 2000;
    let Decision = await DecideCandidateStatus(Player);

    while(Decision.Kind !== Kind && Date.now() < Deadline){
        await new Promise<void>((Resolve) => setTimeout(Resolve, 5));
        Decision = await DecideCandidateStatus(Player);
    }

    assert.equal(Decision.Kind, Kind, Player);
    return Decision;
}

describe("a party of one (every player on their own)", () => {
    it('fails every party member after one minute of unavailable capacity', async () => {
        await FormParty(B, A, C);
        Deploy.Capacity = true;
        await HandlePlayerMatchmaking('ISLAND', '', HUNT, B);
        Advance(61);
        for (const member of [A, B, C]) {
            assert.equal((await DecideCandidateStatus(member)).Kind, 'failed');
        }
        assert.equal(Deploy.Calls.length, 1, 'no allocation after the deadline');
    });
    it('a whole party waits for capacity and retries once with members still present', async () => {
        await FormParty(B, A, C);
        Deploy.Capacity = true;
        await HandlePlayerMatchmaking('ISLAND', '', HUNT, B);
        assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready, false);
        assert.equal(Deploy.Calls.length, 1);
        LeaveParty(C);
        Advance(11);
        Deploy.Capacity = false;
        await Promise.all([CheckAndUpdateQueueStatus(A), CheckAndUpdateQueueStatus(B)]);
        assert.equal(Deploy.Calls.length, 2);
        assert.deepEqual([...Deploy.Calls[1].ExpectedPlayers].sort(), [A, B].sort());
        assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready, true);
        assert.equal((await CheckAndUpdateQueueStatus(B))?.Ready, true);
    });
    it("answers the old stub's reply, with a UUID party id and the account's name", async () => {
        const Reply = await PollParty(A) as any;

        assert.match(Reply.partyId, /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
        assert.deepEqual(Reply, {
            candidateId: "CANDIDATE_ID_LOL",
            candidateState: "QUEUED_FOR_START",
            gauntletLevel: null,
            leaderPlayerId: A,
            partyId: Reply.partyId,
            playerHuntId: null,
            playerStates: [{ consoleSessionId: null, displayName: "Alpha", isMemberOfCandidate: true, platform: "win", playerId: A }]
        });
        assert.deepEqual(Object.keys(Reply), ["candidateId", "candidateState", "gauntletLevel", "leaderPlayerId", "partyId", "playerHuntId", "playerStates"]);
        assert.equal((await PollParty(A) as any).partyId, Reply.partyId, "the same party on the next poll");
        assert.deepEqual(ListPartyInvites(A), { invitations: [] });
    });
});

describe("T1: invite and accept", () => {
    it("B invites A; A sees the invite and accepts with the party id; both polls show the party", async () => {
        const PartyB = (await PollParty(B) as any).partyId;
        const OldA = (await PollParty(A) as any).partyId;

        assert.deepEqual(InviteToParty(B, A, PartyB), { Status: 200, Body: {} });
        assert.deepEqual(ListPartyInvites(A), {
            invitations: [{ recipientPlayerId: A, sendingPlayerId: B, partyId: PartyB, sendingPlatform: "win", sendingDisplayName: "Bravo" }]
        });
        assert.deepEqual(ListPartyInvites(B), { invitations: [] }, "the sender sees no invite");

        const Accepted = await AcceptPartyInvite(A, PartyB);
        const Expected = {
            partyId: PartyB,
            leaderPlayerId: B,
            playerIds: [B, A],
            playerStates: [
                { playerId: B, isMemberOfCandidate: false, platform: "win", displayName: "Bravo", consoleSessionId: null },
                { playerId: A, isMemberOfCandidate: false, platform: "win", displayName: "Alpha", consoleSessionId: null }
            ],
            candidateState: null,
            candidateId: null,
            playerHuntId: null,
            gauntletLevel: null
        };

        assert.equal(Accepted.Status, 200);
        assert.deepEqual(Accepted.Body, Expected);
        assert.deepEqual(await PollParty(A), Expected);
        assert.deepEqual(await PollParty(B), Expected);
        assert.deepEqual(ListPartyInvites(A), { invitations: [] }, "the invite is used up");
        assert.equal(GetPartyOf(A)?.PartyId, PartyB);
        assert.notEqual(OldA, PartyB);
    });

    it("the invite id may also be the sender's id (the client's decline body carries it in both fields)", async () => {
        const PartyB = (await PollParty(B) as any).partyId;

        InviteToParty(B, A, PartyB);
        const Accepted = await AcceptPartyInvite(A, B);

        assert.equal(Accepted.Status, 200);
        assert.deepEqual((Accepted.Body as any).playerIds, [B, A]);
    });

    it("an unknown invite id is 404, and accepting twice is harmless", async () => {
        const PartyB = (await PollParty(B) as any).partyId;

        assert.equal((await AcceptPartyInvite(A, PartyB)).Status, 404, "no invite yet");
        InviteToParty(B, A, PartyB);
        assert.equal((await AcceptPartyInvite(A, "not-a-party")).Status, 404);
        assert.equal((await AcceptPartyInvite(A, PartyB)).Status, 200);
        assert.deepEqual(ListPartyInvites(A).invitations, [], "the invite is gone once used");
        // A repeated accept answers the party A already joined (it was 404 before the Harmonic port)
        const Again = await AcceptPartyInvite(A, PartyB);
        assert.equal(Again.Status, 200);
        assert.deepEqual((Again.Body as any).playerIds, [B, A]);
        assert.deepEqual(GetPartyOf(A)?.Members, [B, A]);
    });
});

describe("invite checks", () => {
    it("only the leader invites; not yourself, not a member, not an unknown account, not twice", async () => {
        const PartyB = await FormParty(B, A);

        assert.equal(InviteToParty(A, C, PartyB).Status, 403, "A is not the leader");
        assert.equal(InviteToParty(B, B, PartyB).Status, 409, "yourself");
        assert.equal(InviteToParty(B, A, PartyB).Status, 409, "already a member");
        assert.equal(InviteToParty(B, "UID-nobody", PartyB).Status, 404, "unknown account");
        assert.equal(InviteToParty(B, undefined, PartyB).Status, 404, "no recipient");
        assert.equal(InviteToParty(B, "../../etc", PartyB).Status, 404, "not an account id");
        assert.equal(InviteToParty(B, C, PartyB).Status, 200);
        assert.equal(InviteToParty(B, C, PartyB).Status, 409, "already invited");
        assert.equal(ListPartyInvites(C).invitations.length, 1);
    });

    it("a wrong party id in the body is logged and the caller's own party used", async () => {
        const PartyB = (await PollParty(B) as any).partyId;

        assert.equal(InviteToParty(B, A, "some-other-party").Status, 200);
        assert.equal(ListPartyInvites(A).invitations[0].partyId, PartyB);
    });

    it("a party holds 4: the 5th is refused at invite and at accept", async () => {
        const PartyA = await FormParty(A, B, C);

        assert.equal(InviteToParty(A, D, PartyA).Status, 200);
        assert.equal(InviteToParty(A, E, PartyA).Status, 200);
        assert.equal((await AcceptPartyInvite(D, PartyA)).Status, 200);
        assert.equal(GetPartyOf(A)?.Members.length, 4);
        assert.equal((await AcceptPartyInvite(E, PartyA)).Status, 409, "full");
        assert.equal(GetPartyOf(E), undefined);
    });

    it("a block either way stops invites", async () => {
        await PollParty(B);
        assert.equal(BlockPlayer(A, B).ok, true);
        assert.equal(InviteToParty(B, A, undefined).Status, 403, "A blocked B");
        assert.equal(InviteToParty(A, B, undefined).Status, 403, "and the other way round");
        UnblockPlayer(A, B);
        assert.equal(InviteToParty(B, A, undefined).Status, 200);
    });

    it("invites expire after 5 minutes and die with their party", async () => {
        const PartyB = (await PollParty(B) as any).partyId;

        InviteToParty(B, A, PartyB);
        Advance(299);
        TouchPlayer(B);
        assert.equal(ListPartyInvites(A).invitations.length, 1);
        Advance(2);
        assert.equal(ListPartyInvites(A).invitations.length, 0, "expired");

        TouchPlayer(B);
        InviteToParty(B, C, PartyB);
        LeaveParty(B);
        assert.equal(ListPartyInvites(C).invitations.length, 0, "the sender left: gone");
    });
});

describe("T7: decline, leave, kick, promote", () => {
    it("decline removes the invite (body carries the id in both fields); a sender can withdraw", async () => {
        const PartyB = (await PollParty(B) as any).partyId;

        InviteToParty(B, A, PartyB);
        InviteToParty(B, C, PartyB);
        assert.deepEqual(DeclinePartyInvites(A, { sendingPlayerId: PartyB, recipientPlayerId: A, partyId: PartyB }), { Status: 200, Body: {} });
        assert.equal(ListPartyInvites(A).invitations.length, 0);

        assert.equal(DeclinePartyInvites(B, { sendingPlayerId: B, recipientPlayerId: C, partyId: PartyB }).Status, 200);
        assert.equal(ListPartyInvites(C).invitations.length, 0, "withdrawn by the sender");

        InviteToParty(B, C, PartyB);
        assert.equal(DeclinePartyInvites(A, { sendingPlayerId: PartyB, recipientPlayerId: C, partyId: PartyB }).Status, 200);
        assert.equal(ListPartyInvites(C).invitations.length, 1, "someone else cannot remove C's invite");
    });

    it("the leader leaving hands the lead to the next member; the leaver gets a new party of one", async () => {
        const PartyA = await FormParty(A, B, C);

        assert.deepEqual(LeaveParty(A), { Status: 200, Body: {} });
        const PartyNow = await PollParty(B) as any;
        assert.equal(PartyNow.partyId, PartyA);
        assert.equal(PartyNow.leaderPlayerId, B);
        assert.deepEqual(PartyNow.playerIds, [B, C]);

        const Alone = await PollParty(A) as any;
        assert.notEqual(Alone.partyId, PartyA);
        assert.equal(Alone.candidateId, "CANDIDATE_ID_LOL");

        LeaveParty(C);
        const Solo = await PollParty(B) as any;
        assert.equal(Solo.partyId, PartyA, "B keeps the party id");
        assert.equal(Solo.candidateState, "QUEUED_FOR_START", "a party of one answers the old stub");
        assert.equal(LeaveParty("UID-nobody").Status, 200, "leaving without a party is harmless");
    });

    it("only the leader kicks; the kicked member is on their own after", async () => {
        const PartyA = await FormParty(A, B, C);

        assert.equal(KickPartyMember(B, C).Status, 403, "B is not the leader");
        assert.equal(KickPartyMember(A, D).Status, 404, "not a member");
        assert.equal(KickPartyMember(A, C).Status, 200);
        assert.deepEqual((await PollParty(A) as any).playerIds, [A, B]);
        assert.notEqual((await PollParty(C) as any).partyId, PartyA);
        assert.equal(KickPartyMember(B, B).Status, 200, "naming yourself is leaving");
        assert.equal((await PollParty(A) as any).candidateId, "CANDIDATE_ID_LOL");
    });

    it("only the leader promotes, and only a member", async () => {
        await FormParty(A, B);

        assert.equal(PromotePartyMember(B, B).Status, 403);
        assert.equal(PromotePartyMember(A, C).Status, 404);
        assert.equal(PromotePartyMember(A, B).Status, 200);
        assert.equal((await PollParty(A) as any).leaderPlayerId, B);
        assert.equal(InviteToParty(A, C, undefined).Status, 403, "A no longer leads");
        assert.equal(InviteToParty(B, C, undefined).Status, 200);
    });
});

describe("T4: members who stay, and members who vanish", () => {
    it("members that keep polling stay; the client's leader eviction is ignored while the leader is heard from", async () => {
        await FormParty(A, B);

        for(let Poll = 0; Poll < 12; Poll++){
            Advance(10);
            await PollParty(A);
            await PollParty(B);
        }

        assert.equal(EvictPartyLeader(B, A).Status, 200);
        assert.deepEqual(GetPartyOf(B)?.Members, [A, B], "eviction ignored: A polled 10 s ago");
        assert.equal(GetPartyOf(B)?.LeaderId, A);
    });

    it("heartbeats count as being heard from (hunts, loading screens)", async () => {
        await FormParty(A, B);

        for(let Beat = 0; Beat < 10; Beat++){
            Advance(20);
            TouchPlayer(B);
            await PollParty(A);
        }

        assert.deepEqual(GetPartyOf(A)?.Members, [A, B]);
    });

    it("a member nobody has heard from for 120 s is dropped; a vanished leader is replaced", async () => {
        await FormParty(A, B, C);

        Advance(100);
        await PollParty(A);
        await PollParty(B);
        Advance(30);
        assert.deepEqual((await PollParty(A) as any).playerIds, [A, B], "C dropped after 130 s");

        Advance(121);
        assert.equal(EvictPartyLeader(B, A).Status, 200);
        assert.deepEqual(GetPartyOf(B)?.Members, [B], "A, not heard from for 121 s, evicted on B's request");
        assert.equal(GetPartyOf(A), undefined);
    });
});

describe("POST /party/status", () => {
    it("lists the parties of the asked players and only the caller's own invitations", async () => {
        const PartyA = await FormParty(A, B);
        await PollParty(C);
        InviteToParty(A, C, PartyA);
        await PollParty(D);
        InviteToParty(D, E, undefined);

        const Result = await PartiesOfPlayers(C, [A, B, C, "UID-nobody", 5]);
        const Body = Result.Body as any;

        assert.equal(Result.Status, 200);
        assert.deepEqual(Body.parties.map((Party: any) => Party.partyId).sort(), [PartyA, GetPartyOf(C)!.PartyId].sort());
        assert.deepEqual(Body.invitations.map((Invite: any) => Invite.recipientPlayerId), [C]);
        assert.deepEqual(((await PartiesOfPlayers(A, "nope")).Body as any), { parties: [], invitations: [] });
    });
});

describe("T5: the whole party on one hunt server", () => {
    it("the leader's join starts one server with every member expected; members follow through their polls", async () => {
        await FormParty(A, B, C);

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true);
        assert.equal(Deploy.Calls.length, 1, "one deploy call for the whole party");
        assert.deepEqual(Deploy.Calls[0], { GameMode: "ISLAND", GameArgs: "", HuntId: HUNT, ExpectedPlayers: [A, B, C] });

        const Party = await PollParty(B) as any;
        assert.equal(Party.candidateState, "IN_PROGRESS");
        assert.equal(Party.playerHuntId, HUNT);
        assert.ok(Party.playerStates.every((State: any) => State.isMemberOfCandidate));
        assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, Party.candidateId, "the leader's join answer carries the party's candidate");

        for(const Member of [A, B, C]){
            const Decision = await DecideCandidateStatus(Member);
            assert.equal(Decision.Kind, "travel", Member);
            assert.deepEqual(Decision.Kind === "travel" && [Decision.Entry.Host, Decision.Entry.Port, Decision.Entry.CandidateId], ["127.0.0.1", 8775, Party.candidateId]);
            assert.deepEqual(Decision.Kind === "travel" && Decision.Entry.PartyMemberIds, [A, B, C]);
        }

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, B), true, "a member's own join of the same hunt: nothing new");
        assert.equal(Deploy.Calls.length, 1);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "CR19_PlayerHunt_Other", B), false, "a member cannot start another hunt (400)");
        assert.equal(Deploy.Calls.length, 1);
    });

    it("a member may follow by joining the candidate by id (the exe's CandidateJoin); nobody else may", async () => {
        await FormParty(A, B);
        await PollParty(C);
        Deploy.DelayMs = 3000;
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        const CandidateId = (await PollParty(B) as any).candidateId;

        assert.equal(JoinPartyCandidateById(B, CandidateId)?.HuntId, HUNT);
        assert.equal((await DecideCandidateStatus(B)).Kind, "matching", "still starting");
        assert.equal(JoinPartyCandidateById(C, CandidateId), undefined, "not in the party");
        assert.equal(JoinPartyCandidateById(B, "some-other-candidate"), undefined);

        await new Promise((Resolve) => setTimeout(Resolve, 800));
        assert.equal((await DecideCandidateStatus(B)).Kind, "travel", "then sent like any member");
        assert.equal(Deploy.Calls.length, 1);
    });

    it("the leader's second join after travelling (the client always sends one) starts nothing; MATCHING for 20 s, then the same server", async () => {
        await FormParty(A, B);
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        assert.equal((await DecideCandidateStatus(A)).Kind, "travel");

        Advance(25);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true);
        assert.equal(Deploy.Calls.length, 1, "no second server");
        const Parked = await DecideCandidateStatus(A);
        assert.equal(Parked.Kind, "matching");
        assert.equal(Parked.Kind === "matching" && Parked.Parked, true);

        Advance(21);
        const Again = await DecideCandidateStatus(A);
        assert.equal(Again.Kind, "travel", "still asking after 20 s: back to the party's server");
        assert.equal(Again.Kind === "travel" && Again.Entry.Port, 8775);
    });

    it("members who poll before the server is up get MATCHING, then IN_PROGRESS", async () => {
        await FormParty(A, B);
        Deploy.DelayMs = 3500;

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true, "answers after 2.5 s at most");
        assert.equal((await PollParty(B) as any).candidateState, "MATCHING");
        assert.equal((await DecideCandidateStatus(B)).Kind, "matching");

        await new Promise((Resolve) => setTimeout(Resolve, 1500));
        assert.equal((await PollParty(B) as any).candidateState, "IN_PROGRESS");
        assert.equal((await DecideCandidateStatus(B)).Kind, "travel");
    });

    it("the candidate leaves the party poll 60 s after the last member was sent", async () => {
        await FormParty(A, B);
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        await DecideCandidateStatus(A);
        Advance(30);
        await DecideCandidateStatus(B);

        Advance(59);
        assert.notEqual((await PollParty(A) as any).candidateId, null);
        Advance(2);
        const Party = await PollParty(A) as any;
        assert.deepEqual([Party.candidateId, Party.candidateState, Party.playerHuntId], [null, null, null]);
        assert.ok(Party.playerStates.every((State: any) => State.isMemberOfCandidate === false));

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, B), true, "a member's late join of the hunt they are on is answered from the last candidate");
        assert.equal(Deploy.Calls.length, 1);
    });

    it("a failed launch tells every member FAILED and clears the candidate", async () => {
        await FormParty(A, B);
        Deploy.Fail = true;

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true);
        assert.equal((await DecideCandidateStatus(A)).Kind, "failed");
        assert.equal((await DecideCandidateStatus(B)).Kind, "failed");
        assert.equal((await PollParty(B) as any).candidateState, null);
    });

    it("a member who leaves before the server is up stays behind", async () => {
        await FormParty(A, B, C);
        Deploy.DelayMs = 3000;

        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        LeaveParty(C);
        assert.equal((await DecideCandidateStatus(C)).Kind, "unknown");

        await new Promise((Resolve) => setTimeout(Resolve, 800));
        assert.equal((await DecideCandidateStatus(B)).Kind, "travel");
        assert.deepEqual(Deploy.Calls[0].ExpectedPlayers, [A, B, C], "the server was asked for before C left");
    });

    it("the leader's cancel (only with MATCHMAKING_CANCEL=1) calls it off for members not sent yet", async () => {
        await FormParty(A, B);
        Deploy.DelayMs = 3000;

        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        CancelMatchmaking(A);
        assert.equal((await PollParty(B) as any).candidateId, null);
        assert.equal((await DecideCandidateStatus(B)).Kind, "unknown");
        assert.equal((await DecideCandidateStatus(A)).Kind, "unknown");
        await new Promise((Resolve) => setTimeout(Resolve, 800));
        assert.equal((await DecideCandidateStatus(B)).Kind, "unknown", "the late server answer changes nothing");
    });

    it("a member's candidate leave takes only that member out", async () => {
        await FormParty(A, B, C);
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);

        LeaveCandidate(C);
        const Party = await PollParty(A) as any;
        assert.equal(Party.candidateState, "IN_PROGRESS");
        assert.deepEqual(Party.playerStates.map((State: any) => State.isMemberOfCandidate), [true, true, false]);
        assert.equal((await DecideCandidateStatus(B)).Kind, "travel");
    });

    it("the tutorial (its own game args) and a party of one use the solo code", async () => {
        await FormParty(A, B);

        assert.equal(await HandlePlayerMatchmaking("ISLAND", TUTORIAL, "", B), true);
        assert.equal(Deploy.Calls[0].ExpectedPlayers, undefined);
        assert.equal((await PollParty(A) as any).candidateId, null, "no party candidate");

        ResetPartiesForTests();
        await PollParty(C);
        assert.equal(await HandlePlayerMatchmaking("CITY", "", "ShatteredIsles_ReturnToRamsgate", C), true);
        assert.equal((await DecideCandidateStatus(C)).Kind, "travel");
    });
});

describe("T6: back to Ramsgate together", () => {
    it("the leader's CITY join takes the members on the leader's server along, and nobody else", async () => {
        await FormParty(A, B, C);
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);
        await DecideCandidateStatus(A);
        await DecideCandidateStatus(B);
        // C went back to Ramsgate on their own (a member's CITY join is theirs alone)
        assert.equal(await HandlePlayerMatchmaking("CITY", "", "ShatteredIsles_ReturnToRamsgate", C), true);
        assert.equal((await DecideCandidateStatus(C)).Kind, "travel");
        Deploy.Calls.length = 0;

        assert.equal(await HandlePlayerMatchmaking("CITY", "", "ShatteredIsles_ReturnToRamsgate", A), true);
        assert.equal(Deploy.Calls.length, 1);
        assert.equal(Deploy.Calls[0].GameMode, "CITY");

        const Party = await PollParty(B) as any;
        assert.equal(Party.playerHuntId, "ShatteredIsles_ReturnToRamsgate");
        assert.deepEqual(Party.playerStates.map((State: any) => [State.playerId, State.isMemberOfCandidate]), [[A, true], [B, true], [C, false]]);

        for(const Member of [A, B]){
            const Decision = await DecideCandidateStatus(Member);
            assert.equal(Decision.Kind === "travel" && Decision.Entry.Port, 8777, Member);
        }
    });

    it("a leader whose members are elsewhere goes alone", async () => {
        await FormParty(A, B);

        assert.equal(await HandlePlayerMatchmaking("SHARED", "", "ShatteredIsles_TrainingDojo", A), true);
        assert.equal((await PollParty(B) as any).candidateId, null);
        assert.equal((await DecideCandidateStatus(A)).Kind, "travel");
        assert.equal((await DecideCandidateStatus(B)).Kind, "unknown");
    });
});

describe("solo matchmaking fixes", () => {
    it("a queue whose launch fails answers FAILED instead of sending players to ':0'", async () => {
        Deploy.Fail = true;

        for(const Player of [A, B, C, D]){
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "CR19_PlayerHunt_SoloFail", Player), true);
        }

        await WaitForDeployCalls(1);
        for(const Player of [A, B, C, D]){
            await WaitForCandidateKind(Player, "failed");
        }
    });

    it("a party candidate takes over a member's solo queue entry", async () => {
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "CR19_PlayerHunt_Solo", B), true);
        await FormParty(A, B);
        await HandlePlayerMatchmaking("ISLAND", "", HUNT, A);

        const Entry = await CheckAndUpdateQueueStatus(B);
        assert.deepEqual([Entry?.HuntId, Entry?.PartyCandidate, Entry?.Ready], [HUNT, true, true]);
    });
});

// The 22 September 2026 two-player test: after a metagame restart V's join (01:31:21) was never
// matched; V joined again at 01:34:33 and O at 01:34:50, both for the same hunt. The deploy server was
// asked for a server expecting V, V, O, the hunt server waited for a third player, and the airship
// countdown froze with both real players ready. A player now has at most one queued join.
describe("one queued join per player", () => {
    const V = D, O = E;

    it("a stale join, the same player's new join and a second player's join: one server expecting the two accounts once each", async () => {
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, V), true);
        const Stale = (await CheckAndUpdateQueueStatus(V))?.CandidateId;
        // The client's DELETE /candidate right after the join keeps its 404, so nothing else takes this join out

        Advance(192);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, V), true);
        Advance(17);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, O), true);
        assert.equal(Deploy.Calls.length, 0, "allocation waits for a queued player to poll");

        Advance(21);
        const SentO = await DecideCandidateStatus(O);
        const SentV = await DecideCandidateStatus(V);
        assert.equal(Deploy.Calls.length, 1, "one hunt server");
        assert.deepEqual(Deploy.Calls[0], { GameMode: "ISLAND", GameArgs: "", HuntId: HUNT, ExpectedPlayers: [V, O] });
        assert.ok(SentV.Kind === "travel" && SentO.Kind === "travel", "both are sent");
        assert.equal(SentV.Entry.Port, SentO.Entry.Port, "to the same server");
        assert.notEqual(SentV.Entry.CandidateId, Stale, "V follows their new join");
    });

    it("a player who joins again does not count twice toward a full queue", async () => {
        for(const Player of [V, V, O, C]){
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, Player), true, Player);
        }

        assert.equal(Deploy.Calls.length, 0, "three players, not four");
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, B), true);
        await WaitForDeployCalls(1);
        assert.deepEqual(Deploy.Calls[0].ExpectedPlayers, [V, O, C, B]);
    });

    it("a join for another hunt, or for Ramsgate, also replaces the older queued join", async () => {
        const OTHER = "CR19_PlayerHunt_Other";

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", OTHER, V), true);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, V), true, "V changes their mind");
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, C), true);
        assert.equal(await HandlePlayerMatchmaking("CITY", "", "ShatteredIsles_ReturnToRamsgate", C), true, "C goes back to Ramsgate instead");
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", OTHER, O), true);
        Deploy.Calls.length = 0;

        Advance(21);
        assert.equal((await DecideCandidateStatus(V)).Kind, "travel");
        assert.equal((await DecideCandidateStatus(O)).Kind, "travel");
        const ToRamsgate = await DecideCandidateStatus(C);
        assert.equal(ToRamsgate.Kind === "travel" && ToRamsgate.Entry.Port, 8777, "C is sent to Ramsgate");
        assert.deepEqual(Deploy.Calls.map((Call) => [Call.HuntId, Call.ExpectedPlayers]), [[HUNT, [V]], [OTHER, [O]]]);
    });

    it("a party's members are expected together and once each, even with older solo joins of their own still queued", async () => {
        // B and a stranger (V) queued for the hunt on their own; then B's party leader A starts it for the party
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, B), true);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, V), true);
        await FormParty(A, B, C);

        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, A), true, "the leader's second join starts nothing");
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", HUNT, B), true, "nor a member's own join of the party's hunt");
        assert.equal(Deploy.Calls.length, 1);
        assert.deepEqual(Deploy.Calls[0], { GameMode: "ISLAND", GameArgs: "", HuntId: HUNT, ExpectedPlayers: [A, B, C] });

        for(const Member of [A, B, C]){
            const Sent = await DecideCandidateStatus(Member);
            assert.equal(Sent.Kind === "travel" && Sent.Entry.Port, 8775, Member);
        }

        // The stranger's queue goes on without B
        Advance(21);
        const Stranger = await DecideCandidateStatus(V);
        assert.equal(Stranger.Kind === "travel" && Stranger.Entry.Port, 8774);
        assert.deepEqual(Deploy.Calls[1], { GameMode: "ISLAND", GameArgs: "", HuntId: HUNT, ExpectedPlayers: [V] });
    });
});

it('a travelling party member survives the heartbeat gap, but the grace is bounded', async () => {
    await FormParty(A,B);
    await HandlePlayerMatchmaking('ISLAND','',HUNT,A);
    await WaitForCandidateKind(A,'travel');
    await WaitForCandidateKind(B,'travel');
    Advance(121);
    await PollParty(B);
    assert.equal(GetPartyOf(B)?.LeaderId,A);
    EvictPartyLeader(B,A);
    assert.equal(GetPartyOf(B)?.LeaderId,A);
    Advance(181);
    await PollParty(B);
    assert.equal(GetPartyOf(B)?.LeaderId,B);
});

it('duplicate tutorial joins share one allocation and the immediate retry keeps that allocation', async () => {
    Deploy.DelayMs=100;
    await Promise.all([HandlePlayerMatchmaking('ISLAND',TUTORIAL,'',A),HandlePlayerMatchmaking('ISLAND',TUTORIAL,'',A)]);
    await HandlePlayerMatchmaking('ISLAND',TUTORIAL,'',A);
    assert.equal(Deploy.Calls.length,1);
    Advance(61);
    await HandlePlayerMatchmaking('ISLAND',TUTORIAL,'',A);
    assert.equal(Deploy.Calls.length,2);
});

it('cross-region party members follow the leader for shared worlds, then recover their own region on leaving', async () => {
    const {SetRegionReader}=await import('../src/controllers/huntregion');
    SetRegionReader(id=>id===B?'aus':'main');
    try {
        await FormParty(B,A);
        await HandlePlayerMatchmaking('CITY','','ShatteredIsles_ReturnToRamsgate',A);
        assert.equal(Deploy.Calls.at(-1).Region,'aus');
        await HandlePlayerMatchmaking('SHARED','','ShatteredIsles_TrainingDojo',A);
        assert.equal(Deploy.Calls.at(-1).Region,'aus');
        LeaveParty(A);
        await HandlePlayerMatchmaking('CITY','','ShatteredIsles_ReturnToRamsgate',A);
        assert.equal(Deploy.Calls.at(-1).Region,undefined);
    } finally {SetRegionReader(()=> 'main');}
});

it('OCE party leader retains region when inviting Main players and city requests carry the selection', async () => {
    const {SetRegionReader}=await import('../src/controllers/huntregion');
    SetRegionReader(id=>id===B?'aus':'main');
    try {
        await HandlePlayerMatchmaking('CITY','','',B);
        assert.equal(Deploy.Calls.at(-1).Region,'aus');
        await FormParty(B,A);
        await HandlePlayerMatchmaking('ISLAND','',HUNT,B);
        await WaitForDeployCalls(2);
        assert.equal(Deploy.Calls[1].Region,'aus');
        assert.deepEqual(new Set(Deploy.Calls[1].ExpectedPlayers),new Set([B,A]));
        await WaitForCandidateKind(B,'travel');
        await WaitForCandidateKind(A,'travel');
    } finally {SetRegionReader(()=>'main');}
});
