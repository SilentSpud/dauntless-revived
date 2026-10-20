import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import ts from "typescript";

// Our product is Dauntless Revived. No text the metagame sends or logs may call it Undaunted:
// every string in src/ is checked. What stays are wire identifiers and file names, which are
// renamed later (the /undaunted/api routes, the x-undaunted-* headers the launcher, gateway and
// upstream's DLL send, the JWT issuer/audience, and import paths). Credits live in the docs.
const SRC = path.join(__dirname, "..", "..", "src");

const WIRE = [
    /\/undaunted\/api\b/gi,
    /\bx-undaunted-(user-api-key|gameserver-apikey)\b/g,
    /^undaunted-metagame$/g,
    /^undaunted\.db$/g // existing Windows backup format; not a product-facing label
];

function NamesProduct(Text: string){
    return /undaunted/i.test(WIRE.reduce((Rest, Pattern) => Rest.replace(Pattern, ""), Text));
}

function SourceFiles(Dir: string): string[] {
    return fs.readdirSync(Dir, { withFileTypes: true }).flatMap((Entry) => {
        const Full = path.join(Dir, Entry.name);

        if(Entry.isDirectory()) return SourceFiles(Full);
        return Entry.name.endsWith(".ts") ? [Full] : [];
    });
}

// Every string literal and template part, outside import and export paths
function Strings(File: string){
    const Source = ts.createSourceFile(File, fs.readFileSync(File, "utf8"), ts.ScriptTarget.Latest, true);
    const Found: { line: number, text: string }[] = [];

    const Visit = (Node: ts.Node) => {
        if(ts.isImportDeclaration(Node) || ts.isExportDeclaration(Node)) return;

        if(ts.isStringLiteralLike(Node) || ts.isTemplateHead(Node) || ts.isTemplateMiddle(Node) || ts.isTemplateTail(Node)){
            Found.push({ line: Source.getLineAndCharacterOfPosition(Node.getStart()).line + 1, text: Node.text });
        }

        ts.forEachChild(Node, Visit);
    };

    Visit(Source);
    return Found;
}

describe("branding", () => {
    it("no string in src/ calls the product Undaunted, apart from wire identifiers", () => {
        const Files = SourceFiles(SRC);
        const Offenders: string[] = [];

        assert.ok(Files.length > 20, `expected the metagame's sources under ${SRC}`);

        for(const File of Files){
            for(const { line, text } of Strings(File)){
                if(NamesProduct(text)){
                    Offenders.push(`${path.relative(SRC, File)}:${line}: ${JSON.stringify(text)}`);
                }
            }
        }

        assert.deepEqual(Offenders, []);
    });

    it("would notice the product name coming back", () => {
        const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "branding-probe-"));
        const Probe = path.join(Dir, "probe.ts");

        try{
            fs.writeFileSync(Probe, [
                'import { x } from "./undauntedapi";',
                'const a = "x-undaunted-user-api-key";',
                "const b = `Welcome to Undaunted ${a}!`;",
                ""
            ].join("\n"));
            assert.deepEqual(Strings(Probe).map((Found) => Found.text), ["x-undaunted-user-api-key", "Welcome to Undaunted ", "!"]);
        }
        finally{
            fs.rmSync(Dir, { recursive: true, force: true });
        }

        for(const Text of ["Welcome to Undaunted v0.0.5!", "Undaunted Metagame on ", "UNDAUNTED", "/undaunted/api/x by Undaunted"]){
            assert.equal(NamesProduct(Text), true, Text);
        }
        for(const Text of ["/undaunted/api/ServerStatus", "/UNDAUNTED/API/Register", "x-undaunted-user-api-key", "x-undaunted-gameserver-apikey", "undaunted-metagame", "Welcome to Dauntless Revived!"]){
            assert.equal(NamesProduct(Text), false, Text);
        }
    });
});
