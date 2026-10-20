import fs from "node:fs";
import { Manifest, ManifestFile, OnDiskPath } from "./manifest";

// What we know about each manifest file on the host's disk. The startup check only compares sizes
// (a full hash of 10.9 GB takes a while; `npm run verify` does that). A file is served only while it
// is "ok", and at open time its size and modification time must still be what the startup check saw.

export type FileState =
    | { status: "ok"; size: number; mtimeMs: number }
    | { status: "missing" }
    | { status: "not_a_file" }
    | { status: "size_mismatch"; actual: number }
    | { status: "changed" };

export type GameDirReport = {
    states: Map<string, FileState>;
    ok: number;
    missing: string[];
    mismatched: string[];
    bytesOk: number;
};

export function CheckGameDir(GameDir: string, TheManifest: Manifest): GameDirReport {
    const States = new Map<string, FileState>();
    const Missing: string[] = [];
    const Mismatched: string[] = [];
    let Ok = 0;
    let BytesOk = 0;

    for(const File of TheManifest.files){
        const State = StatFile(GameDir, File);
        States.set(File.path, State);

        if(State.status === "ok"){
            Ok++;
            BytesOk += File.size;
        }
        else if(State.status === "missing"){
            Missing.push(File.path);
        }
        else{
            Mismatched.push(File.path);
        }
    }

    return { states: States, ok: Ok, missing: Missing, mismatched: Mismatched, bytesOk: BytesOk };
}

function StatFile(GameDir: string, File: ManifestFile): FileState {
    let Stat: fs.Stats;
    try{
        // lstat: a symlink or junction planted in the game folder is not served.
        Stat = fs.lstatSync(OnDiskPath(GameDir, File));
    }
    catch{
        return { status: "missing" };
    }

    if(!Stat.isFile()){
        return { status: "not_a_file" };
    }

    if(Stat.size !== File.size){
        return { status: "size_mismatch", actual: Stat.size };
    }

    return { status: "ok", size: Stat.size, mtimeMs: Stat.mtimeMs };
}
