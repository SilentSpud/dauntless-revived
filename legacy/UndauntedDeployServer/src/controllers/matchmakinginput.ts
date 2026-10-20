// What the matchmaking call may carry. It comes from the metagame, but most of it is the
// game client's own input, and it ends up on a game server's command line: the map and
// the behemoth of the client's game args (the tutorial), and the hunt id inside the
// expected-player list ("uid:huntid,uid:huntid"). The metagame checks the same first;
// this is the second line. Keep in sync with the metagame's CheckMatchmakingInput
// (UndauntedMetagame/src/controllers/matchmaking.ts).

const HUNT_ID = /^[A-Za-z0-9_+]{1,128}$/; // e.g. CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr, ..._Patrol_Heroic+_Gem
const GAME_ARGS_CHARACTERS = /^[A-Za-z0-9_/.?=+-]{1,2048}$/;
const GAME_MAP_PATH = /^\/Game(\/[A-Za-z0-9_]+)+(\.[A-Za-z0-9_]+)?$/; // /Game/Maps/islands/1705/dia_moss_triforce
const GAME_ASSET_PATH = /^\/Game(\/[A-Za-z0-9_]+)+\.[A-Za-z0-9_]+$/; // /Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C
const ACCOUNT_ID = /^[A-Za-z0-9_-]{1,64}$/; // UID-<uuid>; never ':' or ',', the list's separators
const MAX_EXPECTED_PLAYERS = 16;

// undefined when the request is acceptable, else what is wrong with it
export function CheckMatchmakingRequest(GameMode: unknown, GameArgs: unknown, HuntId: unknown, ExpectedPlayers: unknown): string | undefined {
    if(HuntId != undefined && !(typeof HuntId === "string" && (HuntId.length === 0 || HUNT_ID.test(HuntId)))){
        return "the hunt id is not a hunt id";
    }

    if(GameArgs != undefined && typeof GameArgs !== "string"){
        return "the game args are not a string";
    }

    if(GameMode === "ISLAND" && typeof GameArgs === "string" && GameArgs.trim().length > 0){
        const Parts = GameArgs.split("?");
        const Behemoth = Parts[2]?.split("=")[1];

        if(!GAME_ARGS_CHARACTERS.test(GameArgs) || !GAME_MAP_PATH.test(Parts[0]) || Behemoth === undefined || (Behemoth.length > 0 && !GAME_ASSET_PATH.test(Behemoth))){
            return "the game args are not a map and behemoth this server can start";
        }
    }

    if(ExpectedPlayers != undefined){
        if(!Array.isArray(ExpectedPlayers) || ExpectedPlayers.length > MAX_EXPECTED_PLAYERS || !ExpectedPlayers.every((Player) => typeof Player === "string" && ACCOUNT_ID.test(Player))){
            return "the expected players are not a list of account ids";
        }
    }

    return undefined;
}
