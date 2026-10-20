import http from "node:http";
import { CreateContentHandler } from "./app";
import { AuthCache, MetagameLookup } from "./auth";
import { Branding } from "./branding";
import { Config, LoadConfig } from "./config";
import { CheckGameDir } from "./gamedir";
import { StreamLimiter } from "./limits";
import { logger } from "./log";
import { EXPECTED_BUILD, IndexManifest, LoadManifest } from "./manifest";
import { News } from "./news";

function Fatal(Message: string): never {
    logger.error(Message);
    process.exit(1);
}

function Listen(Server: http.Server, Host: string, Port: number): Promise<void> {
    return new Promise((resolve, reject) => {
        const OnError = (error: Error) => reject(error);
        Server.once("error", OnError);
        Server.listen(Port, Host, () => {
            Server.off("error", OnError);
            resolve();
        });
    });
}

async function Main(){
    let TheConfig: Config;
    try{
        TheConfig = LoadConfig();
    }
    catch(error){
        Fatal(`Configuration: ${error instanceof Error ? error.message : String(error)}`);
    }

    let Manifest;
    try{
        Manifest = LoadManifest(TheConfig.manifestPath);
    }
    catch(error){
        Fatal(`Manifest ${TheConfig.manifestPath}: ${error instanceof Error ? error.message : String(error)}`);
    }
    if(Manifest.build !== EXPECTED_BUILD){
        logger.warn("manifest is for a different build than this server was written for", { build: Manifest.build });
    }

    // Startup check: every manifest file present with the right size. Files that fail are not served.
    const Report = CheckGameDir(TheConfig.gameDir, Manifest);
    logger.info("game folder checked (sizes only; npm run verify hashes everything)", {
        gameDir: TheConfig.gameDir,
        build: Manifest.build,
        files: Manifest.files.length,
        ok: Report.ok,
        missing: Report.missing.length,
        mismatched: Report.mismatched.length,
        bytesOk: Report.bytesOk,
        totalBytes: Manifest.totalBytes,
    });
    if(Report.ok === 0){
        Fatal(`None of the ${Manifest.files.length} manifest files is in ${TheConfig.gameDir}. Is CONTENT_GAME_DIR the folder that contains Archon\\?`);
    }
    for(const FilePath of Report.missing.slice(0, 20)){
        logger.warn("missing game file (not served)", { path: FilePath });
    }
    for(const FilePath of Report.mismatched.slice(0, 20)){
        logger.warn("game file has the wrong size or is not a regular file (not served)", { path: FilePath, state: Report.states.get(FilePath) });
    }
    if(Report.missing.length + Report.mismatched.length > 40){
        logger.warn("more problems than listed; run npm run verify for the full list", {});
    }

    const Handler = CreateContentHandler({
        downloadBaseUrl: TheConfig.downloadBaseUrl,
        manifest: Manifest,
        index: IndexManifest(Manifest),
        gameDir: TheConfig.gameDir,
        states: Report.states,
        auth: new AuthCache({ lookup: MetagameLookup(TheConfig.metagameUrl), ttlMs: TheConfig.authCacheSeconds * 1000 }),
        limiter: new StreamLimiter(TheConfig.maxStreamsPerAccount, TheConfig.maxStreamsTotal),
        branding: new Branding(TheConfig.brandingDir),
        news: new News(TheConfig.newsFile, Date.now, process.env.CONTENT_NEWS_URL),
    });

    const Servers: http.Server[] = [];
    for(const Host of TheConfig.bindHosts){
        const Server = http.createServer({ requestTimeout: 30_000, headersTimeout: 20_000, keepAliveTimeout: 5_000 }, Handler);
        // A download whose client stopped reading for two minutes is dropped, which frees its slot.
        Server.setTimeout(120_000);
        Server.maxConnections = 512;
        try{
            await Listen(Server, Host, TheConfig.port);
        }
        catch(error){
            Fatal(`Could not listen on ${Host}:${TheConfig.port}: ${error instanceof Error ? error.message : String(error)}`);
        }
        Servers.push(Server);
        logger.info(`Dauntless Revived content server on ${Host}:${TheConfig.port}`);
    }

    logger.info("content server ready", {
        metagame: TheConfig.metagameUrl,
        branding: TheConfig.brandingDir ?? null,
        news: TheConfig.newsFile ?? null,
        maxStreamsPerAccount: TheConfig.maxStreamsPerAccount,
        maxStreamsTotal: TheConfig.maxStreamsTotal,
    });

    const Stop = (Signal: string) => {
        logger.info("stopping", { signal: Signal });
        for(const Server of Servers){
            Server.close();
            Server.closeAllConnections();
        }
        setTimeout(() => process.exit(0), 500).unref();
    };
    process.on("SIGINT", () => Stop("SIGINT"));
    process.on("SIGTERM", () => Stop("SIGTERM"));
}

Main().catch((error) => Fatal(error instanceof Error ? error.stack ?? error.message : String(error)));
