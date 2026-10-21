// Builds docs/llms.txt and docs/llms-full.txt (the llms.txt convention, https://llmstxt.org/) from the docs pages.
//
//   llms.txt       An index: a short summary, then every English page as "- [title](url): description",
//                  grouped like the site navigation, plus the Finnish pages under "Suomeksi".
//   llms-full.txt  The Markdown of every English page in navigation order, each headed by its URL, with
//                  front matter, Liquid and kramdown attribute lists removed and every link made absolute.
//
// Page titles and notes come from each page's front matter (title, description), so rerun this after
// adding a page or changing a description. Neither output file has front matter, so Jekyll copies both
// to the site byte for byte (served at /dauntless-revived/llms.txt and /dauntless-revived/llms-full.txt).
//
// Usage (from the repository root): bun tools/build-llms.ts
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const root = path.join(import.meta.dir, "..");
const docs = path.join(root, "docs");
const SITE = "https://mixutin.github.io/dauntless-revived";
const REPO = "https://github.com/mixutin/dauntless-revived";
const UPSTREAM = "https://github.com/SyST3MDeV/Undaunted";

const SUMMARY =
    "Dauntless Revived is a private, non-commercial server revival of Dauntless, the Phoenix Labs co-op " +
    "monster-hunting game whose official servers shut down on 30 May 2025. It runs the genuine Dauntless 1.4.4 " +
    "client (October 2020, Unreal Engine 4) against a self-hosted fork of the open-source Undaunted server by " +
    "gwog and contributors (AGPL-3.0).";

const FACTS = [
    "It is not a public server. It is run for a few friends, and anyone with their own copy of the 1.4.4 client " +
        `can host the same thing from the source code (${REPO}).`,
    "Status as of 22 September 2026: the owner played it first on the host PC and then over the internet " +
        "on a rented server, from the invite and the game download to the first hunt. For the owner, login with " +
        "a personal account key (no Epic account needed), the tutorial, Ramsgate, the Training Dojo, hunts and " +
        "crafting work. Items, gear and quests are saved and backed up hourly. Slayer level, mastery and the Hunt " +
        "Pass (with the Elite pass for every account) are saved too: real progression is on by default and passed " +
        "an in-game test on a throwaway account. Later that night two players saw each other in Ramsgate and hunted " +
        "together over the internet, after queueing the same hunt. Parties, the friends list and guilds are built on " +
        "the server side; in that test a party invite and a friend request did not show in game, and the fixes " +
        "(running on the server, not yet tried in game) wait for the next two-player test. Not yet working: text chat, online status and " +
        "bounties. The home page and the roadmap have the current state.",
    "No game files are distributed by the site or the repository. Every player needs their own copy of the client.",
    "Not affiliated with, endorsed by or supported by Phoenix Labs or Epic Games. \"Dauntless\" is a trademark " +
        "of its owners.",
    `Built on Undaunted (${UPSTREAM}) by gwog (Gregory Morford) and contributors: the server-mode DLL, the ` +
        "deploy server, the metagame backend and the launcher are theirs.",
];

interface Page {
    path: string;
    body: string;
    title: string | undefined;
    description: string | undefined;
    lang: string;
    ref: string | undefined;
    parent: string | undefined;
    navOrder: number;
    url: string;
}

// ---------------------------------------------------------------------------------------------------------
// Reading pages

function walk(dir: string, rel = ""): string[] {
    const out: string[] = [];
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        if (entry.name.startsWith("_") || entry.name.startsWith(".")) continue;
        const relPath = rel ? `${rel}/${entry.name}` : entry.name;
        if (entry.isDirectory()) out.push(...walk(path.join(dir, entry.name), relPath));
        else if (entry.name.endsWith(".md")) out.push(relPath);
    }
    return out;
}

function parseScalar(raw: string): string {
    const v = raw.trim();
    if (v.startsWith('"')) return JSON.parse(v) as string;
    if (v.startsWith("'")) return v.slice(1, -1).replace(/''/g, "'");
    return v;
}

function readPage(relPath: string): Page | null {
    const src = readFileSync(path.join(docs, relPath), "utf8").replace(/\r\n/g, "\n");
    const m = src.match(/^---\n([\s\S]*?)\n---\n/);
    if (!m) return null; // not a Jekyll page
    const fm: Record<string, string> = {};
    for (const line of m[1]!.split("\n")) {
        const kv = line.match(/^([A-Za-z_][\w-]*):\s*(.*)$/);
        if (kv && kv[2] !== "") fm[kv[1]!] = parseScalar(kv[2]!);
    }
    let url = fm.permalink;
    if (!url) {
        url = "/" + relPath.replace(/(^|\/)index\.md$/, "$1").replace(/\.md$/, ".html");
    }
    return {
        path: relPath,
        body: src.slice(m[0].length),
        title: fm.title,
        description: fm.description,
        lang: fm.lang || "en",
        ref: fm.ref,
        parent: fm.parent,
        navOrder: fm.nav_order === undefined ? 999 : Number(fm.nav_order),
        url: SITE + url,
    };
}

const pages = walk(docs).map(readPage).filter((p): p is Page => p !== null);
const byPath = new Map(pages.map(p => [p.path, p]));
const english = pages.filter(p => p.lang === "en");
const finnish = pages.filter(p => p.lang === "fi");

// Navigation order: top-level pages by nav_order, each followed by its children.
function navSort(list: Page[]): Page[] {
    const cmp = (a: Page, b: Page) => a.navOrder - b.navOrder || String(a.title).localeCompare(String(b.title));
    const out: Page[] = [];
    const add = (p: Page, depth: number) => {
        out.push(p);
        if (depth > 2) return;
        list.filter(c => c.parent === p.title).sort(cmp).forEach(c => add(c, depth + 1));
    };
    list.filter(p => !p.parent).sort(cmp).forEach(p => add(p, 0));
    for (const p of list) if (!out.includes(p)) out.push(p); // orphans, if any
    return out;
}
const englishOrdered = navSort(english);
const englishIndex = new Map(englishOrdered.map((p, i) => [p.ref, i]));
const finnishOrdered = finnish
    .slice()
    .sort((a, b) => (englishIndex.get(a.ref) ?? 999) - (englishIndex.get(b.ref) ?? 999) || a.path.localeCompare(b.path));

for (const p of pages) {
    if (!p.description) console.warn(`warning: ${p.path} has no description`);
}

// ---------------------------------------------------------------------------------------------------------
// FAQ data (_data/faq_*.yml). A small parser for the fixed format documented at the top of that file.

type FaqItem = Record<string, string>;

function parseFaqYaml(text: string): FaqItem[] {
    const items: FaqItem[] = [];
    let cur: FaqItem | null = null;
    let blockKey: string | null = null;
    let blockLines: string[] = [];
    const fold = (lines: string[]) => {
        while (lines.length && lines[lines.length - 1] === "") lines.pop();
        let out = "";
        let prevText = false;
        for (const l of lines) {
            if (l === "") { out += "\n"; prevText = false; continue; }
            out += (prevText ? " " : "") + l;
            prevText = true;
        }
        return out;
    };
    const flush = () => {
        if (blockKey && cur) cur[blockKey] = fold(blockLines);
        blockKey = null;
        blockLines = [];
    };
    const set = (item: FaqItem, k: string, v: string) => {
        if (v.trim() === ">-") blockKey = k;
        else item[k] = parseScalar(v);
    };
    for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
        if (blockKey) {
            if (line.trim() === "") { blockLines.push(""); continue; }
            if (line.startsWith("    ")) { blockLines.push(line.slice(4).trimEnd()); continue; }
            flush();
        }
        if (line.trim() === "" || /^\s*#/.test(line)) continue;
        let m: RegExpMatchArray | null;
        if ((m = line.match(/^- (\w+):\s*(.*)$/))) { cur = {}; items.push(cur); set(cur, m[1]!, m[2]!); }
        else if (cur && (m = line.match(/^  (\w+):\s*(.*)$/))) set(cur, m[1]!, m[2]!);
        else throw new Error(`FAQ data: cannot parse line: ${line}`);
    }
    flush();
    for (const it of items) {
        if (!it.id || !it.q || !it.a) throw new Error(`FAQ data: item without id, q or a: ${JSON.stringify(it)}`);
    }
    return items;
}

// [text](setup/host.md#anchor) -> [text](https://.../setup/host.html#anchor), like _includes/faq_resolve.html.
function resolveSourceLinks(md: string, where: string): string {
    return md.replace(/\]\(([\w./-]+\.md)(#[^)\s]*)?\)/g, (all, p: string, anchor: string | undefined) => {
        const target = byPath.get(p);
        if (!target) throw new Error(`${where}: link to missing page ${p}`);
        return `](${target.url}${anchor || ""})`;
    });
}

function faqMarkdown(lang: string): string {
    const file = path.join(docs, "_data", `faq_${lang}.yml`);
    return parseFaqYaml(readFileSync(file, "utf8"))
        .map(it => `## ${it.q}\n\n${resolveSourceLinks(it.a!, `faq_${lang}.yml#${it.id}`)}\n`)
        .join("\n");
}

// ---------------------------------------------------------------------------------------------------------
// Markdown for llms-full.txt

function plainMarkdown(page: Page): string {
    const vars = new Map<string, string>();
    const pageUrl = (v: string) => {
        const target = byPath.get(vars.get(v) ?? "");
        if (!target) throw new Error(`${page.path}: Liquid variable ${v} does not name a known page`);
        return target.url;
    };

    // Liquid outside {% raw %} blocks; raw blocks are kept literally. The page-variable assigns sit at the
    // top of each page, before any use, so one pass in document order is enough.
    const segments = page.body.split(/(\{%-?\s*raw\s*-?%\}[\s\S]*?\{%-?\s*endraw\s*-?%\})/);
    const text = segments
        .map((seg, i) => {
            if (i % 2 === 1) return seg.replace(/^\{%-?\s*raw\s*-?%\}/, "").replace(/\{%-?\s*endraw\s*-?%\}$/, "");
            const out = seg
                .replace(/\{%-?\s*comment\s*-?%\}[\s\S]*?\{%-?\s*endcomment\s*-?%\}\n?/g, "")
                .replace(/\{%-?\s*for item in site\.data\.faq_(\w+)\s*-?%\}[\s\S]*?\{%-?\s*endfor\s*-?%\}\n?/g,
                    (all, lang: string) => faqMarkdown(lang))
                .replace(/\{%-?\s*assign\s+(\w+)\s*=\s*site\.pages\s*\|\s*where:\s*"path",\s*"([^"]+)"\s*\|\s*first\s*-?%\}\n?/g,
                    (all, name: string, p: string) => { vars.set(name, p); return ""; })
                .replace(/\{\{\s*(\w+)\.url\s*\|\s*relative_url\s*\}\}/g, (all, v: string) => pageUrl(v))
                .replace(/\{\{\s*['"]([^'"]*)['"]\s*\|\s*relative_url\s*\}\}/g, (all, p: string) => SITE + p)
                .replace(/\{\{\s*site\.github\.repository_url\s*\}\}/g, REPO);
            const leftover = out.match(/\{\{[^}]*\}\}|\{%[^%]*%\}/g);
            if (leftover) throw new Error(`${page.path}: unsupported Liquid: ${leftover.slice(0, 3).join(" ")}`);
            return out;
        })
        .join("");

    return text
        // just-the-docs table-of-contents blocks
        .replace(/<details[^>]*>\s*<summary>[\s\S]*?<\/summary>\s*\{:[^}\n]*\}\s*(?:1\.|-) TOC\s*\{:toc\}\s*<\/details>\n?/g, "")
        // kramdown attribute lists: on their own line, after a link, and explicit heading ids
        .replace(/^[ \t]*\{:[^}\n]*\}[ \t]*\n/gm, "")
        .replace(/\)\{:[^}\n]*\}/g, ")")
        .replace(/^(#{1,6} .*?)[ \t]*\{#[\w-]+\}[ \t]*$/gm, "$1")
        // same-page anchors become absolute
        .replace(/\]\(#/g, `](${page.url}#`)
        .replace(/\n{3,}/g, "\n\n")
        .trim();
}

// ---------------------------------------------------------------------------------------------------------
// Output

const link = (p: Page, label?: string) => `- [${label || p.title}](${p.url}): ${p.description || ""}`.trimEnd();
const englishByRef = new Map(english.map(p => [p.ref, p]));
const pick = (ref: string) => englishByRef.get(ref);
const setupPages = englishOrdered.filter(p => p.ref === "setup/index" || p.parent === "Setup");
const referencePages = englishOrdered.filter(p => p.ref === "reference/index" || p.parent === "Reference");
const findingsPages = englishOrdered.filter(p => p.ref === "findings/index" || p.parent === "Findings");
const listed = new Set([
    "index", "faq",
    ...setupPages.map(p => p.ref), ...referencePages.map(p => p.ref), ...findingsPages.map(p => p.ref),
]);
const projectPages = englishOrdered.filter(p => !listed.has(p.ref));
const home = pick("index"), faq = pick("faq");

const llms = [
    "# Dauntless Revived",
    "",
    `> ${SUMMARY}`,
    "",
    ...FACTS.map(f => `- ${f}`),
    "",
    "## Start here",
    "",
    home && link(home, "Home"),
    faq && link(faq, "FAQ"),
    "",
    "## Setup",
    "",
    ...setupPages.map(p => link(p)),
    "",
    "## Reference",
    "",
    ...referencePages.map(p => link(p)),
    "",
    "## Findings",
    "",
    ...findingsPages.map(p => link(p)),
    "",
    "## Project",
    "",
    ...projectPages.map(p => link(p)),
    `- [Source code on GitHub](${REPO}): The fork's complete source, AGPL-3.0-only, on branch dauntless-revived.`,
    `- [ROADMAP.md](https://raw.githubusercontent.com/mixutin/dauntless-revived/dauntless-revived/ROADMAP.md): ` +
        "The live checklist behind the roadmap page, as raw Markdown. Done items start with \"- [x] **<id>\", open ones with \"- [ ] **<id>\".",
    "",
    // Every line under an H2 must be a link entry (llmstxt.org), so the section's language is in its name.
    "## Suomeksi (the Finnish version of the site, in plain language)",
    "",
    ...finnishOrdered.map(p => link(p)),
    "",
    "## Optional",
    "",
    `- [Full text of the English pages](${SITE}/llms-full.txt): Every English page above as Markdown in one file, each headed by its URL.`,
    `- [Undaunted](${UPSTREAM}): The upstream open-source project by gwog (Gregory Morford) and contributors that this fork is based on.`,
    "- [Mystic Paradox](https://github.com/pranav158/Mystic-Paradox): A related project porting the same approach to the Dauntless 1.12.0 client.",
    "",
]
    .filter((l): l is string => typeof l === "string")
    .join("\n");

const rule = "=".repeat(80);
const full = [
    "# Dauntless Revived: the English documentation in one file",
    "",
    `> ${SUMMARY}`,
    "",
    `The pages of ${SITE}/ in navigation order, as Markdown. Each page starts with its URL. ` +
        `The index of this file is ${SITE}/llms.txt. The Finnish translations are listed there, under "Suomeksi".`,
    "",
    ...englishOrdered.flatMap(p => [rule, `URL: ${p.url}`, rule, "", plainMarkdown(p), ""]),
].join("\n");

await Bun.write(path.join(docs, "llms.txt"), llms);
await Bun.write(path.join(docs, "llms-full.txt"), full);
console.log(
    `docs/llms.txt: ${englishOrdered.length} English + ${finnishOrdered.length} Finnish pages, ${llms.length} bytes\n` +
    `docs/llms-full.txt: ${englishOrdered.length} pages, ${Buffer.byteLength(full)} bytes`
);
