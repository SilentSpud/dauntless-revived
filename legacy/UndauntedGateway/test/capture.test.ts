import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import {
    CaptureWriter, LoadCaptureOptions, RedactCaptureBody, RedactCaptureHeaders, RedactCaptureJson, RedactCaptureText
} from "../src/capture";
import { Gateway } from "../src/gateway";
import { FakeUpstream, HttpsRequest, MakeCert, StartUpstream, TempDir, TestCert, TestConfig } from "./helpers";

// Traffic capture (MIGRATION_PLAN.md M2): what is kept for the shadow replay, and that no token, key
// or secret is ever written. Ports: gateway 62440, fake metagame 62441, content 62442, ws 62443.
const P = { gateway: 62440, metagame: 62441, content: 62442, ws: 62443 };

function Jwt(Claims: object): string {
    const Part = (Value: object) => Buffer.from(JSON.stringify(Value)).toString("base64url");
    return `${Part({ alg: "RS256", typ: "JWT" })}.${Part(Claims)}.${"s".repeat(43)}`;
}

const TOKEN = Jwt({ userId: "UID-capture-a", epoch: 0 });
const KEY = "UUK_" + "0a1b2c3d4e5f".repeat(4);
const KEY_HASH = crypto.createHash("sha256").update(KEY, "utf8").digest("hex");
const GS_KEY = "f".repeat(48);

describe("capture redaction", () => {
    it("turns a token into its subject and a key into the hash the database stores", () => {
        assert.equal(RedactCaptureText(`bearer ${TOKEN}`), "bearer <token:UID-capture-a>");
        assert.equal(RedactCaptureText(`key=${KEY}&x=1`), `key=<key:${KEY_HASH}>&x=1`);
        assert.equal(RedactCaptureText(Jwt({ nobody: true })), "<token>");
        assert.equal(RedactCaptureText(`secret ${"ab".repeat(30)} here`), "secret <redacted> here");
        assert.equal(RedactCaptureText("\u00011\u0001"), "?1?", "a forged placeholder marker is defused");
    });

    it("keeps headers for the replay without their secrets", () => {
        const Out = RedactCaptureHeaders({
            authorization: `bearer ${TOKEN}`, "x-undaunted-user-api-key": KEY, "x-undaunted-gameserver-apikey": GS_KEY,
            cookie: "a=b", "x-dauntless-gateway": "secret", "user-agent": "Dauntless/1.4.4", "content-type": "application/json"
        });
        assert.deepEqual(Out, {
            authorization: "bearer <token:UID-capture-a>", "x-undaunted-user-api-key": `<key:${KEY_HASH}>`,
            "x-undaunted-gameserver-apikey": "<gskey>", "user-agent": "Dauntless/1.4.4", "content-type": "application/json"
        });
    });

    it("redacts secret-named fields in JSON and forms, keeping token and key placeholders", () => {
        assert.deepEqual(RedactCaptureJson({ Username: "Alex", InviteCode: "ABCD-EFGH", Password: "hunter2", UUK: KEY, nested: [{ access_token: TOKEN, note: "x" }] }), {
            Username: "Alex", InviteCode: "<redacted>", Password: "<redacted>", UUK: `<key:${KEY_HASH}>`, nested: [{ access_token: "<token:UID-capture-a>", note: "x" }]
        });
        assert.deepEqual(RedactCaptureBody(Buffer.from(`grant_type=password&username=Alex&password=${KEY}`), "application/x-www-form-urlencoded", 77, 1024), {
            form: [["grant_type", "password"], ["username", "Alex"], ["password", `<key:${KEY_HASH}>`]]
        });
    });

    it("keeps text, and only the size of binary, oversized or cut bodies", () => {
        assert.deepEqual(RedactCaptureBody(Buffer.from(`hello ${TOKEN}`), "text/plain", 6 + TOKEN.length, 1024), { text: "hello <token:UID-capture-a>" });
        assert.deepEqual(RedactCaptureBody(Buffer.from([0, 1, 2, 3]), "application/octet-stream", 4, 1024), { binary: 4 });
        assert.deepEqual(RedactCaptureBody(Buffer.from("abc"), "text/plain", 5000, 1024), { omitted: 5000 });
        assert.equal(RedactCaptureBody(Buffer.alloc(0), undefined, 0, 1024), null);
    });

    it("is off without CAPTURE_DIR, and refuses a bad retention", () => {
        assert.equal(LoadCaptureOptions({}, "gateway"), undefined);
        const On = LoadCaptureOptions({ CAPTURE_DIR: "x" }, "gateway");
        assert.equal(On?.retentionDays, 14);
        assert.equal(On?.source, "gateway");
        assert.throws(() => LoadCaptureOptions({ CAPTURE_DIR: "x", CAPTURE_RETENTION_DAYS: "0" }, "gateway"), /CAPTURE_RETENTION_DAYS/);
    });
});

describe("capture files", () => {
    it("writes one private file per day and removes the ones past the retention", async () => {
        const Dir = TempDir("capture-files");
        let Now = Date.parse("2026-10-20T23:59:59Z");
        fs.writeFileSync(path.join(Dir, "gateway-2026-10-01.jsonl"), "{}\n");
        fs.writeFileSync(path.join(Dir, "notes.txt"), "kept");
        const Writer = new CaptureWriter({ dir: Dir, retentionDays: 14, maxBodyBytes: 1024, source: "gateway" }, () => undefined, () => Now);
        Writer.Write({ kind: "http", seq: 1 });
        Now += 2000;
        Writer.Write({ kind: "http", seq: 2 });
        await Writer.Close();
        assert.deepEqual(fs.readdirSync(Dir).sort(), ["gateway-2026-10-20.jsonl", "gateway-2026-10-21.jsonl", "notes.txt"]);
        const Line = JSON.parse(fs.readFileSync(path.join(Dir, "gateway-2026-10-21.jsonl"), "utf8"));
        assert.deepEqual(Line, { t: "2026-10-21T00:00:01.000Z", src: "gateway", kind: "http", seq: 2 });
        if(process.platform !== "win32"){
            assert.equal(fs.statSync(path.join(Dir, "gateway-2026-10-21.jsonl")).mode & 0o777, 0o600);
        }
    });

    it("the gateway and the metagame keep the same capture module", () => {
        const Here = fs.readFileSync(path.join(__dirname, "..", "..", "src", "capture.ts"), "utf8");
        const There = fs.readFileSync(path.join(__dirname, "..", "..", "..", "UndauntedMetagame", "src", "capture.ts"), "utf8");
        assert.equal(Here.replace(/\r\n/g, "\n"), There.replace(/\r\n/g, "\n"));
    });
});

describe("capture through the gateway", () => {
    let Cert: TestCert;
    let Metagame: FakeUpstream, Content: FakeUpstream, Ws: FakeUpstream;
    let TheGateway: Gateway;
    let Dir: string;

    before(async () => {
        Cert = MakeCert();
        Dir = TempDir("capture-gateway");
        Metagame = await StartUpstream(P.metagame, "metagame", (Req, Res) => {
            if(Req.url !== "/account/api/oauth/token"){
                return false;
            }
            const Text = JSON.stringify({ access_token: TOKEN, account_id: "UID-capture-a", expires_in: 86400 });
            Res.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(Text) });
            Res.end(Text);
            return true;
        });
        Content = await StartUpstream(P.content, "content");
        Ws = await StartUpstream(P.ws, "ws");
        const Options = { dir: Dir, retentionDays: 14, maxBodyBytes: 4096, source: "gateway" };
        TheGateway = new Gateway(TestConfig(P), { cert: Cert.certPem, key: Cert.keyPem }, undefined, new CaptureWriter(Options));
        await TheGateway.Listen();
    });

    after(async () => {
        await TheGateway.Close();
        await Promise.all([Metagame.close(), Content.close(), Ws.close()]);
    });

    it("records the request and the answer, redacted, and refusals too", async () => {
        const Login = await HttpsRequest(P.gateway, "/account/api/oauth/token", {
            method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: `grant_type=password&username=Alex&password=${KEY}`
        });
        assert.equal(Login.status, 200);
        await HttpsRequest(P.gateway, "/fortnite/api/game/v2/profile/UID-capture-a/client/QueryProfile", {
            method: "POST", headers: { authorization: `bearer ${TOKEN}`, "content-type": "application/json" }, body: JSON.stringify({ profileId: "athena" })
        });
        const Refused = await HttpsRequest(P.gateway, "/undaunted/admin/anything", { headers: { "x-undaunted-gameserver-apikey": GS_KEY } });
        await TheGateway.Close();
        TheGateway = new Gateway(TestConfig(P), { cert: Cert.certPem, key: Cert.keyPem });
        await TheGateway.Listen();

        const Text = fs.readdirSync(Dir).map((File) => fs.readFileSync(path.join(Dir, File), "utf8")).join("");
        const Lines = Text.trim().split("\n").map((Line) => JSON.parse(Line));
        assert.equal(Lines.length, 3, Text);
        const [First, Second, Third] = Lines;
        assert.equal(First.kind, "http");
        assert.equal(First.route, "metagame");
        assert.equal(First.status, 200);
        assert.deepEqual(First.reqBody, { form: [["grant_type", "password"], ["username", "Alex"], ["password", `<key:${KEY_HASH}>`]] });
        assert.deepEqual(First.resBody, { json: { access_token: "<token:UID-capture-a>", account_id: "UID-capture-a", expires_in: 86400 } });
        assert.equal(Second.reqHeaders.authorization, "bearer <token:UID-capture-a>");
        assert.deepEqual(Second.reqBody, { json: { profileId: "athena" } });
        assert.equal(Second.resBody.json.upstream, "metagame");
        assert.equal(Third.status, Refused.status);
        assert.equal(Third.route, "gateway");
        assert.equal(Third.reqHeaders["x-undaunted-gameserver-apikey"], "<gskey>");
        assert.ok(First.seq < Second.seq && Second.seq < Third.seq);
        assert.ok(!Text.includes(KEY) && !Text.includes(TOKEN) && !Text.includes(GS_KEY), "no key, token or game-server key is written");
        assert.ok(!Text.includes("test-gateway-secret"), "nor the gateway secret");
    });
});
