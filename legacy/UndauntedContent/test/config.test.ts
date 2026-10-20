import { test } from "node:test";
import assert from "node:assert/strict";
import { IsPrivateBindHost, LoadConfig } from "../src/config";

test("defaults", () => {
    const C = LoadConfig({ CONTENT_GAME_DIR: "C:\\Games\\Dauntless" });
    assert.equal(C.port, 61002);
    assert.deepEqual(C.bindHosts, ["127.0.0.1"]);
    assert.equal(C.metagameUrl, "http://127.0.0.1:61000");
    assert.equal(C.maxStreamsPerAccount, 6);
    assert.equal(C.authCacheSeconds, 300);
    assert.equal(C.brandingDir, undefined);
    assert.equal(C.newsFile, undefined);
});

test("the game folder is required", () => {
    assert.throws(() => LoadConfig({}), /CONTENT_GAME_DIR/);
});

test("optional CDN base is HTTPS without credentials, query or fragment", () => {
    assert.equal(LoadConfig({CONTENT_GAME_DIR: "x"}).downloadBaseUrl, undefined);
    assert.equal(LoadConfig({CONTENT_GAME_DIR: "x", CONTENT_DOWNLOAD_BASE_URL: "https://downloads.example/game"}).downloadBaseUrl, "https://downloads.example/game/");
    for(const Bad of ["http://example/game", "https://u:p@example/game", "https://example/game?q=1", "https://example/game#x"]){
        assert.throws(() => LoadConfig({CONTENT_GAME_DIR: "x", CONTENT_DOWNLOAD_BASE_URL: Bad}));
    }
});

test("bind addresses: loopback and Tailscale only, unless explicitly allowed", () => {
    for(const Ok of ["127.0.0.1", "::1", "localhost", "100.64.0.1", "100.101.102.103", "100.127.255.254", "fd7a:115c:a1e0::1"]){
        assert.equal(IsPrivateBindHost(Ok), true, Ok);
    }
    for(const Bad of ["0.0.0.0", "::", "192.168.1.10", "10.0.0.5", "100.63.255.255", "100.128.0.1", "8.8.8.8", "fe80::1"]){
        assert.equal(IsPrivateBindHost(Bad), false, Bad);
    }

    const C = LoadConfig({ CONTENT_GAME_DIR: "x", BIND_HOST: "127.0.0.1, 100.100.1.2" });
    assert.deepEqual(C.bindHosts, ["127.0.0.1", "100.100.1.2"]);

    assert.throws(() => LoadConfig({ CONTENT_GAME_DIR: "x", BIND_HOST: "0.0.0.0" }), /Refusing/);
    assert.throws(() => LoadConfig({ CONTENT_GAME_DIR: "x", BIND_HOST: "my-host.example" }), /IP addresses/);
    assert.deepEqual(LoadConfig({ CONTENT_GAME_DIR: "x", BIND_HOST: "0.0.0.0", CONTENT_ALLOW_ANY_BIND: "1" }).bindHosts, ["0.0.0.0"]);
});

test("numbers and URLs are checked", () => {
    assert.throws(() => LoadConfig({ CONTENT_GAME_DIR: "x", PORT: "70000" }), /PORT/);
    assert.throws(() => LoadConfig({ CONTENT_GAME_DIR: "x", CONTENT_MAX_STREAMS_PER_ACCOUNT: "0" }), /STREAMS/);
    assert.throws(() => LoadConfig({ CONTENT_GAME_DIR: "x", METAGAME_URL: "ftp://x" }), /METAGAME_URL/);
    assert.equal(LoadConfig({ CONTENT_GAME_DIR: "x", METAGAME_URL: "http://100.64.0.1:61000///" }).metagameUrl, "http://100.64.0.1:61000");
});
