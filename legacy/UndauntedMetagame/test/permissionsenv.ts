// Import after ./setup and ./authenv, before the app. Spare loopback ports for this test
// file only (62471 app, 62472 fake deploy server); nothing here may reach the live
// metagame (61000) or deploy server (61001).
export const PERMISSIONS_API_PORT = 62471;
export const PERMISSIONS_DEPLOYSERVER_PORT = 62472;
export const GATEWAY_TEST_SECRET = "test-gateway-secret-0123456789abcdef";

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = `127.0.0.1:${PERMISSIONS_DEPLOYSERVER_PORT}`;
process.env.REGISTRATION_MODE = "OPEN";
process.env.QOS_TARGET_URL = "http://127.0.0.1:61000/QoS";
process.env.LOG_REQUESTS = "0";
process.env.LOG_BODIES = "0";
// Stub mode on purpose: these tests check both kinds of account side by side, so only
// A and B are real (real progression is the default; stub is the opt-out).
process.env.PROGRESSION_MODE = "stub";
process.env.PROGRESSION_REAL_ACCOUNTS = "UID-real-a,UID-real-b";
process.env.GATEWAY_SECRET = GATEWAY_TEST_SECRET;
delete process.env.MISC_ROUTES;
delete process.env.MATCHMAKING_CANCEL;
delete process.env.PROGRESSION_CONFIRM;
delete process.env.GAMESERVER_ALLOW_FROM;
