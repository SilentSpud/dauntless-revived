"use strict";

// External writes restart SQLite's incremental backup. Bound that work rather
// than leaving an hourly backup reading the same pages for days.
function backupProgress({timeoutMs = 900000, maxRestarts = 3, now = Date.now} = {}) {
    const deadline = now() + timeoutMs;
    let previous = Infinity, restarts = 0;
    return ({remainingPages}) => {
        if (now() >= deadline) throw new Error("backup deadline exceeded; retry when the server is quieter");
        if (remainingPages > previous && ++restarts >= maxRestarts)
            throw new Error("backup repeatedly restarted by live writes; retry when the server is quieter");
        previous = remainingPages;
        return 100;
    };
}
module.exports = {backupProgress};
