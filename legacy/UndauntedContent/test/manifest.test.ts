import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DefaultManifestPath, EXPECTED_BUILD, IndexManifest, LoadManifest, ValidateManifest } from "../src/manifest";
import { CheckGameDir } from "../src/gamedir";

const SHA = "a".repeat(64);

function Doc(Files: { path: string; size: number; sha256?: string }[], Total?: number){
    return {
        build: "test",
        totalBytes: Total ?? Files.reduce((S, F) => S + F.size, 0),
        files: Files.map((F) => ({ sha256: SHA, ...F })),
    };
}

test("the checked-in 1.4.4 manifest loads and is what we expect", () => {
    const M = LoadManifest(DefaultManifestPath());
    assert.equal(M.build, EXPECTED_BUILD);
    assert.equal(M.files.length, 410);
    assert.equal(M.totalBytes, 10893512875);

    const Index = IndexManifest(M);
    const Exe = Index.get("Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe");
    assert.ok(Exe);
    assert.equal(Exe.size, 103673520);
    assert.equal(Exe.sha256, "d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4");
    assert.equal(Index.has("Version.txt"), true);
    assert.equal(Index.has("version.txt"), false, "lookup is exact, case included");
});

test("a valid manifest passes", () => {
    const M = ValidateManifest(Doc([{ path: "A/b.txt", size: 3 }, { path: "B", size: 0 }]));
    assert.equal(M.files.length, 2);
    assert.equal(M.totalBytes, 3);
});

test("invalid manifests are refused", () => {
    const Cases: [unknown, RegExp][] = [
        [null, /not an object/],
        [[], /not an object/],
        [{ build: "x", totalBytes: 0, files: [] }, /missing or empty/],
        [Doc([{ path: "../x", size: 1 }]), /unsafe/],
        [Doc([{ path: "a\\b", size: 1 }]), /unsafe/],
        [Doc([{ path: "a", size: -1 }]), /bad size/],
        [Doc([{ path: "a", size: 1.5 }]), /bad size/],
        [Doc([{ path: "a", size: 1, sha256: "A".repeat(64) }]), /bad sha256/],
        [Doc([{ path: "a", size: 1, sha256: "abc" }]), /bad sha256/],
        [Doc([{ path: "B", size: 1 }, { path: "a", size: 1 }, { path: "b", size: 1 }]), /duplicate/],
        [Doc([{ path: "b", size: 1 }, { path: "a", size: 1 }]), /not sorted/],
        [Doc([{ path: "a", size: 1 }], 2), /totalBytes/],
    ];
    for(const [Raw, Pattern] of Cases){
        assert.throws(() => ValidateManifest(Raw), Pattern);
    }
});

test("the startup check reports missing and wrong-size files", () => {
    const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-content-gamedir-"));
    try{
        fs.mkdirSync(path.join(Dir, "A"));
        fs.writeFileSync(path.join(Dir, "A", "ok.bin"), "abc");
        fs.writeFileSync(path.join(Dir, "A", "short.bin"), "ab");
        fs.mkdirSync(path.join(Dir, "A", "dir.bin"));
        const M = ValidateManifest(Doc([
            { path: "A/dir.bin", size: 0 },
            { path: "A/gone.bin", size: 5 },
            { path: "A/ok.bin", size: 3 },
            { path: "A/short.bin", size: 3 },
        ]));
        const R = CheckGameDir(Dir, M);
        assert.equal(R.ok, 1);
        assert.equal(R.bytesOk, 3);
        assert.deepEqual(R.missing, ["A/gone.bin"]);
        assert.deepEqual(R.mismatched, ["A/dir.bin", "A/short.bin"]);
        assert.equal(R.states.get("A/ok.bin")?.status, "ok");
        assert.deepEqual(R.states.get("A/short.bin"), { status: "size_mismatch", actual: 2 });
        assert.deepEqual(R.states.get("A/dir.bin"), { status: "not_a_file" });
    }
    finally{
        fs.rmSync(Dir, { recursive: true, force: true });
    }
});
