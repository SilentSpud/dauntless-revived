CREATE TABLE `slayerlinkinvites` (
	`inviteId` text PRIMARY KEY NOT NULL,
	`senderId` text NOT NULL,
	`targetId` text NOT NULL,
	`senderSlot` integer NOT NULL,
	`createdAt` integer NOT NULL,
	`expiresAt` integer NOT NULL,
	`status` text NOT NULL
);
--> statement-breakpoint
CREATE INDEX `slayerlinkinvites_sender` ON `slayerlinkinvites` (`senderId`);--> statement-breakpoint
CREATE INDEX `slayerlinkinvites_target` ON `slayerlinkinvites` (`targetId`);--> statement-breakpoint
CREATE UNIQUE INDEX `slayerlinkinvites_pending_pair` ON `slayerlinkinvites` (`senderId`,`targetId`) WHERE "status" = 'PENDING';--> statement-breakpoint
CREATE TABLE `slayerlinks` (
	`linkId` text PRIMARY KEY NOT NULL,
	`senderId` text NOT NULL,
	`targetId` text NOT NULL,
	`senderSlot` integer NOT NULL,
	`targetSlot` integer NOT NULL,
	`createdAt` integer NOT NULL,
	`endsAt` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `slayerlinks_sender` ON `slayerlinks` (`senderId`);--> statement-breakpoint
CREATE INDEX `slayerlinks_target` ON `slayerlinks` (`targetId`);