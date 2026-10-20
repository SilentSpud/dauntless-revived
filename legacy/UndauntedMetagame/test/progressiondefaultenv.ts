// Import after ./setup and ./authenv, before the app. A spare loopback port for this test
// file only (62501; nothing listens on 62502, and no test here matchmakes); nothing here
// may reach the live metagame (61000) or deploy server (61001).
export const PROGRESSION_DEFAULT_API_PORT = 62501;

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = "127.0.0.1:62502";
process.env.REGISTRATION_MODE = "OPEN";
process.env.QOS_TARGET_URL = "http://127.0.0.1:61000/QoS";
process.env.LOG_REQUESTS = "0";
process.env.LOG_BODIES = "0";
// Nothing set: this file checks what a server gets with no progression settings at all
delete process.env.PROGRESSION_MODE;
delete process.env.PROGRESSION_REAL_ACCOUNTS;
delete process.env.PROGRESSION_CONFIRM;
delete process.env.ENTITLEMENTS_DEFAULT;
delete process.env.GATEWAY_SECRET;
delete process.env.GAMESERVER_ALLOW_FROM;
