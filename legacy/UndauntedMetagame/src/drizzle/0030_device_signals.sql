CREATE TABLE `account_device_signals` (
    `account_id` text NOT NULL,
    `kind` text NOT NULL,
    `signal_hash` text NOT NULL,
    `observed_at` integer NOT NULL,
    PRIMARY KEY (`account_id`, `kind`)
);
--> statement-breakpoint
CREATE TABLE `device_ban_signals` (
    `account_id` text NOT NULL,
    `kind` text NOT NULL,
    `signal_hash` text NOT NULL,
    PRIMARY KEY (`account_id`, `kind`)
);
--> statement-breakpoint
CREATE INDEX `device_ban_signal_match` ON `device_ban_signals` (`kind`, `signal_hash`);
