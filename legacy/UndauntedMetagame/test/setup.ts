import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Import this first in every test file. node --test runs each file in its own
// process, so each file gets its own empty database (migrations run on first use).
const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-test-"));

process.env.DB_FILENAME = path.join(Dir, "test.db");
process.env.NODE_ENV = "production";
process.env.LOG_LEVEL = process.env.TEST_LOG_LEVEL ?? "silent";

export function RemoveTestDb(Close: () => void){
    try{
        Close();
    }
    finally{
        fs.rmSync(Dir, {recursive: true, force: true});
    }
}
