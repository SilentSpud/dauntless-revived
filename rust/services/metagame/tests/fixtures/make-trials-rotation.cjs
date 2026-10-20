// Writes trials-rotation.json (tests/trials.rs) from the Node Trials code:
//
//   cd legacy/UndauntedMetagame && npx tsc -p tsconfig.test.json
//   node ../../rust/services/metagame/tests/fixtures/make-trials-rotation.cjs
"use strict";
const path = require("node:path");
const fs = require("node:fs");
const metagame = path.resolve(__dirname, "../../../../../legacy/UndauntedMetagame");
const out = path.join(__dirname, "trials-rotation.json");
process.env.LOG_LEVEL = "silent";
process.env.TRIALS_SCHEDULE = "on";
process.chdir(metagame);
const trials = require(path.join(metagame, "build/src/controllers/trials.js"));
const tuning = require(path.join(metagame, "build/src/routes/tuning.js"));
const weeks = [];
for (let week = -3; week <= 420; week++) {
  weeks.push({ week, behemoth: trials.TrialBehemothForWeek(week), hard: trials.TrialIdForWeek(0, week), elite: trials.TrialIdForWeek(1, week) });
}
const times = ["2019-07-18T18:00:00.000Z", "2019-07-25T17:59:59.999Z", "2024-02-29T12:34:56.789Z", "2026-10-20T09:00:00.000Z", "2026-10-22T18:00:00.000Z"];
const schedules = times.map((at) => ({ at, week: trials.TrialWeekAt(new Date(at)), window: trials.TrialsWindowForWeek(trials.TrialWeekAt(new Date(at)), new Date(at)), schedule: tuning.GetSeasonalEventSchedule(new Date(at)) }));
const ranks = [0, 179999, 180000, 299999, 300000, 1799999, 1800000, 86400000].map((time) => ({ time, rank: trials.TrialRewardRank(time) }));
fs.writeFileSync(out, JSON.stringify({ about: "Written by the Node Trials code (TrialBehemothForWeek, TrialIdForWeek, TrialsWindowForWeek, GetSeasonalEventSchedule with TRIALS_SCHEDULE=on, TrialRewardRank); do not edit.", weeks, schedules, ranks }, null, 2) + "\n");
console.log("weeks", weeks.length);
