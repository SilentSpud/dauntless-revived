// Import after ./setup and before controllers/matchmaking, which reads these at load.
// A fake deploy server that the status tests start on a spare port (62014); nothing
// here may reach the real deploy server on 61001.
process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = "127.0.0.1:62014";

export const FAKE_DEPLOYSERVER_PORT = 62014;
