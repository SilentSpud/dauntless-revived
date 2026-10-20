// A small model of how the 1.4.4 client parses the social replies (docs/findings/social.md). The rules
// were read from the game's executable (Dauntless-Win64-Shipping.exe, image base 0x140000000; the
// addresses are virtual addresses in that image). Tests check replies against this model, not only
// against literal JSON, so a reply the real client would drop fails a test even when it "looks right".
// Fed the replies the server sent on 22 September 2026, the model reproduces what the players saw.
//
// This is a test helper only; nothing in src/ imports it.

// UE4's FJsonObject keeps its fields in a TMap keyed by FString, whose comparison ignores case, so a
// field lookup ignores case. With two keys differing only in case the later one wins (it replaced the
// earlier one while parsing).
export function Field(Object_: unknown, Name: string): unknown {
    if(!IsPlainObject(Object_)){
        return undefined;
    }

    let Found: unknown = undefined;

    for(const [Key, Value] of Object.entries(Object_)){
        if(Key.toLowerCase() === Name.toLowerCase()){
            Found = Value;
        }
    }

    return Found;
}

export function IsPlainObject(Value: unknown): Value is Record<string, unknown> {
    return typeof Value === "object" && Value !== null && !Array.isArray(Value);
}

function NonEmptyString(Value: unknown): Value is string {
    return typeof Value === "string" && Value.length > 0;
}

function ParseJson(Text: string): { ok: true, value: unknown } | { ok: false } {
    try{
        return { ok: true, value: JSON.parse(Text) };
    }
    catch{
        return { ok: false };
    }
}

const IsSuccessStatus = (Status: number) => Status >= 200 && Status <= 206;

// ---- POST /account/mapping (FOnlineUserPhoenix::QueryExternalIdMappings, 0x140b09f60..0x140b0afd4) ----

export type MappedId = { type: "MCP" | "Phoenix", id: string };
export type MappingParse = { ok: true, mapped: Map<string, MappedId> } | { ok: false, error: string };

// The HTTP status is not looked at. An empty body is "Empty response payload", unparseable JSON is
// "Invalid response payload". Root "accountMappings" must be an object (an array reads as an empty one);
// each value must be an object with non-empty string accountId and accountType; accountType "epic" means
// an Mcp (Epic) id, "phoenix" a Phoenix id, anything else is skipped. The key is the asked id.
export function ParseAccountMappings(_Status: number, Text: string): MappingParse {
    if(Text.length === 0){
        return { ok: false, error: "Empty response payload" };
    }

    const Parsed = ParseJson(Text);

    if(!Parsed.ok || !IsPlainObject(Parsed.value)){
        return { ok: false, error: "Invalid response payload" };
    }

    const Mapped = new Map<string, MappedId>();
    const Mappings = Field(Parsed.value, "accountMappings");

    if(!IsPlainObject(Mappings)){
        return { ok: true, mapped: Mapped };
    }

    for(const [Key, Value] of Object.entries(Mappings)){
        const AccountId = Field(Value, "accountId");
        const AccountType = Field(Value, "accountType");

        if(!NonEmptyString(AccountId) || !NonEmptyString(AccountType)){
            continue;
        }

        const Type = AccountType.toLowerCase() === "epic" ? "MCP" : AccountType.toLowerCase() === "phoenix" ? "Phoenix" : undefined;

        if(Type !== undefined){
            Mapped.set(Key, { type: Type, id: AccountId });
        }
    }

    return { ok: true, mapped: Mapped };
}

// ---- POST /accountinfo/public (FOnlineUserPhoenix::QueryUserInfo, 0x140b5aed0; AddUserInfo 0x140b74870) ----

export type UserInfo = { username: string, epicId: string | undefined };
export type AddUserInfoResult = "stored" | "ignored" | "failed";

// A status outside 200-206 or a body that is not a JSON object fails (nothing cached). The cache key is
// the reply's accountId; an id already cached is left alone (first write wins, 0x140b748f9). The user's
// "epic:id" attribute comes from the first linkedAccounts entry whose accountType is "epic" (0x140b7499f).
export function AddUserInfo(Cache: Map<string, UserInfo>, Status: number, Text: string): { result: AddUserInfoResult, key?: string } {
    if(!IsSuccessStatus(Status)){
        return { result: "failed" };
    }

    const Parsed = ParseJson(Text);

    if(!Parsed.ok || !IsPlainObject(Parsed.value)){
        return { result: "failed" };
    }

    const Key = Field(Parsed.value, "accountId");

    if(!NonEmptyString(Key)){
        return { result: "failed" };
    }

    if(Cache.has(Key)){
        return { result: "ignored", key: Key };
    }

    const Linked = Field(Parsed.value, "linkedAccounts");
    const Epic = Array.isArray(Linked) ? Linked.find((Entry) => typeof Field(Entry, "accountType") === "string" && (Field(Entry, "accountType") as string).toLowerCase() === "epic") : undefined;
    const EpicId = Field(Epic, "accountId");
    const Username = Field(Parsed.value, "username");

    Cache.set(Key, { username: typeof Username === "string" ? Username : "", epicId: typeof EpicId === "string" ? EpicId : undefined });

    return { result: "stored", key: Key };
}

// ---- The Phoenix envelope (OnlinePhoenix::Parse 0x140aae300, envelope reader 0x140b12170) ----

export type Envelope = { ok: boolean, parsed: boolean, code: string | undefined, message: string | undefined, payload: unknown };

// Success needs a 200-206 status AND a JSON object body: a 204 with no body is a failure. The body is
// parsed on errors too, for its code.
export function ParsePhoenixEnvelope(Status: number, Text: string): Envelope {
    const Parsed = Text.length > 0 ? ParseJson(Text) : { ok: false as const };
    const Root = Parsed.ok && IsPlainObject(Parsed.value) ? Parsed.value : undefined;
    const Code = Field(Root, "code");
    const Message = Field(Root, "message");

    return {
        ok: IsSuccessStatus(Status) && Root !== undefined,
        parsed: Root !== undefined,
        code: typeof Code === "string" ? Code : undefined,
        message: typeof Message === "string" ? Message : undefined,
        payload: Field(Root, "payload")
    };
}

// ---- Guild error codes (mapper 0x140ade3f0; EGuildRequestError in the SDK) ----

export const GUILD_ERROR_CODES: Record<string, string> = {
    SlyAdorableQuillshot: "InvalidPermission",
    ExcludedAdorableQuillshot: "NotInAGuild",
    ObedientAdorableQuillshot: "GuildNameInvalidLength",
    SeizedAdorableQuillshot: "GuildNameTaken",
    NastyAdorableQuillshot: "GuildNameProfane",
    NumberedAdorableQuillshot: "GuildNameTooManyNumbers",
    LetteredAdorableQuillshot: "GuildNameTooManyLetters",
    DutifulAdorableQuillshot: "GuildNameplateInvalidLength",
    CapturedAdorableQuillshot: "GuildNameplateTaken",
    DirtyAdorableQuillshot: "GuildNameplateProfane",
    OccupiedAdorableQuillshot: "YouAlreadyInAGuild",
    ClonedAdorableQuillshot: "TargetAlreadyInYourGuild",
    RedundantAdorableQuillshot: "TargetAlreadyHasGuildInvite",
    StuffedAdorableQuillshot: "GuildIsFull",
    UninvitedAdorableQuillshot: "GuildInviteNotFound",
    ChiefAdorableQuillshot: "GuildLeaderCannotLeaveGuild",
    DocileAdorableQuillshot: "InvalidGuildRank"
};

export function GuildErrorOf(Code: string | undefined): string {
    return Code !== undefined && Object.prototype.hasOwnProperty.call(GUILD_ERROR_CODES, Code) ? GUILD_ERROR_CODES[Code] : "Unknown";
}

// A guild request as the client sees it: success, or the EGuildRequestError it shows
export function GuildOutcome(Status: number, Text: string): { ok: boolean, error: string, envelope: Envelope } {
    const Parsed = ParsePhoenixEnvelope(Status, Text);

    return { ok: Parsed.ok, error: Parsed.ok ? "None" : GuildErrorOf(Parsed.code), envelope: Parsed };
}

// ---- The guild payload (FGuildData / FGuildMemberData; serializer 0x140b13de0, member 0x140b13ce0) ----

export type GuildData = { id: string, name: string, nameplate: string, leader: string, members: { id: string, rank: string }[], max: number };

// Every field is a string except maximum_guild_members (int32). A value of the wrong type is skipped,
// so an id that is not a string leaves the guild without an id (the client then treats it as no guild).
export function ParseGuildData(Payload: unknown): GuildData | undefined {
    if(!IsPlainObject(Payload)){
        return undefined;
    }

    const Str = (Name: string) => typeof Field(Payload, Name) === "string" ? Field(Payload, Name) as string : "";
    const Members = Field(Payload, "members");
    const Max = Field(Payload, "maximum_guild_members");

    const Data: GuildData = {
        id: Str("id"),
        name: Str("name"),
        nameplate: Str("nameplate"),
        leader: Str("leader_account_id"),
        members: Array.isArray(Members) ? Members.filter(IsPlainObject).map((Member) => ({
            id: typeof Field(Member, "phx_account_id") === "string" ? Field(Member, "phx_account_id") as string : "",
            rank: typeof Field(Member, "rank") === "string" ? Field(Member, "rank") as string : ""
        })) : [],
        max: typeof Max === "number" && Number.isInteger(Max) ? Max : 0
    };

    return Data.id.length > 0 ? Data : undefined;
}

export type GuildInviteData = { id: string, guildId: string, guildName: string, inviter: string };

export function ParseGuildInvites(Payload: unknown): GuildInviteData[] | undefined {
    const Invites = Field(Payload, "invites");

    if(!Array.isArray(Invites)){
        return undefined;
    }

    return Invites.filter(IsPlainObject).map((Invite) => ({
        id: String(Field(Invite, "id") ?? ""),
        guildId: String(Field(Invite, "guild_id") ?? ""),
        guildName: String(Field(Invite, "guild_name") ?? ""),
        inviter: String(Field(Invite, "inviter_account_id") ?? "")
    }));
}

// ---- Friends service (OnlineFriendsMcp, 0x1408dd229..) ----

export type FriendEntry = { accountId: string, status: string, direction: string };

// The client wraps the body as {"friends": <body>}: it must be a JSON array, each entry with a string
// accountId (status and direction are strings)
export function ParseFriendsList(Text: string): FriendEntry[] | undefined {
    const Parsed = ParseJson(Text);

    if(!Parsed.ok || !Array.isArray(Parsed.value)){
        return undefined;
    }

    const Entries: FriendEntry[] = [];

    for(const Entry of Parsed.value){
        const AccountId = Field(Entry, "accountId");

        if(typeof AccountId !== "string"){
            continue;
        }

        Entries.push({ accountId: AccountId, status: String(Field(Entry, "status") ?? ""), direction: String(Field(Entry, "direction") ?? "") });
    }

    return Entries;
}

export function ParseBlockList(Text: string): string[] | undefined {
    const Parsed = ParseJson(Text);
    const List = Parsed.ok ? Field(Parsed.value, "blockedUsers") : undefined;

    return Array.isArray(List) ? List.filter((Id): Id is string => typeof Id === "string") : undefined;
}

// ---- GET /party/invites (0x140b675d0) ----

export type PartyInvitation = { recipientPlayerId: string, sendingPlayerId: string, partyId: string, sendingPlatform: string, sendingDisplayName: string };

const INVITATION_FIELDS = ["recipientPlayerId", "sendingPlayerId", "partyId", "sendingPlatform", "sendingDisplayName"] as const;

// "invitations": an array of objects with five strings each; an entry missing one is skipped
export function ParseInvitations(Text: string): PartyInvitation[] | undefined {
    const Parsed = ParseJson(Text);
    const List = Parsed.ok ? Field(Parsed.value, "invitations") : undefined;

    if(!Array.isArray(List)){
        return undefined;
    }

    return List.filter((Entry) => INVITATION_FIELDS.every((Name) => typeof Field(Entry, Name) === "string"))
        .map((Entry) => Object.fromEntries(INVITATION_FIELDS.map((Name) => [Name, Field(Entry, Name) as string])) as PartyInvitation);
}

// ---- One player's client ----

export type Transport = (Method: string, Path: string, Body?: unknown) => Promise<{ status: number, text: string }>;

// Keeps what the real client keeps for a session: user info per Phoenix id, mappings per source type
export class SocialClient {
    readonly Users = new Map<string, UserInfo>();
    readonly Mappings = new Map<string, Map<string, MappedId>>();
    readonly Calls: string[] = [];

    constructor(readonly Self: string, private readonly Send: Transport){}

    private async Request(Method: string, Path: string, Body?: unknown){
        this.Calls.push(`${Method} ${Path}`);
        return await this.Send(Method, Path, Body);
    }

    // At login the client looks up its own account first (2.1.1 capture, request 6462)
    async Login(){
        await this.QueryUserInfo(this.Self);
        return this;
    }

    async QueryUserInfo(Id: string){
        const Reply = await this.Request("POST", "/accountinfo/public", { accountId: Id });
        return AddUserInfo(this.Users, Reply.status, Reply.text);
    }

    async QueryMappings(Ids: string[], SourceType = "epic"){
        const Cache = this.Mappings.get(SourceType) ?? new Map<string, MappedId>();
        this.Mappings.set(SourceType, Cache);

        const Missing = Ids.filter((Id) => !Cache.has(Id));

        if(Missing.length > 0){
            const Reply = await this.Request("POST", "/account/mapping", { srcAccountType: SourceType, ids: Missing });
            const Parsed = ParseAccountMappings(Reply.status, Reply.text);

            if(Parsed.ok){
                for(const [Key, Value] of Parsed.mapped){
                    Cache.set(Key, Value);
                }
            }
        }

        return Cache;
    }

    // Epic id -> Phoenix id -> user info -> the toolkit's check (0x1409e3c60, 0x1409add00) that the user's
    // Epic id is the one the action started from. "dropped" is what the player sees as nothing happening.
    async ResolveFromEpic(EpicId: string): Promise<{ outcome: "resolved" | "dropped", phoenixId?: string, why?: string }> {
        const Mapped = (await this.QueryMappings([EpicId])).get(EpicId);

        if(Mapped === undefined || Mapped.type !== "Phoenix"){
            return { outcome: "dropped", why: "Mapping primary Id for unknown, unmapped external Id" };
        }

        if(!this.Users.has(Mapped.id)){
            await this.QueryUserInfo(Mapped.id);
        }

        const Info = this.Users.get(Mapped.id);

        if(Info === undefined){
            return { outcome: "dropped", phoenixId: Mapped.id, why: "no user info" };
        }

        if(Info.epicId !== EpicId){
            return { outcome: "dropped", phoenixId: Mapped.id, why: `the user's Epic id is ${Info.epicId}, not ${EpicId}` };
        }

        return { outcome: "resolved", phoenixId: Mapped.id };
    }

    // A Phoenix id (party invite sender, party member, guild member): USocialUser::TryBroadcastInitializationComplete
    // (0x1409ede50) invalidates a user without user info
    async ResolveFromPhoenix(Id: string): Promise<"initialised" | "invalidated"> {
        if(!this.Users.has(Id)){
            await this.QueryUserInfo(Id);
        }

        return this.Users.has(Id) ? "initialised" : "invalidated";
    }

    NameOf(Id: string){
        return this.Users.get(Id)?.username;
    }
}
