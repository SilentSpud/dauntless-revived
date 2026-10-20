import { test } from "node:test";
import assert from "node:assert/strict";
import { ParseFileRequestPath } from "../src/paths";
import { IsSafeManifestPath } from "../src/manifest";

const P = "/content/v1/files/";

test("plain manifest paths parse to themselves", () => {
    for(const Good of ["Version.txt", "Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe", "Engine/Content/Internationalization/icudt53l/brkitr/root.res", "EasyAntiCheat/Licenses/CCO-1.0.txt"]){
        assert.deepEqual(ParseFileRequestPath(P + Good), { ok: true, path: Good });
    }
});

test("the query string is ignored", () => {
    assert.deepEqual(ParseFileRequestPath(P + "Version.txt?x=1"), { ok: true, path: "Version.txt" });
});

test("harmless percent-encoding is decoded once", () => {
    assert.deepEqual(ParseFileRequestPath(P + "Archon/Binaries/Win64/Dauntless%2DWin64%2DShipping.exe"), { ok: true, path: "Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe" });
});

test("traversal and smuggling shapes are refused", () => {
    const Bad: [string, string][] = [
        ["../../Windows/win.ini", "bad_segment"],
        ["Archon/../../x", "bad_segment"],
        ["./Version.txt", "bad_segment"],
        ["Archon//Version.txt", "bad_segment"],
        ["Archon/", "bad_segment"],
        ["..%2f..%2fWindows/win.ini", "encoded_separator"],
        ["..%2F..%2FWindows/win.ini", "encoded_separator"],
        ["%2e%2e/%2e%2e/x", "encoded_separator"],
        ["%2E%2E/x", "encoded_separator"],
        ["Archon%5c..%5cx", "encoded_separator"],
        ["Version.txt%00.png", "encoded_separator"],
        ["Archon\\..\\x", "raw_forbidden_char"],
        ["Version.txt\0", "raw_forbidden_char"],
        ["%252e%252e/x", "double_encoding"],
        ["%25", "double_encoding"],
        ["C:/Windows/win.ini", "forbidden_char"],
        ["Version.txt::$DATA", "forbidden_char"],
        ["Version.txt%3A%3A$DATA", "forbidden_char"],
        ["a%0ab", "forbidden_char"],
        ["a%7fb", "forbidden_char"],
        ["%E0%A4%A", "bad_encoding"],
        ["", "empty"],
    ];

    for(const [Rest, Reason] of Bad){
        const Result = ParseFileRequestPath(P + Rest);
        assert.equal(Result.ok, false, `should refuse ${JSON.stringify(Rest)}`);
        if(!Result.ok){
            assert.equal(Result.reason, Reason, `reason for ${JSON.stringify(Rest)}`);
        }
    }
});

test("only the files prefix is accepted and long targets are refused", () => {
    assert.equal(ParseFileRequestPath("/content/v1/files").ok, false);
    assert.equal(ParseFileRequestPath("/content/v1/manifest").ok, false);
    assert.equal(ParseFileRequestPath("//content/v1/files/Version.txt").ok, false);
    assert.equal(ParseFileRequestPath(P + "a/".repeat(600) + "b").ok, false);
});

test("manifest path rule", () => {
    for(const Good of ["Version.txt", "Archon/Content/Paks/Archon_Audio_0-WindowsClient.pak", "EasyAntiCheat/Licenses/CCO-1.0.txt", "a", "Engine/Binaries/Win64/debug.log"]){
        assert.equal(IsSafeManifestPath(Good), true, Good);
    }
    for(const Bad of ["", "/Version.txt", "Version.txt/", "a//b", "../x", "a/../b", "./a", ".hidden", "trailing.", "a\\b", "C:/x", "a b", "CON", "nul.txt", "Archon/COM1/x", "a:b", "x".repeat(401), "ä.txt"]){
        assert.equal(IsSafeManifestPath(Bad), false, JSON.stringify(Bad));
    }
});
