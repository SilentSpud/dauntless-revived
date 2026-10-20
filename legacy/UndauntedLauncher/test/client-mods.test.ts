import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { disableModLines, insideRoot, installClientMods, safeRelative } from "../src/main/client-mods";

const sha = (s: string) => createHash("sha256").update(s).digest("hex");

// A launcher resources folder with a client-mods bundle, and a game folder (Archon/Binaries/Win64).
function setup(files: Record<string, string>, keep: string[], remove: string[], disable: string[] = []) {
  const root = mkdtempSync(path.join(os.tmpdir(), "dr-client-mods-"));
  const res = path.join(root, "resources");
  const game = path.join(root, "game");
  const win64 = path.join(game, "Archon", "Binaries", "Win64");
  mkdirSync(win64, { recursive: true });
  const list = [];
  for (const [p, text] of Object.entries(files)) {
    const full = path.join(res, "client-mods", ...p.split("/"));
    mkdirSync(path.dirname(full), { recursive: true });
    writeFileSync(full, text);
    list.push({ path: p, sha256: sha(text), size: text.length, keep: keep.includes(p) });
  }
  writeFileSync(path.join(res, "client-mods", "manifest.json"), JSON.stringify({ version: 1, target: "Archon/Binaries/Win64", files: list, remove, disable }));
  const put = (p: string, text: string) => {
    const full = path.join(win64, ...p.split("/"));
    mkdirSync(path.dirname(full), { recursive: true });
    writeFileSync(full, text);
  };
  const at = (p: string) => path.join(win64, ...p.split("/"));
  return { root, res, game, win64, put, at };
}

const BUNDLE = {
  "ue4ss/Mods/mods.txt": "BehemothHealthBars : 1\n",
  "ue4ss/Mods/BehemothHealthBars/Scripts/main.lua": "bars",
  "ue4ss/Mods/shared/FrameTick/FrameTick.lua": "tick",
};
const REMOVE = [
  "ue4ss/Mods/ZFXStatus/Scripts/main.lua",
  "ue4ss/Mods/ZFXStatus/zfx_logo.png",
  "ue4ss/Mods/shared/FrameTick/hb_ModMenu.txt",
];

test("an update removes the dropped mod and its empty folders and switches it off, keeping everything of the player's", async () => {
  const t = setup(BUNDLE, ["ue4ss/Mods/mods.txt"], REMOVE, ["ZFXStatus"]);
  try {
    t.put("ue4ss/Mods/ZFXStatus/Scripts/main.lua", "old");
    t.put("ue4ss/Mods/ZFXStatus/zfx_logo.png", "old");
    t.put("ue4ss/Mods/shared/FrameTick/hb_ModMenu.txt", "1.0");
    t.put("ue4ss/Mods/shared/FrameTick/hb_Other.txt", "not ours");
    t.put("ue4ss/Mods/mods.txt", "BehemothHealthBars : 0\r\nZFXStatus : 1\r\nZFXStatusExtra : 1\r\nMyMod : 1\r\n");
    t.put("ue4ss/Mods/MyMod/Scripts/main.lua", "mine");

    const r = await installClientMods(t.res, t.game);
    assert.deepEqual(r, { installed: 2, kept: 1, upToDate: 0, removed: 3, disabled: 1 });
    assert.equal(existsSync(t.at("ue4ss/Mods/ZFXStatus")), false, "the dropped mod's folder is gone");
    assert.equal(existsSync(t.at("ue4ss/Mods/shared/FrameTick/hb_ModMenu.txt")), false);
    assert.equal(existsSync(t.at("ue4ss/Mods/shared/FrameTick/hb_Other.txt")), true, "unlisted files stay");
    assert.equal(
      readFileSync(t.at("ue4ss/Mods/mods.txt"), "utf8"),
      "BehemothHealthBars : 0\r\nZFXStatus : 0\r\nZFXStatusExtra : 1\r\nMyMod : 1\r\n",
      "only the removed mod's line is switched off; the player's other choices and line endings stay",
    );
    assert.equal(readFileSync(t.at("ue4ss/Mods/MyMod/Scripts/main.lua"), "utf8"), "mine");
    assert.equal(readFileSync(t.at("ue4ss/Mods/BehemothHealthBars/Scripts/main.lua"), "utf8"), "bars");

    const again = await installClientMods(t.res, t.game);
    assert.deepEqual(again, { installed: 0, kept: 1, upToDate: 2, removed: 0, disabled: 0 }, "a second run changes nothing");
  } finally {
    rmSync(t.root, { recursive: true, force: true });
  }
});

test("a fresh install gets the bundle's mods.txt and nothing of the old mod", async () => {
  const t = setup(BUNDLE, ["ue4ss/Mods/mods.txt"], REMOVE, ["ZFXStatus"]);
  try {
    const r = await installClientMods(t.res, t.game);
    assert.deepEqual(r, { installed: 3, kept: 0, upToDate: 0, removed: 0, disabled: 0 });
    assert.equal(readFileSync(t.at("ue4ss/Mods/mods.txt"), "utf8"), "BehemothHealthBars : 1\n");
    assert.equal(existsSync(t.at("ue4ss/Mods/ZFXStatus")), false);
  } finally {
    rmSync(t.root, { recursive: true, force: true });
  }
});

test("a file the bundle still ships is never removed, even if listed in remove", async () => {
  const t = setup(BUNDLE, [], ["ue4ss/Mods/BehemothHealthBars/Scripts/main.lua"]);
  try {
    const r = await installClientMods(t.res, t.game);
    assert.equal(r?.removed, 0);
    assert.equal(r?.disabled, 0);
    assert.equal(readFileSync(t.at("ue4ss/Mods/BehemothHealthBars/Scripts/main.lua"), "utf8"), "bars");
  } finally {
    rmSync(t.root, { recursive: true, force: true });
  }
});

test("remove entries pointing outside the game folder are skipped, and nothing outside is touched", async () => {
  const hostile = ["../outside.txt", "..\\outside.txt", "ue4ss\\..\\..\\outside.txt", "ue4ss/../../outside.txt", "./x", "/outside.txt", "C:/outside.txt", "C:outside.txt", ""];
  const t = setup(BUNDLE, [], hostile);
  try {
    const outside = path.join(t.game, "Archon", "Binaries", "outside.txt");
    writeFileSync(outside, "must survive");
    const r = await installClientMods(t.res, t.game);
    assert.equal(r?.removed, 0);
    assert.equal(readFileSync(outside, "utf8"), "must survive");
    assert.equal(existsSync(t.win64), true, "the game folder itself stays");
  } finally {
    rmSync(t.root, { recursive: true, force: true });
  }
});

test("disableModLines switches off exactly the named mods", () => {
  const before = "A : 1\n  ZFXStatus\t:\t1  \nZFXStatus : 0\nZFXStatus2 : 1\n; ZFXStatus : 1\nB : 1";
  const r = disableModLines(before, ["ZFXStatus", "../bad", 5]);
  assert.equal(r.text, "A : 1\n  ZFXStatus\t:\t0  \nZFXStatus : 0\nZFXStatus2 : 1\n; ZFXStatus : 1\nB : 1");
  assert.equal(r.changed, 1);
  assert.deepEqual(disableModLines("A : 1\r\n", []), { text: "A : 1\r\n", changed: 0 });
});

test("bundle paths must be plain relative paths with forward slashes", () => {
  for (const bad of ["..\\x", "a\\b", "../x", "a/../b", "./a", "/a", "C:/a", "C:a", "a//b", ""]) {
    assert.throws(() => safeRelative(bad), `${JSON.stringify(bad)} must be refused`);
  }
  assert.equal(safeRelative("ue4ss/Mods/mods.txt"), path.join("ue4ss", "Mods", "mods.txt"));
  const root = path.join(os.tmpdir(), "dr-root");
  assert.equal(insideRoot(root, "a/b.txt"), path.join(path.resolve(root), "a", "b.txt"));
  assert.throws(() => insideRoot(root, "../b.txt"));
});

test("upgrading adds CraftStar once, preserving disabled mods and Keybinds order", async () => {
  const mods = "ue4ss/Mods/mods.txt";
  const t = setup({ [mods]: "CraftStar : 1\nBehemothHealthBars : 1\nKeybinds : 1\n" }, [mods], []);
  try {
    t.put(mods, "BehemothHealthBars : 0\r\nKeybinds : 1\r\n");
    await installClientMods(t.res, t.game);
    const expected = "CraftStar : 1\r\nBehemothHealthBars : 0\r\nKeybinds : 1\r\n";
    assert.equal(readFileSync(t.at(mods), "utf8"), expected);
    t.put(mods, expected.replace("CraftStar : 1", "CraftStar : 0"));
    await installClientMods(t.res, t.game);
    assert.equal(readFileSync(t.at(mods), "utf8"), expected.replace("CraftStar : 1", "CraftStar : 0"));
  } finally { rmSync(t.root, { recursive: true, force: true }); }
});
