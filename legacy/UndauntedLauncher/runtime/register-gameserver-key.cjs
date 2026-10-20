// Reuse the server kit CLI's gs-key operation with its file-based credential.
// Electron utility processes retain an IPC channel after a script's work finishes,
// so this one-shot adapter exits explicitly once registration has completed.
require("./dr-db.js")(process.argv.slice(2))
    .then(() => process.exit(0))
    .catch(() => {
        console.error("[local service] game-server credential registration failed");
        process.exit(1);
    });
