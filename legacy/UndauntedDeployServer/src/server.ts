import { app } from "./app";
import { IsPersistentWorldLivenessOn, StartupAndReportFailure, ShutdownGameservers } from "./controllers/gameservers";
import { RunWatchdog } from "./controllers/watchdog";
import { logger } from "./logger";

if(process.env.SERVER_ROLE === "database-only") throw new Error("Deploy service disabled on database-only node");
const PORT = Number(process.env.PORT);
// The deploy server has no authentication at all -- anyone who can reach it
// can spawn game processes on this machine. It must never listen beyond
// loopback; only the metagame on the same host talks to it.
const BIND_HOST = process.env.BIND_HOST || "127.0.0.1";

let Startup: Promise<void> | undefined;
let Watchdog: NodeJS.Timeout | undefined;

const Server = app.listen(PORT, BIND_HOST, (err?: Error) => {
  if (err) {
    logger.fatal(`Could not listen on ${BIND_HOST}:${PORT}: ${err.message}`);
    process.exit(1);
  }

  // A failed start is reported, not an unhandled rejection (from github.com/Harmonicrain/Undaunted 895f7c7)
  Startup = StartupAndReportFailure();

  Watchdog = setInterval(() => { void RunWatchdog().catch(error => logger.error({error}, "Gameserver watchdog failed")); }, 15 * 1000);

  logger.info(`Dauntless Revived deploy server on port ${PORT}`);
  logger.info(`Ramsgate and Dojo liveness check before handing them out: ${IsPersistentWorldLivenessOn() ? "on" : "off (PERSISTENT_WORLD_LIVENESS=0)"}`);
  logger.info(`Clear Skies, Slayer.`);
});

// Used by the Launcher-owned local stack so Deploy can stop its own game processes before exiting.
export async function shutdown(){
  if(Watchdog) clearInterval(Watchdog);

  const Closing = new Promise<void>(Resolve =>
      Server.close(() => Resolve())
  );

  await ShutdownGameservers();

  if(Startup) await Startup;

  await Closing;
}