import http from "node:http";
import { logger, LogThrottle } from "./log";

export type FeedOptions = {
    // http://127.0.0.1:61005 (the allowlist helper; loopback only)
    url: string;
    secret: string;
    // An address the helper accepted is not sent again for this long. The helper keeps an
    // entry for 10 minutes, and a playing client sends a heartbeat every 20 seconds.
    refreshMs: number;
    timeoutMs: number;
    additionalUrls?: string[];
};

// Tells the allowlist helper which addresses belong to logged-in players. Fire and forget: a
// player's request never waits for the helper, and a helper that is down only costs log lines.
export class AllowlistFeed {
    private readonly LastSent = new Map<string, number>();
    private readonly Pending = new Map<string, Promise<void>>();
    private readonly Agent = new http.Agent({ keepAlive: true, maxSockets: 4 });
    private readonly Warnings = new LogThrottle(60_000);
    private readonly Target: URL;
    private readonly Others: AllowlistFeed[];
    // Requests sent to the helper, for tests and the status log.
    public Sent = 0;
    // A helper that is not answering must not be asked again on every single login. One live
    // host logged 222,875 suppressed warnings in four hours against two endpoints that were
    // never going to answer (workers that host is not running). After a failure this feed is
    // left alone for a doubling interval, and the first answer of any kind clears it.
    private Failures = 0;
    private SkipUntil = 0;
    // A single failure still retries at once: a worker that blips must recover on the next
    // login. Only a helper that has failed this many times running is one worth leaving alone.
    private static readonly BackoffAfter = 3;
    private static readonly BackoffBaseMs = 2_000;
    private static readonly BackoffMaxMs = 60_000;

    constructor(private readonly Options: FeedOptions, private readonly Now: () => number = Date.now){
        this.Target = new URL("/allow", Options.url);
        this.Others = (Options.additionalUrls ?? []).map(url => new AllowlistFeed({ ...Options, url, additionalUrls: undefined }, Now));
    }

    Report(Ip: string){
        for (const Feed of this.Others) Feed.Report(Ip);
        if(Ip === "unknown"){
            return;
        }
        const Now = this.Now();
        // Backing off from a helper that is down. The address is deliberately not recorded in
        // LastSent, so it is offered again as soon as the interval passes.
        if(Now < this.SkipUntil){
            return;
        }
        const Last = this.LastSent.get(Ip);
        if(Last !== undefined && Now - Last < this.Options.refreshMs){
            return;
        }
        if(this.Pending.has(Ip)){
            return;
        }

        if(this.LastSent.size > 50_000){
            for(const [Key, At] of this.LastSent){
                if(Now - At >= this.Options.refreshMs){
                    this.LastSent.delete(Key);
                }
            }
        }

        const Work = this.Post(Ip).then((Status) => {
            if(Status === 200){
                this.Answered();
                this.LastSent.set(Ip, this.Now());
                logger.debug("allowlist refreshed", { ip: Ip });
                return;
            }
            if(Status === 400){
                this.Answered();
                // The helper refused this address (a private one, say). Asking again every
                // heartbeat would not change that, so wait like after a success.
                this.LastSent.set(Ip, this.Now());
                if(this.Warnings.Allow(`refused:${Ip}`) !== undefined){
                    logger.warn("allowlist helper refused a player's address; that player's game ports stay closed", { ip: Ip });
                }
                return;
            }
            this.Failed(`status ${Status}`);
        }).catch((error: Error) => {
            this.Failed(error.message);
        }).finally(() => {
            this.Pending.delete(Ip);
        });
        this.Pending.set(Ip, Work);
    }

    // Any answer, including a refusal, proves the helper is up.
    private Answered(){
        this.Failures = 0;
        this.SkipUntil = 0;
    }

    private Failed(Detail: string){
        this.Failures++;
        if(this.Failures >= AllowlistFeed.BackoffAfter){
            this.SkipUntil = this.Now() + Math.min(
                AllowlistFeed.BackoffMaxMs,
                AllowlistFeed.BackoffBaseMs * 2 ** (this.Failures - AllowlistFeed.BackoffAfter),
            );
        }
        this.WarnDown(Detail);
    }

    private WarnDown(Detail: string){
        const Suppressed = this.Warnings.Allow("down");
        if(Suppressed !== undefined){
            logger.warn("allowlist helper did not take an address; players who log in now get no game ports until it answers", { detail: Detail, suppressed: Suppressed });
        }
    }

    private Post(Ip: string): Promise<number> {
        const Body = JSON.stringify({ ip: Ip });
        this.Sent++;
        return new Promise((resolve, reject) => {
            const Req = http.request(this.Target, {
                method: "POST",
                agent: this.Agent,
                headers: {
                    "content-type": "application/json",
                    "content-length": Buffer.byteLength(Body),
                    "x-allowlist-secret": this.Options.secret,
                },
            }, (Res) => {
                Res.resume();
                Res.on("end", () => resolve(Res.statusCode ?? 0));
                Res.on("error", reject);
            });
            Req.setTimeout(this.Options.timeoutMs, () => Req.destroy(new Error("timed out")));
            Req.on("error", reject);
            Req.end(Body);
        });
    }

    // Resolves when nothing is on its way to the helper (tests and shutdown).
    async Idle(){
        await Promise.all(this.Others.map(Feed => Feed.Idle()));
        while(this.Pending.size > 0){
            await Promise.all([...this.Pending.values()]);
        }
    }

    Close(){
        for (const Feed of this.Others) Feed.Close();
        this.Agent.destroy();
    }
}
