// "I already have the game files" (pull request #8): which folders are recognised as the 1.4.4 game,
// and which paths are refused before anything touches the disk.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { cleanGameFolderInput, findExistingGameRoot, locateExistingGame, MAX_GAME_ROOT } from "../src/main/game-folder";
import { EXE_RELATIVE_PATH } from "../src/main/constants";

function makeGame(root: string): string {
  const exe = path.join(root, ...EXE_RELATIVE_PATH.split("/"));
  mkdirSync(path.dirname(exe), { recursive: true });
  writeFileSync(exe, "fixture");
  return exe;
}

test("finds the game from BaseGame144, Dauntless, Archon or Win64, also pasted with quotes or forward slashes", async () => {
  const base = mkdtempSync(path.join(tmpdir(), "dr-existing-game-"));
  try {
    const game = path.join(base, "Dauntless");
    const exe = makeGame(game);
    for (const selected of [base, game, path.join(game, "Archon"), path.dirname(exe), `"${base}"`, `  ${game}  `, base.replace(/\\/g, "/")]) {
      assert.equal(await findExistingGameRoot(selected), game, selected);
    }
    // The folder is kept as typed: no realpath, so a short 8.3 temp folder stays short.
    assert.equal(await findExistingGameRoot(base), path.join(base, "Dauntless"));
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});

test("a folder without the game executable is not a game", async () => {
  const base = mkdtempSync(path.join(tmpdir(), "dr-not-game-"));
  try {
    mkdirSync(path.join(base, "Dauntless", "Archon"), { recursive: true });
    assert.deepEqual(await locateExistingGame(base), { ok: false, reason: "not_found" });
    assert.deepEqual(await locateExistingGame(path.join(base, "nothing-here")), { ok: false, reason: "not_found" });
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});

test("only drive-letter paths: network, device and relative paths are refused before any file-system call", async () => {
  for (const bad of [
    "",
    "   ",
    '""',
    "relative\\BaseGame144",
    "BaseGame144",
    "\\Games\\BaseGame144", // root-relative
    "\\\\host.invalid\\share\\BaseGame144", // UNC: looking at it would contact that host
    "//host.invalid/share/BaseGame144",
    "\\\\?\\C:\\Games\\BaseGame144", // device paths
    "\\\\.\\C:\\Games\\BaseGame144",
    "C:Games\\BaseGame144", // drive-relative
    "C:\\Games\\Base\0Game144",
    "C:\\" + "a".repeat(300),
  ]) {
    assert.equal(cleanGameFolderInput(bad, "win32"), null, JSON.stringify(bad));
    assert.deepEqual(await locateExistingGame(bad, "win32"), { ok: false, reason: "invalid_path" }, JSON.stringify(bad));
  }
  assert.equal(cleanGameFolderInput("c:/Games/BaseGame144/", "win32"), "c:\\Games\\BaseGame144");
});

test("a game root longer than the install-folder limit is refused, so no game file passes 260 characters", async () => {
  const base = mkdtempSync(path.join(tmpdir(), "dr-long-game-"));
  try {
    let deep = base;
    while (deep.length <= MAX_GAME_ROOT) deep = path.join(deep, "folder-name-that-is-long");
    makeGame(deep);
    assert.deepEqual(await locateExistingGame(deep), { ok: false, reason: "too_long" });
    assert.equal(await findExistingGameRoot(deep), null);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
