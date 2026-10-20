import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { compareManifests, isSafeRelativePath, resolveInside, validateManifest, manifestFingerprint, type GameManifest } from "../src/main/manifest";
import { EXE_RELATIVE_PATH, PINNED_EXE_SHA256, PINNED_EXE_SIZE } from "../src/main/constants";
import { makeFiles, manifestFor, TEST_BUILD } from "./fakes";

const base = manifestFor(makeFiles());

function valid(): GameManifest {
  const r = validateManifest(base, TEST_BUILD, false);
  assert.ok(r.ok);
  return r.ok ? r.manifest : (null as never);
}

test("a well-formed manifest validates and is sorted", () => {
  const m = valid();
  assert.equal(m.files.length, 6);
  assert.deepEqual(m.files.map((f) => f.path), [...m.files.map((f) => f.path)].sort());
});

test("the real manifest must contain the pinned game exe", () => {
  const withoutExe = validateManifest(base, TEST_BUILD, true);
  assert.equal(withoutExe.ok, false);
  const files = [...base.files, { path: EXE_RELATIVE_PATH, size: PINNED_EXE_SIZE, sha256: PINNED_EXE_SHA256 }];
  const m = { build: TEST_BUILD, totalBytes: base.totalBytes + PINNED_EXE_SIZE, files };
  assert.equal(validateManifest(m, TEST_BUILD, true).ok, true);
  const wrongHash = { ...m, files: [...base.files, { path: EXE_RELATIVE_PATH, size: PINNED_EXE_SIZE, sha256: "0".repeat(64) }] };
  assert.equal(validateManifest(wrongHash, TEST_BUILD, true).ok, false);
});

test("rejects broken manifests", () => {
  const cases: unknown[] = [
    null,
    { ...base, build: "other" },
    { ...base, files: [] },
    { ...base, totalBytes: base.totalBytes + 1 },
    { ...base, files: [...base.files, { ...base.files[0] }], totalBytes: base.totalBytes + base.files[0].size },
    { ...base, files: [...base.files, { ...base.files[0], path: base.files[0].path.toUpperCase() }], totalBytes: base.totalBytes + base.files[0].size },
    { ...base, files: [{ path: "../evil.dll", size: 1, sha256: "a".repeat(64) }], totalBytes: 1 },
    { ...base, files: [{ path: "a", size: -1, sha256: "a".repeat(64) }], totalBytes: -1 },
    { ...base, files: [{ path: "a", size: 1.5, sha256: "a".repeat(64) }], totalBytes: 1.5 },
    { ...base, files: [{ path: "a", size: 1, sha256: "A".repeat(64) }], totalBytes: 1 },
    { ...base, files: [{ path: "Archon/Binaries/Win64/dxgi.dll", size: 1, sha256: "a".repeat(64) }], totalBytes: 1 },
  ];
  for (const c of cases) assert.equal(validateManifest(c, TEST_BUILD, false).ok, false, JSON.stringify(c)?.slice(0, 100));
});

test("path safety", () => {
  for (const p of ["Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak", "Version.txt", "Engine/Binaries/ThirdParty/Ogg/Win64/VS2015/libogg_64.dll", "a/intro (1).bk2"]) {
    assert.equal(isSafeRelativePath(p), true, p);
  }
  for (const p of [
    "", "/abs", "C:/x", "C:x", "a\\b", "a/../b", "./a", "a//b", "a/", "..", "a/.", "a:stream", "CON", "a/nul.txt", "a/COM1", "trail. ", "trail.", " lead", "a/b\u0000", "é.txt", "a?b", "a*b", "x".repeat(241),
  ]) {
    assert.equal(isSafeRelativePath(p), false, JSON.stringify(p));
  }
  const root = path.resolve("C:/games/dr");
  assert.equal(resolveInside(root, "Archon/Binaries/Win64/x.exe"), path.join(root, "Archon", "Binaries", "Win64", "x.exe"));
  assert.throws(() => resolveInside(root, "../x"));
});

test("compareManifests: identical, reordered, and every kind of difference", () => {
  const m = valid();
  assert.deepEqual(compareManifests(m, base), { equal: true });
  assert.deepEqual(compareManifests(m, { ...base, files: [...base.files].reverse() }), { equal: true });

  const changedHash = JSON.parse(JSON.stringify(base));
  changedHash.files[1].sha256 = "f".repeat(64);
  const r1 = compareManifests(m, changedHash);
  assert.equal(r1.equal, false);
  if (!r1.equal) assert.match(r1.difference, /hash differs/);

  const changedSize = JSON.parse(JSON.stringify(base));
  changedSize.files[0].size += 1;
  changedSize.totalBytes += 1;
  const r2 = compareManifests(m, changedSize);
  assert.equal(r2.equal, false);
  if (!r2.equal) assert.match(r2.difference, /size|total/);

  const extra = JSON.parse(JSON.stringify(base));
  extra.files.push({ path: "Archon/extra.pak", size: 0, sha256: "e".repeat(64) });
  assert.equal(compareManifests(m, extra).equal, false);

  const renamed = JSON.parse(JSON.stringify(base));
  renamed.files[0].path = "Archon/renamed.bin";
  assert.equal(compareManifests(m, renamed).equal, false);

  assert.equal(compareManifests(m, { ...base, build: "dauntless_rel-1.4.5" }).equal, false);
  assert.equal(compareManifests(m, "nope").equal, false);
  assert.equal(manifestFingerprint(m), manifestFingerprint(valid()));
});
