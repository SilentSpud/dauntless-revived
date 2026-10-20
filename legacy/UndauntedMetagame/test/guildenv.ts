// Import after ./setup and ./authenv, before the app. Spare loopback ports for guildhttp.test.ts only;
// nothing here may reach the live metagame (61000) or deploy server (61001).
export const GUILD_API_PORT = 62903;
export const GUILD_DEPLOYSERVER_PORT = 62904;

process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = `127.0.0.1:${GUILD_DEPLOYSERVER_PORT}`;
process.env.REGISTRATION_MODE = "OPEN";
process.env.QOS_TARGET_URL = "http://127.0.0.1:61000/QoS";
process.env.TARGET_CHANGELIST = "239827";
process.env.LOG_REQUESTS = "0";
process.env.LOG_BODIES = "0";
// Small, so "the guild is full" is quick to reach
process.env.GUILD_MAX_MEMBERS = "3";
delete process.env.GUILDS;
delete process.env.GUILD_INVITE_TTL_DAYS;
delete process.env.GUILD_NAME_DENYLIST;
delete process.env.MISC_ROUTES;
delete process.env.ACCOUNTINFO_PUBLIC_LEGACY;
delete process.env.ACCOUNT_MAPPING;
delete process.env.GATEWAY_SECRET;
delete process.env.GAMESERVER_ALLOW_FROM;
