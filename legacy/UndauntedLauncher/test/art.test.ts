// The one helper for the two art URL forms (src/shared/art.ts): what platform/art.rs answers.

import { test } from "node:test";
import assert from "node:assert/strict";
import { artSource } from "../src/shared/art";

const ID = "a".repeat(64);

test("art loads from dr-art:// as named, or from dr-art.localhost on WebView2", () => {
  assert.equal(artSource(`dr-art://bg/${ID}`, false), `dr-art://bg/${ID}`);
  assert.equal(artSource(`dr-art://bg/${ID}`, true), `http://dr-art.localhost/bg/${ID}`);
});

test("nothing that is not the launcher's own art is loaded", () => {
  for (const url of [
    `dr-art://bg/${ID.toUpperCase()}`,
    `dr-art://bg/${ID}0`,
    `dr-art://other/${ID}`,
    `http://dr-art.localhost/bg/${ID}`,
    `https://example.com/bg/${ID}`,
    `dr-art://bg/../${ID}`,
    "",
  ]) {
    assert.equal(artSource(url, false), null, url);
    assert.equal(artSource(url, true), null, url);
  }
});
