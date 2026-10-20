CREATE TABLE `bounties` (
	`accountId` text NOT NULL,
	`bountyId` text NOT NULL,
	`slotIndex` integer,
	`updateVersion` integer NOT NULL,
	`data` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `bountyId`)
);
--> statement-breakpoint
CREATE TABLE `bountydraft` (
	`accountId` text PRIMARY KEY NOT NULL,
	`data` text NOT NULL,
	`updatedDate` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE `cooldowns` (
	`accountId` text NOT NULL,
	`cooldownId` text NOT NULL,
	`startedDate` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `cooldownId`)
);
--> statement-breakpoint
CREATE TABLE `entitlements` (
	`accountId` text NOT NULL,
	`name` text NOT NULL,
	`activatedDate` text NOT NULL,
	`duration` integer NOT NULL,
	`source` text NOT NULL,
	`grantedDate` text NOT NULL,
	`revokedDate` text,
	PRIMARY KEY(`accountId`, `name`)
);
--> statement-breakpoint
CREATE TABLE `huntpassselection` (
	`accountId` text PRIMARY KEY NOT NULL,
	`progressionId` text NOT NULL,
	`updatedDate` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE `loadoutslots` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`numCharacterSlots` integer NOT NULL,
	`activeIndex` integer NOT NULL,
	`updatedDate` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE `objectives` (
	`accountId` text NOT NULL,
	`objectiveId` text NOT NULL,
	`progress` integer NOT NULL,
	`completedCount` integer NOT NULL,
	`createdDate` text NOT NULL,
	`lastModifiedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `objectiveId`)
);
--> statement-breakpoint
CREATE TABLE `progression_events` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`time` text NOT NULL,
	`accountId` text NOT NULL,
	`caller` text NOT NULL,
	`route` text NOT NULL,
	`body` text,
	`status` integer NOT NULL,
	`reply` text,
	`note` text
);
--> statement-breakpoint
CREATE INDEX `progression_events_account` ON `progression_events` (`accountId`,`id`);--> statement-breakpoint
CREATE TABLE `progress_tracks` (
	`accountId` text NOT NULL,
	`progressionId` text NOT NULL,
	`progress` integer NOT NULL,
	`confirmedFreeRank` integer NOT NULL,
	`confirmedPremiumRank` integer NOT NULL,
	`confirmedDate` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `progressionId`)
);
--> statement-breakpoint
CREATE TRIGGER `progression_events_no_update` BEFORE UPDATE ON `progression_events` BEGIN SELECT RAISE(ABORT, 'progression_events is append-only'); END;--> statement-breakpoint
CREATE TRIGGER `progression_events_no_delete` BEFORE DELETE ON `progression_events` BEGIN SELECT RAISE(ABORT, 'progression_events is append-only'); END;
