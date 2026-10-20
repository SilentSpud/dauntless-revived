// Import after ./setup and ./authenv, before the app. Spare loopback ports for chathttp.test.ts only;
// nothing here may reach the live metagame (61000), deploy server (61001) or chat (61099). The deploy port
// is only written into the settings: nothing in these tests starts a hunt.
export const CHAT_HTTP_API_PORT = 62921;
export const CHAT_HTTP_DEPLOYSERVER_PORT = 62922;

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = `127.0.0.1:${CHAT_HTTP_DEPLOYSERVER_PORT}`;
process.env.REGISTRATION_MODE = "OPEN";
process.env.QOS_TARGET_URL = "http://127.0.0.1:61000/QoS";
process.env.TARGET_CHANGELIST = "239827";
process.env.LOG_REQUESTS = "0";
process.env.LOG_BODIES = "0";
delete process.env.MISC_ROUTES;
delete process.env.ACCOUNT_DISPLAY_NAME;
delete process.env.ACCOUNTINFO_PUBLIC_LEGACY;
delete process.env.ACCOUNT_MAPPING;
delete process.env.GATEWAY_SECRET;
delete process.env.CHAT;
delete process.env.CHAT_NICK_CHECK;
