CREATE TABLE `blocks` (
	`blockerId` text NOT NULL,
	`blockedId` text NOT NULL,
	`createdAt` integer NOT NULL,
	PRIMARY KEY(`blockerId`, `blockedId`)
);
--> statement-breakpoint
CREATE INDEX `blocks_blocked` ON `blocks` (`blockedId`);--> statement-breakpoint
CREATE TABLE `friendships` (
	`userLow` text NOT NULL,
	`userHigh` text NOT NULL,
	`requesterId` text NOT NULL,
	`status` text NOT NULL,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL,
	PRIMARY KEY(`userLow`, `userHigh`)
);
--> statement-breakpoint
CREATE INDEX `friendships_high` ON `friendships` (`userHigh`);