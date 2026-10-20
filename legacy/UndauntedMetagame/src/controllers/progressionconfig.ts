import fs from "node:fs";
import path from "node:path";
import bundledConfig from "../vendor/progression_config.json";

// The progression config (the Hunt Pass seasons and the mastery tracks), loaded in one place: GET
// /progression/config serves it and the rank math (progressionrank.ts) reads it, so the server and the
// client always work from the same tracks. The idea of seasons on disk and ACTIVE_HUNT_PASS comes from
// Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7, controllers/huntpass.ts).
//
// - The bundled vendor/progression_config.json, unless:
// - PROGRESSION_CONFIG_DIR names a folder of .json files, each one path object (a track, the shape of
//   the paths in the bundled file), an array of them, or a whole config ({"payload": {"paths": [...]}}).
//   A path replaces the bundled one with the same progression_id (in its place, so the order the client
//   sees stays the same); a new id is added at the end.
// - ACTIVE_HUNT_PASS (default season09b) is the Hunt Pass an account has before it chooses one (GET
//   /huntpass); it must be a loaded path.
// A bad folder, file or path, or an unknown ACTIVE_HUNT_PASS, stops the metagame at boot with a message
// that names it (CheckProgressionConfig). The folder is read once; a change needs a restart. A running
// season's ranks should not be edited in place: players' stored XP is converted to ranks with the new
// requirements at the next read.

export type RankRequirement = { rank_id: number, xp_required: number };

export type ProgressionPath = {
    progression_id: string,
    premium_gating_entitlement?: string,
    requirements?: RankRequirement[],
    prestige?: { xp_per_level: number } | null,
    free_rewards?: unknown[],
    premium_rewards?: unknown[],
    [Field: string]: unknown
};

export class ProgressionConfigError extends Error {
    constructor(message: string){
        super(message);
        this.name = "ProgressionConfigError";
    }
}

export const DEFAULT_ACTIVE_HUNT_PASS = "season09b";

type LoadedConfig = {
    Key: string,
    Paths: ProgressionPath[],
    ById: Map<string, ProgressionPath>,
    Payload: unknown,
    ActiveHuntPass: string,
    Description: string
};

let Loaded: LoadedConfig | undefined;

function IsWhole(Value: unknown){
    return Number.isSafeInteger(Value) && (Value as number) >= 0;
}

function AssertValidPath(Path: any, Source: string): asserts Path is ProgressionPath {
    if(Path == null || typeof Path !== "object" || Array.isArray(Path)){
        throw new ProgressionConfigError(`${Source}: not a progression path object`);
    }

    const Name = `${Source} (${String(Path.progression_id)})`;

    if(typeof Path.progression_id !== "string" || Path.progression_id.length === 0){
        throw new ProgressionConfigError(`${Source}: a path has no progression_id`);
    }

    if(!Array.isArray(Path.requirements) || Path.requirements.length === 0){
        throw new ProgressionConfigError(`${Name}: no requirements, so no rank could ever be reached`);
    }

    let Previous = -1;

    for(const Requirement of Path.requirements){
        if(!IsWhole(Requirement?.rank_id) || !IsWhole(Requirement?.xp_required)){
            throw new ProgressionConfigError(`${Name}: a requirement is not a {rank_id, xp_required} pair of whole numbers`);
        }

        // The client walks them in array order
        if(Requirement.rank_id <= Previous){
            throw new ProgressionConfigError(`${Name}: requirements are not in rising rank_id order (${Requirement.rank_id} after ${Previous})`);
        }

        Previous = Requirement.rank_id;
    }

    for(const Field of ["free_rewards", "premium_rewards"]){
        if(Path[Field] !== undefined && !Array.isArray(Path[Field])){
            throw new ProgressionConfigError(`${Name}: ${Field} is not an array`);
        }
    }

    if(Path.premium_gating_entitlement !== undefined && typeof Path.premium_gating_entitlement !== "string"){
        throw new ProgressionConfigError(`${Name}: premium_gating_entitlement is not a string`);
    }

    if(Path.prestige != null && (typeof Path.prestige !== "object" || !Number.isSafeInteger(Path.prestige.xp_per_level) || Path.prestige.xp_per_level <= 0)){
        throw new ProgressionConfigError(`${Name}: prestige needs a whole xp_per_level above 0`);
    }
}

function ReadPathsFromFolder(Folder: string): ProgressionPath[] {
    let Stat: fs.Stats;

    try{
        Stat = fs.statSync(Folder);
    }
    catch{
        throw new ProgressionConfigError(`PROGRESSION_CONFIG_DIR ${Folder} does not exist`);
    }

    if(!Stat.isDirectory()){
        throw new ProgressionConfigError(`PROGRESSION_CONFIG_DIR ${Folder} is not a folder`);
    }

    const Files = fs.readdirSync(Folder).filter((Name) => Name.toLowerCase().endsWith(".json")).sort();
    const Paths: ProgressionPath[] = [];
    const Seen = new Map<string, string>();

    if(Files.length === 0){
        throw new ProgressionConfigError(`PROGRESSION_CONFIG_DIR ${Folder} holds no .json file`);
    }

    for(const Name of Files){
        const File = path.join(Folder, Name);
        let Parsed: any;

        try{
            Parsed = JSON.parse(fs.readFileSync(File, "utf8").replace(/^﻿/, ""));
        }
        catch(error){
            throw new ProgressionConfigError(`${File} is not valid JSON: ${(error as Error).message}`);
        }

        const List = Array.isArray(Parsed) ? Parsed : Array.isArray(Parsed?.payload?.paths) ? Parsed.payload.paths : [Parsed];

        for(const Entry of List){
            AssertValidPath(Entry, File);

            if(Seen.has(Entry.progression_id)){
                throw new ProgressionConfigError(`${File}: ${Entry.progression_id} is also in ${Seen.get(Entry.progression_id)}`);
            }

            Seen.set(Entry.progression_id, File);
            Paths.push(Entry);
        }
    }

    return Paths;
}

function Load(Folder: string | undefined, Active: string): LoadedConfig {
    const Bundled = (bundledConfig as any).payload.paths as ProgressionPath[];
    let Paths = Bundled;
    let Payload: unknown = bundledConfig;
    let Description = `bundled, ${Bundled.length} tracks`;

    if(Folder !== undefined){
        const FromDisk = ReadPathsFromFolder(Folder);
        const Replaced: string[] = [];
        const Added: string[] = [];

        Paths = Bundled.map((Path) => {
            const Override = FromDisk.find((Entry) => Entry.progression_id === Path.progression_id);

            if(Override !== undefined){
                Replaced.push(Path.progression_id);
            }

            return Override ?? Path;
        });

        for(const Entry of FromDisk){
            if(!Bundled.some((Path) => Path.progression_id === Entry.progression_id)){
                Added.push(Entry.progression_id);
                Paths.push(Entry);
            }
        }

        Payload = {...(bundledConfig as any), payload: {...(bundledConfig as any).payload, paths: Paths}};
        Description = `${Paths.length} tracks, from ${Folder}: ${[...Replaced.map((Id) => `${Id} replaced`), ...Added.map((Id) => `${Id} added`)].join(", ")}`;
    }

    const ById = new Map(Paths.map((Path) => [Path.progression_id, Path]));

    if(!ById.has(Active)){
        throw new ProgressionConfigError(`ACTIVE_HUNT_PASS is "${Active}", which is not a loaded track (${[...ById.keys()].join(", ")})`);
    }

    return {Key: "", Paths, ById, Payload, ActiveHuntPass: Active, Description: `${Description}; active Hunt Pass ${Active}`};
}

// The loaded config; loaded again when PROGRESSION_CONFIG_DIR or ACTIVE_HUNT_PASS changed. Throws
// ProgressionConfigError for a bad folder or active Hunt Pass.
export function GetProgressionConfig(): LoadedConfig {
    const Folder = process.env.PROGRESSION_CONFIG_DIR?.trim() || undefined;
    const Active = process.env.ACTIVE_HUNT_PASS?.trim() || DEFAULT_ACTIVE_HUNT_PASS;
    const Key = `${Folder ?? ""}|${Active}`;

    if(Loaded === undefined || Loaded.Key !== Key){
        Loaded = {...Load(Folder, Active), Key: Key};
    }

    return Loaded;
}

// The body of GET /progression/config ({code, message, payload: {paths}}): the bundled file as it is
// when nothing is overridden
export function GetProgressionConfigPayload(){
    return GetProgressionConfig().Payload;
}

export function GetConfiguredPaths(): ProgressionPath[] {
    return GetProgressionConfig().Paths;
}

export function GetConfiguredPath(ProgressionId: string): ProgressionPath | undefined {
    return GetProgressionConfig().ById.get(ProgressionId);
}

export function GetActiveHuntPass(){
    return GetProgressionConfig().ActiveHuntPass;
}

// At boot: loads the config once and says what was loaded; throws ProgressionConfigError (the caller stops)
export function CheckProgressionConfig(): string {
    return GetProgressionConfig().Description;
}
