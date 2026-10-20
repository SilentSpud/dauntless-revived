import http from "node:http";

// A stand-in for the deploy server on a spare loopback port: GET /gameservers answers
// the given list; the matchmaking call "starts" a game server on MatchmakingPort.
export type FakeDeployServer = {
    Servers: any[],
    MatchmakingPort: number,
    Requests: string[],
    Close: () => Promise<void>
};

export function StartFakeDeployServer(Port: number, Servers: any[]): Promise<FakeDeployServer> {
    const Fake: FakeDeployServer = { Servers: Servers, MatchmakingPort: 8775, Requests: [], Close: async () => {} };

    const Server = http.createServer((req, res) => {
        Fake.Requests.push(`${req.method} ${req.url}`);
        req.resume();

        if(req.method === "GET" && req.url === "/gameservers"){
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ servers: Fake.Servers }));
            return;
        }

        if(req.method === "POST" && req.url === "/api/matchmaker/handle-matchmaking-for-player"){
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ host: "127.0.0.1", port: Fake.MatchmakingPort }));
            return;
        }

        res.writeHead(404);
        res.end();
    });

    Fake.Close = () => new Promise<void>((Resolve) => {
        Server.closeAllConnections();
        Server.close(() => Resolve());
    });

    return new Promise((Resolve, Reject) => {
        Server.once("error", Reject);
        Server.listen(Port, "127.0.0.1", () => Resolve(Fake));
    });
}

// Three servers as the deploy server reports them: Ramsgate, a hunt and the tutorial
export function ThreeServers(HunterUserId: string){
    return [
        {
            id: "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d01", port: 8777, kind: "city", map: "/Game/Maps/ramsgate/ramsgate_01_persistent", gameMode: null,
            behemoth: null, huntId: null, matchmakerHuntId: null, expectedPlayers: [], maxPlayers: null, startedAt: "2026-09-21T10:00:00.000Z"
        },
        {
            id: "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d02", port: 8774, kind: "hunt", map: "/Game/Maps/islands/1702/cora_jamima", gameMode: null,
            behemoth: "/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C", huntId: "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr",
            matchmakerHuntId: "CR19_MatchmakerHunt_Lerawr_Beta", expectedPlayers: [HunterUserId], maxPlayers: 4, startedAt: "2026-09-21T11:00:00.000Z"
        },
        {
            id: "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d03", port: 8775, kind: "tutorial", map: "/Game/Maps/islands/1705/dia_moss_triforce", gameMode: null,
            behemoth: "/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C", huntId: null, matchmakerHuntId: null,
            expectedPlayers: [], maxPlayers: 1, startedAt: "2026-09-21T11:30:00.000Z"
        }
    ];
}

export const TUTORIAL_GAME_ARGS = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";
