import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Import after ./setup and before controllers/gameservers, which reads these at load. The binary path
// is inside a fresh temporary folder that is never created, so a real start fails the way a wrong
// GAMESERVER_BINARY_PATH does: nothing here may start a game process.
const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "dr-deploy-test-"));

export const MISSING_BINARY = path.join(Dir, "missing", "Dauntless-Win64-Shipping.exe");

process.env.PORT_RANGE_BEGIN = "8770";
process.env.PORT_RANGE_END = "8777";
process.env.GAMESERVER_BINARY_PATH = MISSING_BINARY;
process.env.METAGAME_API_KEY = "test-key-not-real";
process.env.MY_IP = "127.0.0.1";
process.env.SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP = "0";
delete process.env.PERSISTENT_WORLD_LIVENESS;
delete process.env.ENABLE_DOJO;

export function RemoveDeployTestDir(){
    fs.rmSync(Dir, { recursive: true, force: true });
}

// Legacy fixed-city coverage; pool behavior has its own tests.
process.env.RAMSGATE_POOL = '0';
