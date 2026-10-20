import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Import after ./setup and ./authenv, before the app: LOG_BODIES is read when the app loads. The body
// log goes to a fresh temporary folder; the app listens on a port the system picks.
const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "dr-platform-test-"));

export const BODY_LOG = path.join(Dir, "bodies.log");

process.env.LOG_BODIES = "1";
process.env.BODY_LOG_FILE = BODY_LOG;
process.env.LOG_REQUESTS = "0";
process.env.MATCHMAKING_MODE = "DISABLED";
delete process.env.BODY_LOG_PER_PATH;
delete process.env.MISC_ROUTES;
delete process.env.GATEWAY_SECRET;
delete process.env.PROGRESSION_MODE;
delete process.env.PROGRESSION_REAL_ACCOUNTS;
// The switches of src/features.ts at their defaults (the boot-line test lists them)
for(const Name of ["ESCALATION_MODE", "ESCALATION_STRICT", "STORE", "STORE_REPEATABLE_TOKENS", "PROGRESSION_REPLAY_WINDOW_S", "PROGRESSION_CONFIRM_ENTITLEMENTS", "BALANCE_FROM_INVENTORY", "SLAYER_LINKS", "CHAT_PRESENCE", "VERIFY_STUB_ACCOUNT"]){
    delete process.env[Name];
}

export function RemovePlatformTestDir(){
    fs.rmSync(Dir, { recursive: true, force: true });
}
