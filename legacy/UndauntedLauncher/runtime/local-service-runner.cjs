// Runs one local service inside an Electron utility process.
//
// The environment is loaded inside the child process so service configuration
// and secrets do not become global Launcher environment variables.
const [serverPath, envPath] = process.argv.slice(2);

if(!serverPath || !envPath){
    console.error("[local service] missing server or environment path");
    process.exit(1);
}

let Service;

try{
    process.loadEnvFile(envPath);
    Service = require(serverPath);
}
catch(error){
    console.error("[local service] startup failed:", error instanceof Error ? error.message : String(error));
    process.exit(1);
}

let Stopping = false;

process.parentPort?.on("message", (event) => {
    if(event.data?.type !== "shutdown" || Stopping)return;
    Stopping = true;

    if(typeof Service?.shutdown !== "function"){
        process.kill(process.pid, "SIGTERM");
        return;
    }

    void Promise.resolve(Service.shutdown())
        .catch((error) => console.error("[local service] shutdown failed:", error instanceof Error ? error.message : String(error)))
        .finally(() => process.exit(0));
});