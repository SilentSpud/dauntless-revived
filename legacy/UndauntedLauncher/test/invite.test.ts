import { test } from "node:test";
import assert from "node:assert/strict";
import { formatInvite, isPrivateModeHost, isTailscaleIPv4, isValidFingerprint, isValidHost, parseInvite, type Invite } from "../src/shared/invite";
import { readFileSync } from "node:fs";
import path from "node:path";

const FP = "3f".repeat(32);
const good = "dauntless-revived://join?v=1&host=100.101.102.103&port=61000&code=ABCD-1234&name=Alex%27s%20Ramsgate";
const goodV2 = `dauntless-revived://join?v=2&mode=public&host=203.0.113.10&port=443&fp=${FP}&code=ABCD-EFGH-JKLM&name=Friday%20Hunts`;

// ------------------------------------------------------------------ v1 (private, Tailscale)

test("v1: parses a valid invite", () => {
  const r = parseInvite(good);
  assert.equal(r.ok, true);
  if (!r.ok) return;
  assert.deepEqual(r.invite, { mode: "private", host: "100.101.102.103", port: 61000, code: "ABCD-1234", name: "Alex's Ramsgate", share: null, fp: null });
});

test("v1: accepts surrounding whitespace, a MagicDNS name, a share link and an upper-case scheme", () => {
  const share = encodeURIComponent("https://login.tailscale.com/admin/invite/abcDEF123");
  const r = parseInvite(`  DAUNTLESS-REVIVED://join/?v=1&host=HostPC.tail1234.ts.net&port=61000&code=abcd&name=Friends+Night&share=${share}\n`);
  assert.equal(r.ok, true);
  if (!r.ok) return;
  assert.equal(r.invite.mode, "private");
  assert.equal(r.invite.host, "hostpc.tail1234.ts.net");
  assert.equal(r.invite.name, "Friends Night");
  assert.equal(r.invite.share, "https://login.tailscale.com/admin/invite/abcDEF123");
});

test("v1: round-trips through formatInvite", () => {
  const inv: Invite = { mode: "private", host: "100.64.0.1", port: 61000, code: "Code-42", name: "Ember & Aether", share: "https://login.tailscale.com/x", fp: null };
  const r = parseInvite(formatInvite(inv));
  assert.equal(r.ok, true);
  if (r.ok) assert.deepEqual(r.invite, inv);
});

// ------------------------------------------------------------------ v2 (public, TLS gateway)

test("v2: parses a valid public invite", () => {
  const r = parseInvite(goodV2);
  assert.equal(r.ok, true);
  if (!r.ok) return;
  assert.deepEqual(r.invite, { mode: "public", host: "203.0.113.10", port: 443, code: "ABCD-EFGH-JKLM", name: "Friday Hunts", share: null, fp: FP });
});

test("v2: the port defaults to 443, a DNS name and any parameter order work", () => {
  const r = parseInvite(`dauntless-revived://join?name=Hunts&code=abcd&fp=${FP}&host=Play.Example.ORG&mode=public&v=2`);
  assert.equal(r.ok, true);
  if (!r.ok) return;
  assert.equal(r.invite.port, 443);
  assert.equal(r.invite.host, "play.example.org");
  assert.equal(r.invite.mode, "public");
});

test("v2: a custom gateway port", () => {
  const r = parseInvite(`dauntless-revived://join?v=2&mode=public&host=198.51.100.7&port=8443&fp=${FP}&code=abcd&name=x`);
  assert.equal(r.ok, true);
  if (r.ok) assert.equal(r.invite.port, 8443);
});

test("v2: round-trips through formatInvite", () => {
  const inv: Invite = { mode: "public", host: "play.example.org", port: 8443, code: "XXXX-YYYY", name: "Aether & Ember", share: null, fp: FP };
  const r = parseInvite(formatInvite(inv));
  assert.equal(r.ok, true);
  if (r.ok) assert.deepEqual(r.invite, inv);
  assert.throws(() => formatInvite({ ...inv, fp: null }));
});

const v2 = (params: string) => `dauntless-revived://join?${params}`;
const base2 = { v: "2", mode: "public", host: "203.0.113.10", port: "443", fp: FP, code: "abcd", name: "x" };
const build2 = (over: Record<string, string | null>) =>
  v2(
    Object.entries({ ...base2, ...over })
      .filter(([, v]) => v !== null)
      .map(([k, v]) => `${k}=${v}`)
      .join("&"),
  );

// ------------------------------------------------------------------ the shared table

// test/cases/invite.json: the cases the Rust parser (src-tauri/src/core/invite.rs) runs too (L11)
const cases = JSON.parse(readFileSync(path.join(__dirname, "../../test/cases/invite.json"), "utf8")) as {
  accept: { input: string; invite: Invite }[];
  reject: { input: string; error: string }[];
};

for (const { input, invite } of cases.accept) {
  test(`accepts: ${input.slice(0, 100)}`, () => {
    assert.deepEqual(parseInvite(input), { ok: true, invite });
  });
}

for (const { input, error } of cases.reject) {
  test(`rejects (${error}): ${input.slice(0, 100)}`, () => {
    const r = parseInvite(input);
    assert.equal(r.ok, false);
    if (!r.ok) assert.equal(r.error, error);
  });
}

test("rejects non-strings", () => {
  for (const v of [undefined, null, 42, {}, ["dauntless-revived://join?v=1"]]) {
    assert.equal(parseInvite(v).ok, false);
  }
});

test("v1: loopback is allowed (the host's own PC, tests)", () => {
  for (const host of ["127.0.0.1", "localhost"]) {
    const r = parseInvite(`dauntless-revived://join?v=1&host=${host}&port=61000&code=abcd&name=x`);
    assert.equal(r.ok, true, host);
  }
});

test("v2 still takes any public address or DNS name", () => {
  for (const host of ["203.0.113.9", "evil.example.com", "hostpc.tail1234.ts.net"]) {
    assert.equal(parseInvite(build2({ host })).ok, true, host);
  }
});

test("private-mode host rule", () => {
  assert.equal(isPrivateModeHost("100.64.0.1"), true);
  assert.equal(isPrivateModeHost("my-pc.tail-scale.ts.net"), true);
  assert.equal(isPrivateModeHost("127.0.0.1"), true);
  assert.equal(isPrivateModeHost("localhost"), true);
  assert.equal(isPrivateModeHost("203.0.113.9"), false);
  assert.equal(isPrivateModeHost("10.0.0.5"), false);
  assert.equal(isPrivateModeHost("example.com"), false);
  assert.equal(isPrivateModeHost("ts.net"), false);
});

test("host and fingerprint rules", () => {
  assert.equal(isValidHost("100.64.0.1"), true);
  assert.equal(isValidHost("localhost"), true);
  assert.equal(isValidHost("my-pc.tail-scale.ts.net"), true);
  assert.equal(isValidHost("MyPC"), false); // upper case is lowered by the parser, not here
  assert.equal(isValidHost("a_b"), false);
  assert.equal(isTailscaleIPv4("100.64.0.1"), true);
  assert.equal(isTailscaleIPv4("100.127.255.255"), true);
  assert.equal(isTailscaleIPv4("100.128.0.1"), false);
  assert.equal(isTailscaleIPv4("192.168.1.1"), false);
  assert.equal(isValidFingerprint(FP), true);
  assert.equal(isValidFingerprint(FP.toUpperCase()), false);
  assert.equal(isValidFingerprint(""), false);
  assert.equal(isValidFingerprint(null), false);
});
