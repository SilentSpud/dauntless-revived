import { Element, parse } from "ltx";

// A small model of how the 1.4.4 client reads chat (docs/findings/chat.md). The rules were read from the
// game's executable (Dauntless-Win64-Shipping.exe, image base 0x140000000; the addresses are virtual
// addresses in that image). Tests feed it the frames a server sent, so a reply the real client would
// read differently fails a test even when it "looks right". Fed the frames of pull request #9's first
// server, it reproduces what its author saw with a real client on 22 September 2026: his own lines under
// "UID-...", joins stuck with "Another operation already pending", and "[unknown]".
//
// This is a test helper only; nothing in src/ imports it, and it shares no code with the server.

// ---- The client's URL encoding (FGenericPlatformHttp) ----

// UrlEncode (0x1428aebb0): the text as UTF-8 (0x1407b6f40); A-Z a-z 0-9 - _ . ~ kept (the set at
// 0x145993a80, tested at 0x1428a5440); every other byte %XX in upper case (0x1428aed4b, 0x1428aed5e)
export function UrlEncode(Text: string): string {
    let Out = "";

    for(const Byte of Buffer.from(Text, "utf8")){
        const Char = String.fromCharCode(Byte);

        Out += /[A-Za-z0-9\-_.~]/.test(Char) ? Char : `%${Byte.toString(16).toUpperCase().padStart(2, "0")}`;
    }

    return Out;
}

// UrlDecode (0x1428ae680): %XX back to bytes, then UTF-8. (Malformed escapes are not modelled: the
// client's own encoder never writes one.)
export function UrlDecode(Text: string): string {
    const Bytes: number[] = [];

    for(let Index = 0; Index < Text.length;){
        if(Text[Index] === "%" && /^[0-9A-Fa-f]{2}$/.test(Text.slice(Index + 1, Index + 3))){
            Bytes.push(parseInt(Text.slice(Index + 1, Index + 3), 16));
            Index += 3;
        }
        else{
            for(const Byte of Buffer.from(Text[Index], "utf8")) Bytes.push(Byte);
            Index += 1;
        }
    }

    return Buffer.from(Bytes).toString("utf8");
}

// ---- JIDs ----

type Jid = { Local: string, Domain: string, Resource: string };

function ParseJid(Text: string): Jid {
    const Slash = Text.indexOf("/");
    const Bare = Slash === -1 ? Text : Text.slice(0, Slash);
    const At = Bare.indexOf("@");

    return {
        Local: At === -1 ? "" : Bare.slice(0, At),
        Domain: At === -1 ? Bare : Bare.slice(At + 1),
        Resource: Slash === -1 ? "" : Text.slice(Slash + 1)
    };
}

function Escape(Text: string): string {
    return Text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
}

function Child(Node: Element | undefined, Name: string): Element | undefined {
    return Node?.getChildElements().find((Item) => Item.getName() === Name);
}

// ---- Rooms ----

// The Xmpp MUC room states (to-string switch 0x143a32c00)
export const NOT_JOINED = 0;
export const JOINED = 1;
export const JOIN_PUBLIC_PENDING = 4;
export const EXIT_PENDING = 5;

// A room member (FOnlineChatMcp's room member, parsed at 0x1408fe300 / 0x1408c3500)
export type Member = {
    From: string,   // the full room JID it was seen from
    Nick: string,   // the resource: the nickname
    Id: string,     // member id: <item jid>'s node, else nickname part 2, else the room JID's full path
    Name: string    // nickname part 1 URL-decoded, or the whole nickname when it has no ":"
};

// Members is FOnlineChatMcp's member map of the room, keyed by member id (lower case: the key compare is
// case-insensitive, 0x141c5bff0). One entry per account, however many connections it has in the room:
// - an occupant presence looks its id up and updates that entry, or adds one (UpdateMember 0x1408fe300:
//   find 0x1408fe3d4, update 0x1408fe409 -> 0x1408fbd70, add 0x1408fe489);
// - a leave removes the entry of the id taken from the leaving nickname alone, never from <item jid>
//   (OnXmppRoomMemberExit 0x1408c0680: 0x1408c07a9 -> 0x1408c3500, then TMap::Remove 0x1408c0b54 ->
//   0x1408e1ce0);
// - a line's sender is found by walking the entries for the one whose room JID matches (0x1408a83c0).
export type Room = { RoomId: string, State: number, Nick: string, Members: Map<string, Member> };

// The member id the client takes from a leaving occupant: nickname part 2, else the room JID's full path
function ExitIdOf(From: string, Nick: string): string {
    return Nick.includes(":") ? (Nick.split(":")[1] ?? "") : From;
}

// A line shown in a room. Known is false when the sender matched no occupant: the nickname stays
// "unknown" (0x14087ed65).
export type RoomLine = { Room: string, Known: boolean, Member?: Member, DerivedId: string, Text: string };
export type Whisper = { FromId: string, Text: string };

// Looks another player up by id, the way GET /account/api/public/account?accountId=<id> is read
// (0x1408fe9c0 -> 0x140987d90): their displayName, or undefined when nobody is found
export type Lookup = (Id: string) => Promise<string | undefined> | string | undefined;

function ParseMember(From: string, Nick: string, Item: Element | undefined): Member {
    const Colon = Nick.indexOf(":");
    const ItemJid = Item?.attrs.jid;
    let Id: string;

    if(typeof ItemJid === "string" && ItemJid.length > 1){
        Id = ParseJid(ItemJid).Local;
    }
    else if(Colon !== -1){
        Id = Nick.split(":")[1] ?? "";
    }
    else{
        // No ":" at all: the id becomes the room JID's full path (0x1408c3686, 0x1408aea40)
        Id = From;
    }

    return { From, Nick, Id, Name: Colon !== -1 ? UrlDecode(Nick.slice(0, Colon)) : Nick };
}

export class ChatClientModel {
    readonly LocalUid: string;
    readonly Domain: string;
    readonly Resource: string;
    readonly LocalSocialName: string;
    LoggedIn = true;
    readonly Rooms = new Map<string, Room>();
    readonly Log: string[] = [];
    readonly Lines: RoomLine[] = [];
    readonly Whispers: Whisper[] = [];
    readonly JoinResults: Array<{ Room: string, Ok: boolean }> = [];
    readonly ExitResults: string[] = [];

    constructor(Options: { LocalUid: string, Resource: string, LocalSocialName: string, Domain?: string }) {
        this.LocalUid = Options.LocalUid;
        this.Resource = Options.Resource;
        this.LocalSocialName = Options.LocalSocialName;
        this.Domain = Options.Domain ?? "prod.ol.epicgames.com";
    }

    // "muc." + the connection's domain (0x143a33392)
    get MucDomain(): string {
        return `muc.${this.Domain}`;
    }

    // Printf("%s:%s:%s", UrlEncode(Nickname), UserId, conn->GetUserJid().Resource) (0x1408b5aeb). The
    // display name is GetPlayerNickname (0x14092bad0), which is "InvalidMCPUser" when the login-time
    // account read failed (0x14092bc0b), never empty.
    Nickname(DisplayName: string): string {
        return `${UrlEncode(DisplayName.length > 0 ? DisplayName : "InvalidMCPUser")}:${this.LocalUid}:${this.Resource}`;
    }

    // The room lookup ignores case (seen with a real client: joins completed on lowercased rooms)
    RoomOf(RoomId: string): Room | undefined {
        return this.Rooms.get(RoomId.toLowerCase());
    }

    // JoinPublicRoom (0x143a3267b): needs a logged-in connection (0x143a327fe) and a room that is not
    // joining or leaving (0x143a32938-0x143a32990), then state 4 (0x143a329a9). Returns the stanza sent.
    JoinPublicRoom(RoomId: string, DisplayName: string): string | undefined {
        if(!this.LoggedIn){
            this.Log.push(`MUC: JoinPublicRoom failed. Not currently connected`);
            return undefined;
        }

        const Existing = this.RoomOf(RoomId);

        if(Existing !== undefined && Existing.State !== NOT_JOINED && Existing.State !== JOINED){
            this.Log.push(`MUC: JoinPublicRoom failed. Another operation already pending for room ${RoomId}`);
            return undefined;
        }

        const Nick = this.Nickname(DisplayName);

        this.Rooms.set(RoomId.toLowerCase(), { RoomId, State: JOIN_PUBLIC_PENDING, Nick, Members: Existing?.Members ?? new Map() });

        // FXmppUserJid(RoomId, MucDomain, Nickname) (0x143a328b3); the stanza builder 0x143a3b140
        return `<presence to="${Escape(`${RoomId}@${this.MucDomain}/${Nick}`)}"><x xmlns="http://jabber.org/protocol/muc"><history maxstanzas="50"/></x></presence>`;
    }

    // ExitRoom: state 5 until the room's own unavailable presence arrives
    ExitRoom(RoomId: string): string | undefined {
        const Room = this.RoomOf(RoomId);

        if(Room === undefined || Room.State !== JOINED){
            return undefined;
        }

        Room.State = EXIT_PENDING;
        return `<presence type="unavailable" to="${Escape(`${Room.RoomId}@${this.MucDomain}/${Room.Nick}`)}"/>`;
    }

    // A groupchat as SendChat builds it (0x143a3a6b0): to the bare room JID (0x143a3a950)
    RoomMessage(RoomId: string, Text: string, Id: string): string {
        return `<message id="${Escape(Id)}" type="groupchat" to="${Escape(`${RoomId}@${this.MucDomain}`)}"><body>${Escape(Text)}</body></message>`;
    }

    // A whisper (builder 0x143a3a9d0): to the bare JID of the account
    WhisperTo(AccountId: string, Text: string): string {
        return `<message type="chat" to="${Escape(`${AccountId}@${this.Domain}`)}"><body>${Escape(Text)}</body></message>`;
    }

    Receive(Frame: string): void {
        let Node: Element;

        try{
            Node = parse(Frame);
        }
        catch{
            this.Log.push("XMPP: parse error");
            return;
        }

        if(Node.getName() === "presence"){
            this.presence(Node);
        }
        else if(Node.getName() === "message"){
            this.message(Node);
        }
    }

    ReceiveAll(Frames: string[]): void {
        for(const Frame of Frames){
            this.Receive(Frame);
        }
    }

    private presence(Node: Element): void {
        const FromText = typeof Node.attrs.from === "string" ? Node.attrs.from as string : "";
        const From = ParseJid(FromText);

        // The MUC module claims a presence only from exactly muc.<Domain>, case included (0x143a379d2-0x143a37a26);
        // anything else goes to the presence module, which this model does not cover
        if(From.Domain !== this.MucDomain){
            return;
        }

        const Room = this.RoomOf(From.Local);

        // The presence error handler (0x143a34d80): for a room in JoinPublicPending the room is removed
        // and the join-complete delegate fires with failure (state-4 branch 0x143a34dfc-0x143a34e53);
        // in any other state nothing happens
        if(Node.attrs.type === "error"){
            if(Room !== undefined && Room.State === JOIN_PUBLIC_PENDING){
                this.Rooms.delete(From.Local.toLowerCase());
                this.JoinResults.push({ Room: Room.RoomId, Ok: false });
                this.Log.push(`MUC: join failed for room ${Room.RoomId}`);
            }

            return;
        }

        if(Room === undefined){
            this.Log.push("MUC: Ignored presence from room we haven't joined");
            return;
        }

        const Nick = From.Resource;
        const Unavailable = Node.attrs.type === "unavailable";
        // The self test: a case-sensitive search for the local account id in the nickname
        // (0x143a34acd-0x143a34aec). Status 110 is not read.
        const Self = Nick.includes(this.LocalUid);
        const Item = Child(Child(Node, "x"), "item");

        if(Self){
            if(Unavailable){
                // Server initiated room exit / exit complete (0x143a34bab-0x143a34bb9 -> 0x143a28180)
                if(Room.State === EXIT_PENDING) this.ExitResults.push(Room.RoomId);
                Room.State = NOT_JOINED;
                Room.Members.clear();
            }
            else if(Room.State === JOIN_PUBLIC_PENDING){
                // HandleJoinPublicRoomComplete (0x143a34c0f -> 0x143a296b0); the room lists itself as a member
                const Member = ParseMember(FromText, Nick, Item);

                Room.State = JOINED;
                Room.Members.set(Member.Id.toLowerCase(), Member);
                this.JoinResults.push({ Room: Room.RoomId, Ok: true });
            }
            // A self presence in any other state is ignored (0x143a34bf8-0x143a34c00)

            return;
        }

        // Someone else: no state check (0x143a34c19-0x143a34c2c -> 0x143a2bcc0)
        if(Unavailable){
            Room.Members.delete(ExitIdOf(FromText, Nick).toLowerCase());
        }
        else{
            const Member = ParseMember(FromText, Nick, Item);

            Room.Members.set(Member.Id.toLowerCase(), Member);
        }
    }

    // The member a room line came from: the entry whose room JID matches, ignoring case (0x1408a83c0)
    MemberFrom(Room: Room, FromText: string): Member | undefined {
        const Wanted = FromText.toLowerCase();

        return [...Room.Members.values()].find((Each) => Each.From.toLowerCase() === Wanted);
    }

    private message(Node: Element): void {
        const FromText = typeof Node.attrs.from === "string" ? Node.attrs.from as string : "";
        const From = ParseJid(FromText);
        const Type = Node.attrs.type;
        const Body = Node.getChildText("body") ?? "";

        if(Type === "error"){
            // Routed to the MUC module, which only logs it (xref 0x143a288a7)
            this.Log.push("MUC: Received GroupChat error");
            return;
        }

        if(Type === "groupchat"){
            if(From.Domain !== this.MucDomain){
                return;
            }

            const Room = this.RoomOf(From.Local);

            if(Room === undefined || Body.length === 0){
                return;
            }

            // The sender is matched to an occupant by room id, domain and nickname, ignoring case
            // (0x1408a83c0); a <nick> element is never read (its namespace is not in the exe)
            const Member = this.MemberFrom(Room, FromText);
            let DerivedId = Member?.Id ?? "";

            if(Member === undefined){
                // No occupant: the id comes from the nickname (0x1408b1030 -> 0x1408b132e)
                DerivedId = From.Resource.includes(":") ? (From.Resource.split(":")[1] ?? "") : From.Resource.length > 0 ? FromText : "";

                if(DerivedId.length === 0){
                    this.Log.push(`could not get user id for room=${From.Local} member=${From.Resource}`);
                    return;
                }
            }

            this.Lines.push({ Room: Room.RoomId, Known: Member !== undefined, Member, DerivedId, Text: Body });
            return;
        }

        if(Type === "chat"){
            // The shared receive path drops senders without a resource
            if(From.Resource.length === 0 || From.Local.length === 0){
                this.Log.push("XMPP: dropped a message from a bare JID");
                return;
            }

            this.Whispers.push({ FromId: From.Local, Text: Body });
        }
    }

    // The name the chat shows for a room line:
    // - no occupant: "[unknown]" (the nickname stays "unknown", 0x14087ed65; the brackets are the widget's)
    // - its member id is the local account (0x1408febd5): the local social user's name (UChatClient,
    //   vtable 0x14485e640 slot +0x270 = 0x1415a8360)
    // - else the account lookup's displayName; when it finds nobody, nickname part 1 (the trust flag is
    //   true in the shipped config, 0x14097fc20; an empty one also falls back to part 1, 0x1408b67be)
    async ShownName(Line: RoomLine, Lookup: Lookup): Promise<string> {
        if(!Line.Known || Line.Member === undefined){
            return "[unknown]";
        }

        if(Line.Member.Id === this.LocalUid){
            return this.LocalSocialName;
        }

        const Found = await Lookup(Line.Member.Id);

        return Found !== undefined && Found.length > 0 ? Found : Line.Member.Name;
    }

    async ShownLines(Lookup: Lookup): Promise<string[]> {
        const Shown: string[] = [];

        for(const Line of this.Lines){
            Shown.push(`${await this.ShownName(Line, Lookup)}: ${Line.Text}`);
        }

        return Shown;
    }

    // A whisper's sender comes from the same lookup; not found: the whisper waits (0x14157e290)
    async ShownWhispers(Lookup: Lookup): Promise<string[]> {
        const Shown: string[] = [];

        for(const Item of this.Whispers){
            const Found = await Lookup(Item.FromId);

            Shown.push(Found !== undefined && Found.length > 0 ? `${Found}: ${Item.Text}` : `(waiting): ${Item.Text}`);
        }

        return Shown;
    }
}

// Lookups as the ?accountId= route answers them, from a fixed list
export function LookupFrom(Names: Record<string, string>): Lookup {
    return (Id) => Names[Id];
}
