// Import after ./setup and ./authenv, before the app. Spare loopback ports for this test
// file only; nothing here may reach the live metagame (61000) or deploy server (61001).
export const FRIENDS_API_PORT = 62016;
export const FRIENDS_DEPLOYSERVER_PORT = 62015;

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = `127.0.0.1:${FRIENDS_DEPLOYSERVER_PORT}`;
process.env.REGISTRATION_MODE = "OPEN";
process.env.CONTENT_PORT = "61002";
process.env.LOG_REQUESTS = "0";
process.env.LOG_BODIES = "0";
