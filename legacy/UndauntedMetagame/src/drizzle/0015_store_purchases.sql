CREATE TABLE `storepurchases` (
	`tokenHash` text PRIMARY KEY NOT NULL,
	`accountId` text NOT NULL,
	`characterId` text NOT NULL,
	`skuId` text NOT NULL,
	`offerHash` text NOT NULL,
	`createdDate` text NOT NULL,
	`expiresDate` text NOT NULL,
	`redeemedDate` text
);
--> statement-breakpoint
CREATE INDEX `storepurchases_account` ON `storepurchases` (`accountId`);