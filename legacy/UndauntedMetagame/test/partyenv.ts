import http from "node:http";

// Import after ./setup and ./authenv, before anything that loads controllers/matchmaking
// (it reads these at load). Spare loopback ports for the party tests only; nothing here may
// reach the live metagame (61000) or deploy server (61001).
export const PARTY_DEPLOYSERVER_PORT = 62481;   // party.test.ts
export const PARTY_HTTP_API_PORT = 62482;       // partyhttp.test.ts
export const PARTY_HTTP_DEPLOYSERVER_PORT = 62483;

export function UsePartyEnv(DeployPort: number){
    process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
    process.env.DEPLOYSERVER_URL = `127.0.0.1:${DeployPort}`;
    process.env.REGISTRATION_MODE = "OPEN";
    process.env.QOS_TARGET_URL = "http://127.0.0.1:61000/QoS";
    process.env.TARGET_CHANGELIST = "239827";
    process.env.LOG_REQUESTS = "0";
    process.env.LOG_BODIES = "0";
    delete process.env.MISC_ROUTES;
    delete process.env.MATCHMAKING_CANCEL;
    delete process.env.ACCOUNT_DISPLAY_NAME;
    delete process.env.ACCOUNTINFO_PUBLIC_LEGACY;
    delete process.env.ACCOUNT_MAPPING;
    delete process.env.GATEWAY_SECRET;
    delete process.env.PROGRESSION_REAL_ACCOUNTS;
    delete process.env.PROGRESSION_MODE;
}

// A deploy server stand-in that records every matchmaking body. Ramsgate is 8777, the Dojo
// 8776, hunts get 8775, 8774, ... unless Fail is set (then it answers 500).
export type RecordingDeploy = {
    Calls: any[],
    Fail: boolean,
    Capacity?: boolean,
    DelayMs: number,
    NextHuntPort: number,
    Close: () => Promise<void>
};

export function StartRecordingDeploy(Port: number): Promise<RecordingDeploy> {
    const Deploy: RecordingDeploy = { Calls: [], Fail: false, DelayMs: 0, NextHuntPort: 8775, Close: async () => {} };

    const Server = http.createServer((req, res) => {
        let Body = "";

        req.on("data", (Chunk) => { Body += Chunk; });
        req.on("end", () => {
            if(req.method !== "POST" || req.url !== "/api/matchmaker/handle-matchmaking-for-player"){
                res.writeHead(404);
                res.end();
                return;
            }

            const Parsed = JSON.parse(Body);
            Deploy.Calls.push(Parsed);

            setTimeout(() => {
                if (Deploy.Capacity) {
                    res.writeHead(503, {'content-type': 'application/json'});
                    res.end(JSON.stringify({error: 'capacity_unavailable', reason: 'memory'}));
                    return;
                }
                if(Deploy.Fail){
                    res.writeHead(500);
                    res.end();
                    return;
                }

                const Port = Parsed.GameMode === "CITY" ? 8777 : Parsed.GameMode === "SHARED" ? 8776 : Deploy.NextHuntPort--;

                res.writeHead(200, { "content-type": "application/json" });
                res.end(JSON.stringify({ host: "127.0.0.1", port: Port }));
            }, Deploy.DelayMs);
        });
    });

    Deploy.Close = () => new Promise<void>((Resolve) => {
        Server.closeAllConnections();
        Server.close(() => Resolve());
    });

    return new Promise((Resolve, Reject) => {
        Server.once("error", Reject);
        Server.listen(Port, "127.0.0.1", () => Resolve(Deploy));
    });
}
