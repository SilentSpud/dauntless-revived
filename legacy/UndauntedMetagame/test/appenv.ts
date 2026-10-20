// Import after ./setup and ./authenv, before the app (the matchmaking controller reads its mode when it
// loads). Every switch these tests depend on starts at its default; a test that needs another value sets
// it and puts it back (the switches of src/features.ts are read at every use). The app listens on a port
// the system picks and never reaches a deploy server.
process.env.MATCHMAKING_MODE = "DISABLED";
process.env.LOG_BODIES = "0";
process.env.LOG_REQUESTS = "0";

for(const Name of [
    "PROGRESSION_MODE", "PROGRESSION_REAL_ACCOUNTS", "PROGRESSION_CONFIRM", "PROGRESSION_GRANT_CAP", "PROGRESSION_ALLOW_DELETE",
    "ENTITLEMENTS_DEFAULT", "INVENTORY_REFUSE_OVERSPEND", "INVENTORY_REPORT_REMOVALS", "GATEWAY_SECRET", "GAMESERVER_ALLOW_FROM", "MISC_ROUTES",
    "ESCALATION_MODE", "ESCALATION_STRICT", "STORE", "STORE_REPEATABLE_TOKENS", "PROGRESSION_REPLAY_WINDOW_S",
    "PROGRESSION_CONFIG_DIR", "ACTIVE_HUNT_PASS", "PROGRESSION_CONFIRM_ENTITLEMENTS", "BALANCE_FROM_INVENTORY",
    "SLAYER_LINKS", "CHAT", "CHAT_PRESENCE", "VERIFY_STUB_ACCOUNT"
]){
    delete process.env[Name];
}

// Sets environment variables for the length of Body (a test), then puts the old values back
export async function WithEnv<T>(Values: Record<string, string | undefined>, Body: () => T | Promise<T>): Promise<T> {
    const Old: Record<string, string | undefined> = {};

    for(const [Name, Value] of Object.entries(Values)){
        Old[Name] = process.env[Name];

        if(Value === undefined){
            delete process.env[Name];
        }
        else{
            process.env[Name] = Value;
        }
    }

    try{
        return await Body();
    }
    finally{
        for(const [Name, Value] of Object.entries(Old)){
            if(Value === undefined){
                delete process.env[Name];
            }
            else{
                process.env[Name] = Value;
            }
        }
    }
}
