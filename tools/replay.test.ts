import { describe, expect, it } from "bun:test";
import { applyIds, buildRequest, comparableFrames, compareReplies, type Credentials, firstDifference, type HttpRecord, learnIds, Normalizer, readCorpus, substitute } from "./replay-core.ts";

const credentials: Credentials = {
    token: uid => `token-for-${uid}`,
    key: hash => hash === "a".repeat(64) ? "UUK_replay" : undefined,
    gsKey: "gs-replay",
};

function record(overrides: Partial<HttpRecord>): HttpRecord {
    return {
        kind: "http", t: "2026-10-21T10:00:00.000Z", src: "gateway", conn: 1, seq: 1, ip: "203.0.113.5", method: "GET", target: "/x",
        route: "metagame", reqHeaders: {}, reqBody: null, status: 200, resHeaders: {}, resBody: null, ms: 1, ...overrides,
    };
}

describe("replay placeholders", () => {
    it("puts tokens, keys, logins and the game-server key back", () => {
        expect(substitute("bearer <token:UID-a>", credentials)).toEqual({ text: "bearer token-for-UID-a", exact: true });
        expect(substitute(`<key:${"a".repeat(64)}>`, credentials)).toEqual({ text: "UUK_replay", exact: true });
        expect(substitute("<gskey>", credentials)).toEqual({ text: "gs-replay", exact: true });
        expect(Buffer.from(substitute("<plain:UID-a>", credentials).text, "base64").toString()).toBe("\0UID-a\0token-for-UID-a");
    });

    it("marks a request whose secret was cut as inexact", () => {
        expect(substitute("<redacted>", credentials).exact).toBe(false);
        expect(substitute(`<key:${"b".repeat(64)}>`, credentials).exact).toBe(false);
    });
});

describe("replay requests", () => {
    it("rebuilds a gateway request as the gateway forwarded it", () => {
        const request = buildRequest(record({
            method: "POST", target: "/account/api/oauth/token", reqHeaders: { host: "x", "content-type": "application/x-www-form-urlencoded", "content-length": "9" },
            reqBody: { form: [["grant_type", "password"], ["password", `<key:${"a".repeat(64)}>`]] },
        }), credentials, "secret");
        expect(request).toEqual({
            method: "POST", path: "/account/api/oauth/token", exact: true, body: "grant_type=password&password=UUK_replay",
            headers: { "content-type": "application/x-www-form-urlencoded", "x-dauntless-gateway": "secret", "x-forwarded-for": "203.0.113.5", "x-forwarded-proto": "https" },
        });
    });

    it("sends a game server's request as it came, and skips what it cannot replay", () => {
        const request = buildRequest(record({ src: "metagame", reqHeaders: { "x-undaunted-gameserver-apikey": "<gskey>" }, reqBody: { json: { a: "<token:UID-b>" } } }), credentials, "secret");
        expect(request).toEqual({ method: "GET", path: "/x", exact: true, body: '{"a":"token-for-UID-b"}', headers: { "x-undaunted-gameserver-apikey": "gs-replay" } });
        expect(buildRequest(record({ route: "content" }), credentials, undefined)).toEqual({ skip: "route content" });
        expect(buildRequest(record({ reqBody: { omitted: 9_000_000 } }), credentials, undefined)).toEqual({ skip: "body not recorded" });
    });
});

describe("replay comparison", () => {
    it("normalises dates, times, generated ids and tokens, per server", () => {
        const left = { id: "0f0e0d0c-0b0a-4908-8706-050403020100", other: "0f0e0d0c-0b0a-4908-8706-050403020100", at: "2026-10-21T10:00:00.123Z", ms: Date.now(), count: 3 };
        const right = { id: "11111111-2222-4333-8444-555555555555", other: "11111111-2222-4333-8444-555555555555", at: "2026-10-21T10:00:01Z", ms: Date.now() - 5, count: 3 };
        expect(compareReplies({ status: 200, body: JSON.stringify(left) }, { status: 200, body: JSON.stringify(right) }, { node: new Normalizer(), rust: new Normalizer() })).toBeUndefined();
        expect(compareReplies({ status: 200, body: '{"a":1}' }, { status: 404, body: '{"a":1}' }, { node: new Normalizer(), rust: new Normalizer() })).toBe("status 200 vs 404");
        expect(compareReplies({ status: 200, body: '{"name":"x","version":"0.0.5","commit":"abc"}' }, { status: 200, body: '{"name":"x","version":"0.0.0","commit":"unknown"}' }, { node: new Normalizer(), rust: new Normalizer() })).toBeUndefined();
        expect(compareReplies({ status: 200, body: '{"a":1,"b":2}' }, { status: 200, body: '{"b":2,"a":1}' }, { node: new Normalizer(), rust: new Normalizer() })).toBe("body at $ (key order)");
    });

    it("finds the first difference", () => {
        expect(firstDifference({ a: [1, 2] }, { a: [1, 3] })).toBe("$.a[1]");
        expect(firstDifference({ a: 1 }, { b: 1 })).toBe("$.a");
        expect(firstDifference([1], [1, 2])).toBe("$.length");
    });

    it("compares chat output without liveness pings or random ids", () => {
        const frames = ['<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" from="x" id="0123456789abcdef" version="1.0"/>',
            '<iq type="get" id="sp1"><ping xmlns="urn:xmpp:ping"/></iq>', "<jid>UID-a@x/srv-0123456789abcdef0123456789abcdef</jid>"];
        expect(comparableFrames(frames, new Normalizer())).toEqual(['<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" from="x" id="<id>" version="1.0"/>', "<jid>UID-a@x/srv-<hex></jid>"]);
    });

    it("reads a corpus from several files in time order", () => {
        const a = JSON.stringify(record({ t: "2026-10-21T10:00:02.000Z", seq: 2 })) + "\n";
        const b = JSON.stringify(record({ t: "2026-10-21T10:00:01.000Z", seq: 1 })) + "\n{cut";
        expect(readCorpus([a, b]).map(item => (item as HttpRecord).seq)).toEqual([1, 2]);
    });
});

describe("replay ids", () => {
    it("learns a server's own id for a recorded one and uses it in later requests", () => {
        const ids = new Map<string, string>();
        learnIds({ id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", name: "x" }, { id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", name: "x" }, ids);
        learnIds({ id: "UID-cccccccc-cccc-4ccc-8ccc-cccccccccccc" }, { id: "UID-cccccccc-cccc-4ccc-8ccc-cccccccccccc" }, ids);
        expect([...ids]).toEqual([["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"]]);
        const request = applyIds({ method: "GET", path: "/inventory/u/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", headers: {}, body: '{"c":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}', exact: true }, ids);
        expect(request.path).toBe("/inventory/u/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
        expect(request.body).toBe('{"c":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"}');
    });
});
