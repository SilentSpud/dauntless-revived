// The launcher's look follows the brand (brand/README.md): the eight palette colours from
// brand/palette.json, defined once in styles.css and used everywhere else only through those tokens;
// every text and control colour pair the stylesheet uses passes WCAG AA; the icons and in-app images
// are current copies of the files brand/build.py makes; and the built-in background is drawn by the
// launcher itself, with nothing loaded from outside.

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const ROOT = path.resolve(__dirname, "..", "..");
const BRAND = path.resolve(ROOT, "..", "..", "brand");
const CSS_FILE = path.join(ROOT, "src", "renderer", "styles.css");
const CSS = readFileSync(CSS_FILE, "utf8").replace(/\r\n/g, "\n");
const HTML = readFileSync(path.join(ROOT, "index.html"), "utf8");

type Token = "ink" | "navy" | "deep" | "azure" | "ice" | "glow" | "steel" | "frost";
const PALETTE = JSON.parse(readFileSync(path.join(BRAND, "palette.json"), "utf8")) as { colors: Record<Token, { hex: string }> };
const HEX = Object.fromEntries(Object.entries(PALETTE.colors).map(([k, v]) => [k, v.hex.toUpperCase()])) as Record<Token, string>;

function luminance(hex: string): number {
  const n = parseInt(hex.slice(1), 16);
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// The CSS without comments, and its declarations outside the token block.
const CODE = CSS.replace(/\/\*[\s\S]*?\*\//g, "");

test("styles.css defines the eight palette tokens once, with the values in brand/palette.json", () => {
  const defined = [...CODE.matchAll(/--dr-([a-z]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim().toUpperCase()]);
  assert.deepEqual(
    defined.map(([name]) => name).sort(),
    Object.keys(HEX).sort(),
    "each of the eight tokens is defined exactly once, and nothing else is named --dr-*",
  );
  for (const [name, value] of defined) assert.equal(value, HEX[name as Token], `--dr-${name}`);
});

// Every CSS named colour except transparent and currentColor.
const NAMED = (
  "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue " +
  "chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey " +
  "darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray " +
  "darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen " +
  "fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender " +
  "lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey " +
  "lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen " +
  "magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen " +
  "mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange " +
  "orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple " +
  "rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray " +
  "slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen " +
  "canvas canvastext linktext visitedtext activetext buttonface buttontext buttonborder field fieldtext highlight highlighttext " +
  "selecteditem selecteditemtext mark marktext graytext accentcolor accentcolortext"
).split(" ");

function colourProblem(value: string): string | null {
  if (/#[0-9a-f]{3,8}\b/i.test(value)) return "a hex colour";
  const fn = /\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i.exec(value);
  if (fn) return `${fn[1]}()`;
  // Words only, outside var(...) names and url(#id) references.
  const words = value.replace(/var\(--[a-z0-9-]+\)/gi, " ").replace(/url\([^)]*\)/gi, " ").toLowerCase().match(/[a-z]+/g) ?? [];
  const named = words.find((w) => NAMED.includes(w));
  return named ? `the named colour ${named}` : null;
}

test("styles.css uses colours only through the tokens", () => {
  let declarations = 0;
  for (const m of CODE.matchAll(/(^|[;{])\s*(-?-?[a-z][a-z0-9-]*)\s*:\s*([^;{}]+)(?=;|})/gm)) {
    const [, , prop, value] = m;
    if (/^--dr-/.test(prop)) continue;
    // Animation and font names are names, not colours.
    if (/^(animation|animation-name|font|font-family|transition|transition-property|grid-template-areas|content|counter-reset|counter-increment)$/.test(prop)) continue;
    declarations++;
    const problem = colourProblem(value);
    assert.equal(problem, null, `${prop}: ${value.trim()} uses ${problem}; use a --dr-* token`);
  }
  assert.ok(declarations > 400, `only ${declarations} declarations found; the scan is broken`);
});

test("the page and the renderer code carry no colours of their own", () => {
  const files = [path.join(ROOT, "index.html"), ...readdirSync(path.join(ROOT, "src", "renderer")).filter((f) => f.endsWith(".ts")).map((f) => path.join(ROOT, "src", "renderer", f))];
  for (const file of files) {
    const text = readFileSync(file, "utf8").replace(/\/\/.*$/gm, "");
    assert.doesNotMatch(text, /#[0-9a-f]{3,8}\b(?![-\w])/i, `${path.basename(file)} has a hex colour`);
    assert.doesNotMatch(text, /\b(rgba?|hsla?)\(/i, `${path.basename(file)} has a colour function`);
    assert.doesNotMatch(text, /\b(fill|stroke|stop-color|flood-color|color)="(?!url\()/i, `${path.basename(file)} sets a colour attribute`);
  }
  // The window's background before the page paints is the page's own ink.
  const main = readFileSync(path.join(ROOT, "src", "main.ts"), "utf8");
  assert.match(main, new RegExp(`backgroundColor: "${HEX.ink}"`, "i"));
});

// Every text and control colour pair styles.css uses, with the AA minimum it needs: 4.5 for text,
// 3 for large text (the 23 px bold big button), control edges, focus rings and meaningful graphics.
const PAIRS: [Token, Token, number, string][] = [
  ["frost", "ink", 4.5, "text on the page, danger button, input text"],
  ["frost", "navy", 4.5, "text on cards, the rail, the panel and dialogs"],
  ["frost", "deep", 4.5, "current page, pressed language, avatars and chips on deep"],
  ["steel", "ink", 4.5, "muted text on the page, stat labels, placeholders"],
  ["steel", "navy", 4.5, "muted text, hints, disabled buttons and links"],
  ["steel", "deep", 4.5, "empty avatar, disabled big button"],
  ["ice", "ink", 4.5, "eyebrow, private-server badge, fingerprints on ink"],
  ["ice", "navy", 4.5, "links and secondary button labels on cards"],
  ["ice", "deep", 3, "city icons and the progress fill against its track"],
  ["glow", "ink", 4.5, "public badge, highlights"],
  ["glow", "navy", 4.5, "running button, problem hints, progress percentage"],
  ["glow", "deep", 4.5, "hovered buttons, step numbers, current-page icon"],
  ["ink", "ice", 4.5, "primary buttons, current step, avatar initials, switch knob"],
  ["ink", "glow", 4.5, "hovered primary buttons"],
  ["frost", "azure", 4.5, "pressed buttons, selection, hovered close button"],
  ["glow", "ink", 3, "focus ring on the page"],
  ["glow", "navy", 3, "focus ring on surfaces, input focus edge, online dot"],
  ["glow", "deep", 3, "focus ring on raised surfaces"],
  ["steel", "navy", 3, "button, input and switch edges on navy"],
  ["steel", "ink", 3, "input edge against its ink well"],
  ["steel", "deep", 3, "paused progress fill against its track"],
  ["azure", "ink", 3, "disabled big button edge against the action bar"],
  ["ice", "navy", 3, "switch track when on, update banner edge"],
  ["frost", "navy", 3, "danger button and invalid input edges"],
];

test("every colour pair the launcher uses passes WCAG AA", () => {
  for (const [fg, bg, min, use] of PAIRS) {
    const ratio = contrast(HEX[fg], HEX[bg]);
    assert.ok(ratio >= min, `${fg} on ${bg} is ${ratio.toFixed(2)}:1, below ${min}:1 (${use})`);
  }
});

test("rules that set both a text colour and a fill pass AA", () => {
  let checked = 0;
  for (const m of CODE.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const [, selector, body] = m;
    const fg = /(?:^|[;\s])color:\s*var\(--dr-([a-z]+)\)\s*;/.exec(body);
    const bg = /(?:^|[;\s])background(?:-color)?:\s*var\(--dr-([a-z]+)\)\s*;/.exec(body);
    if (!fg || !bg) continue;
    checked++;
    // The big button's label is large text (23 px bold); everything else is normal text.
    const min = selector.includes(".big-button") ? 3 : 4.5;
    const ratio = contrast(HEX[fg[1] as Token], HEX[bg[1] as Token]);
    assert.ok(ratio >= min, `${selector.trim()}: ${fg[1]} on ${bg[1]} is ${ratio.toFixed(2)}:1`);
  }
  assert.ok(checked >= 20, `only ${checked} rules found; the scan is broken`);
});

test("the icons and in-app images are current copies of brand/", () => {
  const r = spawnSync(process.execPath, [path.join(ROOT, "scripts", "make-icon.mjs"), "--check"], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr || r.stdout);
  // What forge builds with: the exe and Setup.exe icon, and the window icon shipped next to the app.
  const forge = readFileSync(path.join(ROOT, "forge.config.ts"), "utf8");
  assert.match(forge, /icon: "assets\/icon"/);
  assert.match(forge, /setupIcon: "assets\/icon\.ico"/);
  assert.match(forge, /const launcherResources\s*=\s*\[[^\]]*"assets\/icon\.png"/);
  assert.match(forge, /extraResource:\s*launcherResources/);
});

test("the page's images are local brand files, and the logo has a translated text alternative", () => {
  const sources = [...HTML.matchAll(/\b(?:src|href|srcset)="([^"]+)"/g)].flatMap((m) => m[1].split(",").map((s) => s.trim().split(/\s+/)[0]));
  const images = sources.filter((s) => /\.(png|jpe?g|webp|svg|gif)$/i.test(s));
  assert.ok(images.length >= 6, `only ${images.length} images found; the scan is broken`);
  for (const src of images) assert.match(src, /^\.\/src\/renderer\/brand\/[a-z0-9@-]+\.png$/, `${src} is not a local brand image`);
  assert.doesNotMatch(HTML, /(?:src|href)="(?:https?:)?\/\//i, "nothing is loaded from another host");
  assert.match(HTML, /<img\s[^>]*class="tile-logo"[^>]*data-i18n-alt="logo_alt"/s);
  assert.match(HTML, /<img class="titlebar-emblem"[^>]*alt=""/, "the title bar emblem is decoration next to the title");
});

test("the built-in background stays still for reduced motion, while the game runs and under a host's art", () => {
  assert.match(CODE, /@media \(prefers-reduced-motion: reduce\)\s*\{[\s\S]*\.scene \*\s*\{\s*animation: none !important;/);
  assert.match(CODE, /\.hero\.game-running \.scene \*,\s*\.hero\.has-art \.scene \*\s*\{\s*animation-play-state: paused !important;/);
  // Only compositor-friendly properties are animated in the scene.
  for (const name of ["twinkle", "aurora", "drift", "fall"]) {
    const frames = CODE.slice(CODE.indexOf(`@keyframes ${name} {`));
    const body = frames.slice(0, frames.indexOf("\n}\n"));
    for (const prop of body.matchAll(/^\s*([a-z-]+):/gm)) assert.ok(["transform", "opacity"].includes(prop[1]), `@keyframes ${name} animates ${prop[1]}`);
  }
  const index = readFileSync(path.join(ROOT, "src", "renderer", "index.ts"), "utf8");
  assert.match(index, /classList\.toggle\("game-running", snap\.game\.running\)/);
  assert.match(index, /classList\.add\("has-art"\)/);
});
