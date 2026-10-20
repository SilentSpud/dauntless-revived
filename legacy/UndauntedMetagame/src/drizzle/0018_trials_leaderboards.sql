CREATE TABLE `trialruns` (
    `id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
    `trialId` text NOT NULL,
    `trialWeek` integer NOT NULL,
    `difficulty` integer NOT NULL,
    `mode` text NOT NULL,
    `runKey` text NOT NULL,
    `groupKey` text NOT NULL,
    `completionTime` integer NOT NULL,
    `objectivesCompleted` integer NOT NULL,
    `sessionId` text NOT NULL,
    `entries` text NOT NULL,
    `submittedDate` text NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `trialruns_week_trial_difficulty_run` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`runKey`);
--> statement-breakpoint
CREATE INDEX `trialruns_board` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`mode`,`completionTime`);
--> statement-breakpoint
CREATE INDEX `trialruns_group` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`mode`,`groupKey`);
--> statement-breakpoint
CREATE TABLE `trialweeks` (
    `week` integer PRIMARY KEY NOT NULL,
    `finalizedDate` text NOT NULL,
    `awardedAccounts` integer NOT NULL
);
