CREATE TABLE `escalationprogression` (
	`accountId` text NOT NULL,
	`seasonId` text NOT NULL,
	`level` integer NOT NULL,
	`xp` integer NOT NULL,
	`updateVersion` integer NOT NULL,
	`contentHash` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `seasonId`)
);
--> statement-breakpoint
CREATE TABLE `escalationtalents` (
	`accountId` text NOT NULL,
	`seasonId` text NOT NULL,
	`talentId` text NOT NULL,
	`rank` integer NOT NULL,
	PRIMARY KEY(`accountId`, `seasonId`, `talentId`)
);
--> statement-breakpoint
CREATE TABLE `escalationunlocks` (
	`accountId` text NOT NULL,
	`seasonId` text NOT NULL,
	`unlockId` text NOT NULL,
	`collectedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `seasonId`, `unlockId`)
);
