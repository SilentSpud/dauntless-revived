import fs from "node:fs";
import path from "node:path";

// The manifest is generated from the verified 1.4.4 zip by tools/make-game-manifest.js and checked in
// as data/dauntless-1.4.4.json. The friend launcher compiles in the same file, so what we serve and
// what it accepts are the same list.

export type ManifestFile = {
    path: string;
    size: number;
    sha256: string;
};

export type Manifest = {
    build: string;
    totalBytes: number;
    files: ManifestFile[];
};

export const EXPECTED_BUILD = "dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827";

// Every manifest path is one or more segments joined by "/". A segment is letters, digits, "_", "-"
// and ".", may not start or end with "." (so no "." or ".." and nothing Windows would silently trim),
// and may not be a Windows device name. Keep in sync with IsSafeManifestPath in tools/make-game-manifest.js.
const SEGMENT = /^[A-Za-z0-9_-](?:[A-Za-z0-9_.-]{0,126}[A-Za-z0-9_-])?$/;
const RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;

export function IsSafeManifestPath(Value: unknown): Value is string {
    if(typeof Value !== "string" || Value.length === 0 || Value.length > 400){
        return false;
    }

    return Value.split("/").every((Segment) => SEGMENT.test(Segment) && !RESERVED.test(Segment));
}

// Throws with a readable reason if the JSON is not a manifest we are willing to serve from.
export function ValidateManifest(Raw: unknown): Manifest {
    if(typeof Raw !== "object" || Raw === null || Array.isArray(Raw)){
        throw new Error("manifest is not an object");
    }

    const Obj = Raw as Record<string, unknown>;

    if(typeof Obj.build !== "string" || Obj.build.length === 0){
        throw new Error("manifest.build missing");
    }

    if(!Array.isArray(Obj.files) || Obj.files.length === 0){
        throw new Error("manifest.files missing or empty");
    }

    const Files: ManifestFile[] = [];
    const Seen = new Set<string>();
    let Total = 0;
    let Previous = "";

    for(const Entry of Obj.files){
        if(typeof Entry !== "object" || Entry === null){
            throw new Error("manifest entry is not an object");
        }

        const { path: FilePath, size: Size, sha256: Sha256 } = Entry as Record<string, unknown>;

        if(!IsSafeManifestPath(FilePath)){
            throw new Error(`unsafe manifest path ${JSON.stringify(FilePath)}`);
        }

        if(typeof Size !== "number" || !Number.isSafeInteger(Size) || Size < 0){
            throw new Error(`bad size for ${FilePath}`);
        }

        if(typeof Sha256 !== "string" || !/^[0-9a-f]{64}$/.test(Sha256)){
            throw new Error(`bad sha256 for ${FilePath}`);
        }

        // Windows paths are case-insensitive: two entries that differ only in case would be one file.
        const Lower = FilePath.toLowerCase();
        if(Seen.has(Lower)){
            throw new Error(`duplicate manifest path ${FilePath}`);
        }
        Seen.add(Lower);

        if(Previous !== "" && !(Previous < FilePath)){
            throw new Error(`manifest is not sorted at ${FilePath}`);
        }
        Previous = FilePath;

        Total += Size;
        Files.push({ path: FilePath, size: Size, sha256: Sha256 });
    }

    if(Obj.totalBytes !== Total){
        throw new Error(`manifest.totalBytes is ${String(Obj.totalBytes)}, the files add up to ${Total}`);
    }

    return { build: Obj.build, totalBytes: Total, files: Files };
}

export function LoadManifest(File: string): Manifest {
    const Text = fs.readFileSync(File, "utf8").replace(/^﻿/, "");
    return ValidateManifest(JSON.parse(Text));
}

// Exact-match lookup. The request path is never touched after this: the file we open is always
// built from the manifest's own path.
export function IndexManifest(TheManifest: Manifest): Map<string, ManifestFile> {
    return new Map(TheManifest.files.map((File) => [File.path, File]));
}

// data/dauntless-1.4.4.json next to dist/ (npm start) or build/src/ (tests).
export function DefaultManifestPath(): string {
    const Candidates = [
        path.resolve(__dirname, "..", "data", "dauntless-1.4.4.json"),
        path.resolve(__dirname, "..", "..", "data", "dauntless-1.4.4.json"),
    ];

    return Candidates.find((Candidate) => fs.existsSync(Candidate)) ?? Candidates[0];
}

// Where a manifest file lives on disk under the game folder.
export function OnDiskPath(GameDir: string, File: ManifestFile): string {
    return path.join(GameDir, ...File.path.split("/"));
}
