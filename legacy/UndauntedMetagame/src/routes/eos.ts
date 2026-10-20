import { Router } from "express";
import { logger } from "../logger";
import { GetUserIDForAPIKey, SignMetagameJWTForUid } from "../controllers/auth";
import {CheckPlayerAccess} from '../controllers/moderation';
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { DisplayNameForUserId, FindAccountByUsername, FindUsernameForUserId, FindUsernames } from "../controllers/login";
import { SoftMetagameAuth, SoftPlayerOf } from "../middleware/PlayerAuth";
import { VerifyStubAccount } from "../features";
import { LoginRateLimit, TokenVerifyRateLimit } from "../middleware/RateLimits";

export const eosRouter = Router();

eosRouter.post("/account/api/oauth/token", LoginRateLimit, async (req, res) => {
    if(process.env.AUTH_MODE === "NONE" && process.env.NODE_ENV !== "production"){
        const UserId = req.body.exchange_code;

        logger.info(`Logging in ${UserId}!`);

        const AuthToken = SignMetagameJWTForUid(UserId);

        res.json({
            "access_token": AuthToken,
            "token_type": "bearer",
            "expires_at": "2085-09-09T01:01:01.703Z", // TODO: We sign 24hr JWTs so we're unlikely to hit this, but just in case (tm)
            "features": ["Achievements", "AntiCheat", "Ecom", "Voice"],
            "organization_id": "o-krlzxj88qrtb69fredeuaf887bl5az",
            "product_id": "prod-jackal",
            "sandbox_id": "jackal",
            "deployment_id": "53565ba467df4edbb6f5a3d939a8b4f2",
            "expires_in": 86400,
            "refresh_token": "refresh.token.lol", // TODO: IDK if we need to support this considering our intended flow, but flagged regardless
            "refresh_expires_at": "2085-09-09T01:01:01.703Z",
            "account_id": UserId
        });
    }
    else if(process.env.AUTH_MODE === "APIKEY"){
        const ApiKey = req.body.exchange_code;

        const UserId = await GetUserIDForAPIKey(ApiKey);

        if(UserId != undefined){
            if(!CheckPlayerAccess(req,res,UserId))return;
            logger.info(`Logging in ${UserId}!`);

            const AuthToken = SignMetagameJWTForUid(UserId);

            res.json({
                "access_token": AuthToken,
                "token_type": "bearer",
                "expires_at": "2085-09-09T01:01:01.703Z", // TODO: We sign 24hr JWTs so we're unlikely to hit this, but just in case (tm)
                "features": ["Achievements", "AntiCheat", "Ecom", "Voice"],
                "organization_id": "o-krlzxj88qrtb69fredeuaf887bl5az",
                "product_id": "prod-jackal",
                "sandbox_id": "jackal",
                "deployment_id": "53565ba467df4edbb6f5a3d939a8b4f2",
                "expires_in": 86400,
                "refresh_token": "refresh.token.lol", // TODO: IDK if we need to support this considering our intended flow, but flagged regardless
                "refresh_expires_at": "2085-09-09T01:01:01.703Z",
                "account_id": UserId
            });
        }
        else{
            logger.error(`Invalid API key auth!`);

            res.status(400);
            res.send();
        }
    }
    else{
        logger.fatal("No login method configured!");
    }
});

// The account upstream's stub named for everyone
const VERIFY_STUB_ACCOUNT_ID = "9626f441055349ce8cb7d7d5a483eaa2";

// The client's token check, every 30 s (VerifyTokenInterval) for as long as it runs. The client looks its
// logged-in account up by the account_id of the reply ("Verify auth response ignored, can't find
// UserAccount for %s" otherwise), so a valid token now gets its own account, as the real service answered
// (Harmonic's fork did the same). It never answers 401: after the token's 24 hours a refusal here could log
// the player out mid-session, so a missing, bad or expired token still gets the old stub, and expires_at
// stays far off. VERIFY_STUB_ACCOUNT=1 answers the stub to everyone again.
eosRouter.get("/account/api/oauth/verify", TokenVerifyRateLimit, SoftMetagameAuth, (req: any, res) => {
    logger.info("Verifying token");

    const Caller = VerifyStubAccount() ? undefined : SoftPlayerOf(req);

    res.json({
      "active": true,
      "scope": "basic_profile friends_list presence",
      "token_type": "bearer",
      "expires_in": 86400,
      "expires_at": "2085-09-09T01:01:01.703Z",
      "account_id": Caller ?? VERIFY_STUB_ACCOUNT_ID,
      "client_id": "xyza7891lhxMVYGCON7LgnKZZ8HQGD5H",
      "application_id": "fghi4567O03HROxEjwbn7kgXpBhnhWwv"
    });
});

// Epic-style account lookups the client makes for other players (party members, Hunt
// Members, the chat's /invite <name>). With a valid player token they answer
// {id, displayName, externalAuths}; without one they answer what they always did.
function PublicAccount(UserId: string, Username: string){
    return {
        id: UserId,
        displayName: Username,
        externalAuths: {}
    };
}

eosRouter.get("/account/api/public/account/:AccId", SoftMetagameAuth, (req: any, res) => {
    const Caller = SoftPlayerOf(req);
    const Username = Caller !== undefined ? FindUsernameForUserId(req.params.AccId) : undefined;

    if(Caller !== undefined){
        logger.info(`EOS Account Info for ${String(req.params.AccId).slice(0, 64)} by ${Caller}: ${Username !== undefined ? "found" : "unknown"}`);
    }
    else{
        logger.info("EOS Account Info (stubbed)");
    }

    res.json(Username !== undefined ? PublicAccount(req.params.AccId, Username) : {});
});

// Before /:AccId/externalAuths, which a user named "externalAuths" would otherwise hit
eosRouter.get("/account/api/public/account/displayName/:displayName", SoftMetagameAuth, (req: any, res) => {
    const Caller = SoftPlayerOf(req);
    const Account = Caller !== undefined ? FindAccountByUsername(req.params.displayName) : undefined;

    logger.info(`EOS Account by name by ${Caller ?? "<no token>"}: ${Account !== undefined ? Account.UserId : "not found"}`);

    if(Account === undefined){
        res.status(404);
        res.send();
        return;
    }

    res.json(PublicAccount(Account.UserId, Account.Username));
});

eosRouter.get("/account/api/public/account/:AccId/externalAuths", (req, res) => {
    logger.info("External Auths (stubbed)");

    res.json({});
});

// POST /account/mapping is the 1.4.4 client's QueryAccountMappingsEndpoint (FOnlineUserPhoenix::
// QueryExternalIdMappings): it turns Epic account ids into Phoenix account ids. The client calls it for
// Add Friends (after GET /account/api/public/account/displayName/<name>), for the chat's /invite <name>,
// for the guild add-member box, for every id on its friends list and blocklist, and at login for the
// local player ("Calling GetExternalIdMappings for Epic -> Phoenix"; the 2.1.1 capture has it too, but the
// 1.4.4 census of 22 September 2026 shows 6 calls for 8 logins and 2 Add Friends, so not every login). The body is
// {"srcAccountType": "epic", "ids": ["<id>", ...]} (at most 100 ids; seen live on 22 September 2026 and
// in the 2.1.1 capture). Upstream answered 404, and every one of those actions was dropped silently.
//
// The reply the parser reads (read from the exe, docs/findings/social.md): a root key "accountMappings"
// holding an OBJECT keyed by each asked id, each value {"accountId": "<id>", "accountType": "phoenix"}
// with non-empty strings; accountType is compared without case with "epic" and "phoenix", and nothing
// else in an entry is read. HTTP status is not checked; an empty or invalid body fails the query.
// Neither earlier answer worked: the first (an object keyed by id at the root, no "accountMappings")
// gave the parser nothing, and the second ("accountMappings" as an array) reads as an empty object, so
// no mapping was cached and the queued friend request was dropped (0 friend requests reached us live).
// Extra root keys are ignored, so the same map is also sent wrapped under "payload" with "code" and
// "message", in case a later build reads the Phoenix envelope.
//
// On this server a player's Epic id and Phoenix id are the same UID-..., so every mapping is the
// identity. srcAccountType "phoenix" maps the other way (accountType "epic"). Only ids that are our
// accounts are answered, and only to a caller with a valid player token (the client always sends one);
// without one the map is empty. ACCOUNT_MAPPING=0 maps nothing (the old effective behaviour). The body is
// still logged by shape only (keys and id counts, never values that could be tokens).
const MAX_MAPPING_IDS = 100;

function MappingIds(Body: any): string[] {
    const List = (Value: unknown) => Array.isArray(Value) ? Value : undefined;
    const Raw: unknown[] = Array.isArray(Body) ? Body : List(Body?.ids) ?? List(Body?.externalIds) ?? List(Body?.accountIds) ?? List(Body?.externalAuthIds) ?? [];

    return [...new Set(Raw.filter((Id): Id is string => typeof Id === "string" && Id.length > 0 && Id.length <= 128))].slice(0, MAX_MAPPING_IDS);
}

// The account type the ids are in: srcAccountType as the client sends it, or the older guesses
function MappingSourceType(Body: any): string {
    const Field = [Body?.srcAccountType, Body?.type, Body?.externalAuthType].find((Value) => typeof Value === "string");

    return typeof Field === "string" && /^[A-Za-z0-9_.-]{1,32}$/.test(Field) ? Field : "epic";
}

// Where an id of SourceType maps to: Phoenix ids for anything but "phoenix", Epic ids for "phoenix"
function MappedAccountType(SourceType: string){
    return SourceType.toLowerCase() === "phoenix" ? "epic" : "phoenix";
}

function DescribeMappingBody(Body: any): string {
    if(Body === undefined || Body === null){
        return "no body";
    }

    if(Array.isArray(Body)){
        return `array of ${Body.length} (${[...new Set(Body.map((Value: unknown) => typeof Value))].join(",")})`;
    }

    if(typeof Body !== "object"){
        return typeof Body;
    }

    const Keys = Object.keys(Body);

    if(Keys.length === 0){
        return "empty object";
    }

    return Keys.slice(0, 12).map((Key) => {
        const Value = Body[Key];
        const Shape = Array.isArray(Value) ? `[${Value.length}]` : typeof Value === "string" ? (Value.length <= 24 && /^[A-Za-z0-9_.-]*$/.test(Value) ? JSON.stringify(Value) : `string(${Value.length})`) : typeof Value;
        return `${Key.slice(0, 40)}=${Shape}`;
    }).join(" ");
}

eosRouter.post("/account/mapping", SoftMetagameAuth, (req: any, res) => {
    const Caller = SoftPlayerOf(req);
    const Ids = MappingIds(req.body);
    const SourceType = MappingSourceType(req.body);
    const Enabled = process.env.ACCOUNT_MAPPING !== "0";
    const Known = Caller !== undefined && Enabled ? FindUsernames(Ids) : new Map<string, string>();
    const AccountType = MappedAccountType(SourceType);

    // Keyed by the asked id exactly as sent. Object.fromEntries defines own keys, whatever the id.
    const Mapped = Ids.filter((Id) => Known.has(Id));
    const MappingsOf = () => Object.fromEntries(Mapped.map((Id) => [Id, { accountId: Id, accountType: AccountType }]));

    logger.info(`account/mapping by ${Caller ?? "<no token>"}: ${DescribeMappingBody(req.body)}; content-type ${String(req.headers["content-type"] ?? "none").slice(0, 60)} -> ${Mapped.length} of ${Ids.length} mapped${Enabled ? "" : " (ACCOUNT_MAPPING=0)"}`);

    res.status(200);
    res.json({
        accountMappings: MappingsOf(),
        code: "OK",
        message: "",
        payload: { accountMappings: MappingsOf() }
    });
});

eosRouter.delete("/account/api/oauth/sessions/kill", (req, res) => {
    logger.info("Session kill (stubbed)");

    // TODO: Is this needed?

    res.json({});
})

eosRouter.delete("/account/api/oauth/sessions/kill/:AuthToken", (req, res) => {
    logger.info("Session kill (stubbed)");

    // TODO: Is this needed?

    res.json({});
})

// Several accounts at once (?accountId=A&accountId=B): an array of the ones that exist
const MAX_ACCOUNT_LOOKUP = 100;

eosRouter.get("/account/api/public/account", HasUndauntedMetagameAuth, async (req: any, res) => {
    const UserId = req.AuthData.userId;

    if(req.query?.accountId !== undefined){
        const Asked: string[] = (Array.isArray(req.query.accountId) ? req.query.accountId : [req.query.accountId])
            .filter((Id: unknown): Id is string => typeof Id === "string")
            .slice(0, MAX_ACCOUNT_LOOKUP);
        const Names = FindUsernames(Asked);
        const Found = [...new Set(Asked)].filter((Id) => Names.has(Id)).map((Id) => PublicAccount(Id, Names.get(Id)!));

        logger.info(`Account info for ${Asked.length} account(s) by userId ${UserId}: ${Found.length} found`);

        res.json(Found);
        return;
    }

    const Username = await DisplayNameForUserId(UserId);

    logger.info(`Account info for userId ${UserId}`);

    res.json({
        "id": UserId,
        "displayName": Username,
        "name": "",
        "lastName": "",
        "email": "",
        "failedLoginAttempts": 0,
        "lastLogin": new Date().toISOString(),
        "numberOfDisplayNameChanges": 0,
        "ageGroup": "ADULT",
        "headless": false,
        "country": "US",
        "lastNameChange": new Date().toISOString(),
        "preferredLanguage": "en",
        "canUpdateDisplayName": false,
        "tfaEnabled": false,
        "emailVerified": true,
        "minorVerified": false,
        "minorStatus": "NOT_MINOR"
    });
});
