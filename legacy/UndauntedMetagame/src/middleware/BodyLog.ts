import fs from "node:fs";
import { NextFunction, Request, Response } from "express";
import { logger } from "../logger";
import { BodyLogPerPath } from "../features";

// Removes credentials from a URL or a serialised body before it is logged: tokens (JWTs, which some
// routes carry in the path, e.g. DELETE /account/api/oauth/sessions/kill/<token>), account keys (UUK_
// and 48 hex characters) and any other run of 64 or more token characters (keys, purchase tokens).
export function Redact(Text: string){
    return RedactJwt(Text)
        .replace(/UUK_[0-9a-fA-F]{48}(?![0-9a-fA-F])/g, "<redacted>")
        .replace(/[\w-]{64,}/g, "<redacted>");
}

function IsJwtCharacter(Character: string | undefined): boolean {
    return Character !== undefined && /[A-Za-z0-9_-]/.test(Character);
}

// Linear scanner rather than a backtracking expression: request bodies and paths are untrusted.
function RedactJwt(Text: string): string {
    let Out = "";
    let From = 0;

    while(From < Text.length){
        const Start = Text.indexOf("eyJ", From);
        if(Start < 0){ Out += Text.slice(From); break; }
        let At = Start + 3;
        while(IsJwtCharacter(Text[At])) At++;
        if(At === Start + 3 || Text[At++] !== "."){
            Out += Text.slice(From, Start + 3); From = Start + 3; continue;
        }
        const Payload = At;
        while(IsJwtCharacter(Text[At])) At++;
        if(At === Payload || Text[At++] !== "."){
            Out += Text.slice(From, Start + 3); From = Start + 3; continue;
        }
        const Signature = At;
        while(IsJwtCharacter(Text[At])) At++;
        if(At === Signature){
            Out += Text.slice(From, Start + 3); From = Start + 3; continue;
        }
        Out += Text.slice(From, Start) + "<token>";
        From = At;
    }
    return Out;
}

// The routes whose request bodies LOG_BODIES records: the save routes that are still stubbed or
// missing (their request formats are only inferred from the client binary, and a wrong response shape
// can crash the client), the party, friends, guild and account lookups of roadmap 1.9 and 3.11, and the
// store, Trials, leaderboard-profile, Slayer Link and Escalation routes of the Harmonic port.
export const BODY_ROUTES = /^\/(progression|huntpass|bounty|cooldown|escalation|entitlement|loadout\/[^/]+\/[^/]+\/unlock|product|token\/|notification\/|reconcile|slayerlink|candidate|party|friends|guild|balance|store|inventory|trials\/|profile\/update|account\/api\/public\/account|account\/mapping|accountinfo\/public)/;

// Lines written per "METHOD /path" in this run, for BODY_LOG_PER_PATH
const LinesPerPath = new Map<string, number>();

// LOG_BODIES=1: one JSON object per line in File, for the routes above:
//   {t, method, url, gs, body, status, ms}
// t is when the request arrived, url has the query string, gs is 1 for a game server's call, body is
// capped at 8 KB (64 KB for /inventory, whose hunt-end batches decide INVENTORY_REFUSE_OVERSPEND),
// status and ms are the answer's status and how long it took. A request whose connection closed before
// the answer was sent also gets "aborted": true. Credentials are removed from the url and the body
// (Redact). The body is read when the request arrives, before a route can change it; the line is
// written when the answer is done. BODY_LOG_PER_PATH caps the lines per method and path.
export function BodyLog(File: string){
    return (req: Request, res: Response, next: NextFunction) => {
        if(!BODY_ROUTES.test(req.path)){
            next();
            return;
        }

        const Cap = BodyLogPerPath();

        if(Cap > 0){
            const Key = `${req.method} ${Redact(req.path)}`;
            const Written = LinesPerPath.get(Key) ?? 0;

            if(Written >= Cap){
                next();
                return;
            }

            if(LinesPerPath.size >= 10000){
                LinesPerPath.clear();
            }

            LinesPerPath.set(Key, Written + 1);

            if(Written + 1 === Cap){
                logger.info(`body log: ${Key} reached BODY_LOG_PER_PATH=${Cap}; no more lines for it in this run`);
            }
        }

        const Started = Date.now();
        let Body = "";
        try{ Body = Redact(JSON.stringify(req.body ?? null)); } catch { Body = "<unserialisable>"; }
        const Limit = req.path.startsWith("/inventory") ? 65536 : 8192;
        if(Body.length > Limit) Body = Body.slice(0, Limit) + "…<truncated>";

        const Entry = {
            t: new Date(Started).toISOString(),
            method: req.method,
            url: Redact(req.originalUrl),
            gs: req.headers["x-undaunted-gameserver-apikey"] ? 1 : 0,
            body: Body
        };

        let Done = false;
        const Write = (Aborted: boolean) => {
            if(Done){
                return;
            }

            Done = true;

            const Line = JSON.stringify({...Entry, status: res.statusCode, ms: Date.now() - Started, ...(Aborted ? {aborted: true} : {})});
            fs.appendFile(File, Line + "\n", (err) => { if (err) logger.warn(`body log write failed: ${err.message}`); });
        };

        res.once("finish", () => Write(false));
        res.once("close", () => Write(!res.writableFinished));

        next();
    };
}

// Tests only
export function ResetBodyLogForTests(){
    LinesPerPath.clear();
}
