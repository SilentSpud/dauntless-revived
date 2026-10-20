const WEEK_MS = 7 * 86400000;
const EPOCH = Date.UTC(1970, 0, 1, 18);

// One boundary for Middleman offers, cosmetic stock and their availability dates.
export function WeeklyStoreWindow(at = new Date()) {
    const index = Math.floor((at.getTime() - EPOCH) / WEEK_MS);
    const start = EPOCH + index * WEEK_MS;
    return {index, start, end: start + WEEK_MS};
}
