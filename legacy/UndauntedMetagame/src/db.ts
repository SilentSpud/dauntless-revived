import { drizzle } from "drizzle-orm/better-sqlite3";
import { migrate } from "drizzle-orm/better-sqlite3/migrator"
import * as schema from "./db/schema"
import { logger } from "./logger";

 const db = drizzle(process.env.DB_FILENAME!, {schema});

// Rollback journal unless DB_WAL=1. The documented backup (copy the stopped file) and
// restore (copy a backup over it) handle undaunted.db alone, and the stack stops node
// with Stop-Process -Force, which never checkpoints: in WAL mode the recent saves would
// live only in undaunted.db-wal, and a stale -wal next to a restored file is replayed
// over it. A file left in WAL mode is switched back, which checkpoints and removes the -wal.
export function ApplyJournalMode(Client: typeof db.$client, Wal: boolean){
    const Wanted = Wal ? "wal" : "delete";

    try{
        const JournalMode = Client.pragma(`journal_mode = ${Wanted}`, {simple: true});

        if(JournalMode !== Wanted){
            logger.warn(`Database journal mode is ${JournalMode}, not ${Wanted}`);
        }

        return JournalMode;
    }
    catch(error){
        logger.warn(error, `Could not switch the database to journal mode ${Wanted}`);
        return undefined;
    }
}

ApplyJournalMode(db.$client, process.env.DB_WAL === "1");

let didMigration = false;

export function GetDb(){
    if(!didMigration){
        didMigration = true;

        migrate(db, {migrationsFolder: "./src/drizzle"});
    }

    return db;
}