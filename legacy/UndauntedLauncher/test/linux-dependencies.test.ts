import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { linuxDependencyInstallPlan } from "../src/main/linux-dependencies";

function executable(file: string): string {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, "#!/bin/sh\nexit 0\n");
  chmodSync(file, 0o755);
  return file;
}

test("Linux dependencies: apt runtime install is fixed and elevated", { skip: process.platform !== "linux" }, () => {
  const root = mkdtempSync(path.join(tmpdir(), "dr-linux-deps-"));
  try {
    const apt = executable(path.join(root, "apt-get"));
    const pkexec = executable(path.join(root, "pkexec"));
    const plan = linuxDependencyInstallPlan("runtime", { PATH: root, HOME: root });
    assert.equal(plan?.command, pkexec);
    assert.deepEqual(plan?.args, [apt, "install", "-y", "wine64"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Linux dependencies: secure storage installs keyring and libsecret", { skip: process.platform !== "linux" }, () => {
  const root = mkdtempSync(path.join(tmpdir(), "dr-linux-deps-"));
  try {
    const apt = executable(path.join(root, "apt-get"));
    const pkexec = executable(path.join(root, "pkexec"));
    const plan = linuxDependencyInstallPlan("secure-storage", { PATH: root, HOME: root });
    assert.equal(plan?.command, pkexec);
    assert.deepEqual(plan?.args, [apt, "install", "-y", "gnome-keyring", "libsecret-1-0"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
