import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { DefaultManifestPath, LoadManifest, OnDiskPath } from "./manifest";

// npm run verify [-- --quick] [-- --game-dir <folder>] [-- --manifest <file>]
//
// Hashes every file of the game folder (CONTENT_GAME_DIR unless --game-dir) against the manifest and
// prints what is missing, the wrong size or the wrong hash. --quick compares sizes only. Read-only;
// runs at low CPU priority. Exit code 0 only if every file matches. Run it after copying the game onto
// a new host, and before restarting the content server after anything touched the game folder.

type Args = { gameDir?: string; manifest?: string; quick: boolean };

function ParseArgs(Argv: string[]): Args {
    const Result: Args = { quick: false };
    for(let I = 0; I < Argv.length; I++){
        const A = Argv[I];
        if(A === "--quick"){
            Result.quick = true;
        }
        else if(A === "--game-dir" && I + 1 < Argv.length){
            Result.gameDir = Argv[++I];
        }
        else if(A === "--manifest" && I + 1 < Argv.length){
            Result.manifest = Argv[++I];
        }
        else{
            console.error(`Unknown argument: ${A}`);
            process.exit(2);
        }
    }
    return Result;
}

function HashFile(File: string): Promise<string> {
    return new Promise((resolve, reject) => {
        const Hash = createHash("sha256");
        fs.createReadStream(File, { highWaterMark: 4 * 1024 * 1024 })
            .on("data", (Chunk) => Hash.update(Chunk))
            .on("error", reject)
            .on("end", () => resolve(Hash.digest("hex")));
    });
}

function ListFiles(Dir: string, Prefix = ""): string[] {
    const Out: string[] = [];
    for(const Entry of fs.readdirSync(Dir, { withFileTypes: true })){
        const Rel = Prefix === "" ? Entry.name : `${Prefix}/${Entry.name}`;
        if(Entry.isDirectory()){
            Out.push(...ListFiles(path.join(Dir, Entry.name), Rel));
        }
        else{
            Out.push(Rel);
        }
    }
    return Out;
}

async function Main(){
    try{
        os.setPriority(os.constants.priority.PRIORITY_LOW);
    }
    catch{
        // not fatal
    }

    const Args = ParseArgs(process.argv.slice(2));
    const GameDirRaw = Args.gameDir ?? process.env.CONTENT_GAME_DIR;
    if(GameDirRaw === undefined || GameDirRaw.trim() === ""){
        console.error("Set CONTENT_GAME_DIR in .env or pass --game-dir <folder that contains Archon\\>");
        process.exit(2);
    }
    const GameDir = path.resolve(GameDirRaw);
    const ManifestPath = path.resolve(Args.manifest ?? process.env.CONTENT_MANIFEST ?? DefaultManifestPath());
    const Manifest = LoadManifest(ManifestPath);

    console.log(`Manifest:  ${ManifestPath}`);
    console.log(`Build:     ${Manifest.build}`);
    console.log(`Game dir:  ${GameDir}`);
    console.log(`Mode:      ${Args.quick ? "sizes only (--quick)" : "full SHA-256 of every file"}`);
    console.log(`Files:     ${Manifest.files.length}, ${Manifest.totalBytes} bytes\n`);

    const Problems: string[] = [];
    let Ok = 0;
    let DoneBytes = 0;
    const T0 = Date.now();
    let LastReport = T0;

    for(const File of Manifest.files){
        const OnDisk = OnDiskPath(GameDir, File);
        let Stat: fs.Stats;
        try{
            Stat = fs.lstatSync(OnDisk);
        }
        catch{
            Problems.push(`MISSING   ${File.path}`);
            continue;
        }
        if(!Stat.isFile()){
            Problems.push(`NOT FILE  ${File.path}`);
            continue;
        }
        if(Stat.size !== File.size){
            Problems.push(`SIZE      ${File.path} (${Stat.size} bytes, expected ${File.size})`);
            continue;
        }
        if(!Args.quick){
            const Sha = await HashFile(OnDisk);
            if(Sha !== File.sha256){
                Problems.push(`HASH      ${File.path}`);
                continue;
            }
        }
        Ok++;
        DoneBytes += File.size;
        if(Date.now() - LastReport > 5000){
            LastReport = Date.now();
            console.log(`  ${Ok + Problems.length}/${Manifest.files.length} files, ${((DoneBytes / Manifest.totalBytes) * 100).toFixed(1)}%, ${((Date.now() - T0) / 1000).toFixed(0)} s`);
        }
    }

    // Extra files are fine (the two DLLs you install, logs), but worth seeing.
    const Known = new Set(Manifest.files.map((File) => File.path.toLowerCase()));
    let Extras: string[] = [];
    try{
        Extras = ListFiles(GameDir).filter((Rel) => !Known.has(Rel.toLowerCase()));
    }
    catch{
        // the folder itself is missing; every file is already reported
    }

    for(const Problem of Problems){
        console.log(Problem);
    }
    console.log(`\nok: ${Ok}/${Manifest.files.length}  problems: ${Problems.length}  (${((Date.now() - T0) / 1000).toFixed(1)} s)`);
    if(Extras.length > 0){
        console.log(`Not in the manifest (never served): ${Extras.length} file(s): ${Extras.slice(0, 10).join(", ")}${Extras.length > 10 ? ", ..." : ""}`);
    }
    process.exit(Problems.length === 0 ? 0 : 1);
}

Main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(2);
});
