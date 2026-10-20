// Import after ./setup and before controllers/matchmaking, which reads these at load.
// Port 9 on loopback: nothing here may reach a real deploy server.
process.env.MATCHMAKING_MODE = "DEPLOYSERVER";
process.env.DEPLOYSERVER_URL = "127.0.0.1:9";
