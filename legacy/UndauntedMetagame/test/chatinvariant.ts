import { after } from "node:test";
import assert from "node:assert/strict";
import { ObserveChatOutputForTests } from "../src/realtime/chat";

// The party-safety rule of chat, checked over every stanza every chat test makes the server send: nothing
// outside a room ever reaches a player from the player's own account, not even from a second session of it
// (docs/findings/chat.md, party safety: the client's own presence arriving from the server could wake the
// party's automatic kick, B 0x1415f7562). Import it in each test file that starts a chat server; the file
// fails at the end if one such stanza was sent. This is a test helper only.

export const SelfStanzas: string[] = [];
let Seen = 0;

function Unescape(Text: string){
    return Text.replace(/&quot;/g, "\"").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

// The stanza's own "from" (on its first tag only), when it is an account's JID outside a room
function SenderAccount(Stanza: string): string | undefined {
    const Head = /^<[^>]*>/.exec(Stanza)?.[0] ?? "";
    const From = /\sfrom=(?:"([^"]*)"|'([^']*)')/.exec(Head);

    if(From === null){
        return undefined;
    }

    const Jid = Unescape(From[1] ?? From[2] ?? "");
    const Bare = Jid.split("/")[0];
    const At = Bare.indexOf("@");

    if(At <= 0){
        return undefined;
    }

    // A room's stanzas come from muc.<domain> (or conference.<domain>) and carry a nickname, not a sender
    return /^(muc|conference)\./i.test(Bare.slice(At + 1)) ? undefined : Bare.slice(0, At);
}

// The rule itself: true when a stanza to Uid's session breaks it
export function IsSelfStanza(Uid: string | undefined, Stanza: string): boolean {
    const Sender = SenderAccount(Stanza);

    return Uid !== undefined && Sender !== undefined && Sender.toLowerCase() === Uid.toLowerCase();
}

export function StanzasChecked(){
    return Seen;
}

ObserveChatOutputForTests((Uid, Stanza) => {
    Seen++;

    if(IsSelfStanza(Uid, Stanza)){
        SelfStanzas.push(`to ${Uid}: ${Stanza.slice(0, 300)}`);
    }
});

after(() => {
    ObserveChatOutputForTests(undefined);
    assert.deepEqual(SelfStanzas, [], "no stanza outside a room came from the receiving player's own account");
});
