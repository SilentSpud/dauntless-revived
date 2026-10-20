// Import after ./setup and ./authenv, before the app. Spare loopback ports for socialflow.test.ts only;
// nothing here may reach the live metagame (61000) or deploy server (61001).
export const SOCIAL_API_PORT = 62901;
export const SOCIAL_DEPLOYSERVER_PORT = 62902;

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = `127.0.0.1:${SOCIAL_DEPLOYSERVER_PORT}`;
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
