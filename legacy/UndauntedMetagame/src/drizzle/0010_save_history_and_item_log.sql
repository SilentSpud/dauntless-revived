CREATE TABLE `characterhistory` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`characterId` text NOT NULL,
	`userId` text NOT NULL,
	`updateVersion` integer NOT NULL,
	`name` text NOT NULL,
	`data` text NOT NULL,
	`savedDate` text NOT NULL,
	`reason` text NOT NULL
);
--> statement-breakpoint
CREATE INDEX `characterhistory_character` ON `characterhistory` (`characterId`,`id`);--> statement-breakpoint
CREATE TABLE `inventorylog` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`time` text NOT NULL,
	`userId` text NOT NULL,
	`characterId` text NOT NULL,
	`transactionId` text,
	`source` text,
	`caller` text NOT NULL,
	`operation` text NOT NULL,
	`catalogId` text,
	`instanceId` text,
	`quantityChange` integer,
	`quantityAfter` integer,
	`updateVersion` integer
);
--> statement-breakpoint
CREATE INDEX `inventorylog_character` ON `inventorylog` (`characterId`,`id`);--> statement-breakpoint
CREATE TABLE `inventorytransactions` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`transactionId` text NOT NULL,
	`characterId` text NOT NULL,
	`userId` text NOT NULL,
	`requestHash` text NOT NULL,
	`status` integer NOT NULL,
	`response` text NOT NULL,
	`createdDate` text NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `inventorytransactions_character_transaction_request` ON `inventorytransactions` (`characterId`,`transactionId`,`requestHash`);--> statement-breakpoint
CREATE TABLE `loadouthistory` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`characterId` text NOT NULL,
	`userId` text NOT NULL,
	`version` integer NOT NULL,
	`loadouts` text NOT NULL,
	`persistent` text NOT NULL,
	`savedDate` text NOT NULL,
	`reason` text NOT NULL
);
--> statement-breakpoint
CREATE INDEX `loadouthistory_character` ON `loadouthistory` (`characterId`,`id`);--> statement-breakpoint
-- Hand-edited: drizzle-kit also emitted a rebuild of `users` and `invitecodes` for the
-- boolean -> integer declared-type drift left by 0009. Dropped on purpose: it rewrites
-- existing rows for no behavioural change (SQLite stores both the same way).
CREATE TRIGGER `inventorylog_no_update` BEFORE UPDATE ON `inventorylog` BEGIN SELECT RAISE(ABORT, 'inventorylog is append-only'); END;--> statement-breakpoint
CREATE TRIGGER `inventorylog_no_delete` BEFORE DELETE ON `inventorylog` BEGIN SELECT RAISE(ABORT, 'inventorylog is append-only'); END;
