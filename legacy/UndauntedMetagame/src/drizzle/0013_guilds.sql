CREATE TABLE `guildinvites` (
	`inviteId` text PRIMARY KEY NOT NULL,
	`guildId` text NOT NULL,
	`inviteeId` text NOT NULL,
	`inviterId` text NOT NULL,
	`createdAt` integer NOT NULL,
	`expiresAt` integer NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `guildinvites_pair` ON `guildinvites` (`guildId`,`inviteeId`);--> statement-breakpoint
CREATE INDEX `guildinvites_invitee` ON `guildinvites` (`inviteeId`);--> statement-breakpoint
CREATE TABLE `guildmembers` (
	`accountId` text PRIMARY KEY NOT NULL,
	`guildId` text NOT NULL,
	`rank` text NOT NULL,
	`joinedAt` integer NOT NULL,
	`updatedAt` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `guildmembers_guild` ON `guildmembers` (`guildId`);--> statement-breakpoint
CREATE TABLE `guilds` (
	`guildId` text PRIMARY KEY NOT NULL,
	`name` text NOT NULL,
	`nameKey` text NOT NULL,
	`nameplate` text DEFAULT '' NOT NULL,
	`nameplateKey` text,
	`leaderId` text NOT NULL,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `guilds_name_key` ON `guilds` (`nameKey`);--> statement-breakpoint
CREATE UNIQUE INDEX `guilds_nameplate_key` ON `guilds` (`nameplateKey`);