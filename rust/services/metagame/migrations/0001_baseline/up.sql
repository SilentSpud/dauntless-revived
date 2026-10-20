CREATE TABLE account_bans (account_id TEXT PRIMARY KEY NOT NULL, reason TEXT NOT NULL, address TEXT, active INTEGER NOT NULL, updated_at INTEGER NOT NULL, actor TEXT NOT NULL, expires_at integer);
CREATE TABLE `account_device_signals` (
    `account_id` text NOT NULL,
    `kind` text NOT NULL,
    `signal_hash` text NOT NULL,
    `observed_at` integer NOT NULL,
    PRIMARY KEY (`account_id`, `kind`)
);
CREATE TABLE `accountkeyrecovery` (`userId` text PRIMARY KEY NOT NULL, `keyHash` text NOT NULL, `ciphertext` text NOT NULL);
CREATE TABLE authepochs (userId TEXT PRIMARY KEY NOT NULL, epoch INTEGER NOT NULL DEFAULT 0);
CREATE TABLE `blocks` (
	`blockerId` text NOT NULL,
	`blockedId` text NOT NULL,
	`createdAt` integer NOT NULL,
	PRIMARY KEY(`blockerId`, `blockedId`)
);
CREATE TABLE `bounties` (
	`accountId` text NOT NULL,
	`bountyId` text NOT NULL,
	`slotIndex` integer,
	`updateVersion` integer NOT NULL,
	`data` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `bountyId`)
);
CREATE TABLE `bountydraft` (
	`accountId` text PRIMARY KEY NOT NULL,
	`data` text NOT NULL,
	`updatedDate` text NOT NULL
);
CREATE TABLE `breadcrumbs` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`breadcrumbs` text NOT NULL,
	`updateVersion` integer NOT NULL
);
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
CREATE TABLE `characters` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`createdDate` text NOT NULL,
	`lastModifiedDate` text NOT NULL,
	`name` text NOT NULL,
	`updateVersion` integer NOT NULL,
	`data` text NOT NULL
);
CREATE TABLE `cooldowns` (
	`accountId` text NOT NULL,
	`cooldownId` text NOT NULL,
	`startedDate` text NOT NULL,
	`updatedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `cooldownId`)
);
CREATE TABLE `device_ban_signals` (
    `account_id` text NOT NULL,
    `kind` text NOT NULL,
    `signal_hash` text NOT NULL,
    PRIMARY KEY (`account_id`, `kind`)
);
CREATE TABLE `discordlinks` (
    `discordId` text PRIMARY KEY NOT NULL,
    `userId` text NOT NULL UNIQUE REFERENCES `users`(`userId`),
    `linkedAt` text NOT NULL
);
CREATE TABLE `encounteredcontent` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`encounteredcontent` text NOT NULL
);
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
CREATE TABLE `escalationtalents` (
	`accountId` text NOT NULL,
	`seasonId` text NOT NULL,
	`talentId` text NOT NULL,
	`rank` integer NOT NULL,
	PRIMARY KEY(`accountId`, `seasonId`, `talentId`)
);
CREATE TABLE `escalationunlocks` (
	`accountId` text NOT NULL,
	`seasonId` text NOT NULL,
	`unlockId` text NOT NULL,
	`collectedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `seasonId`, `unlockId`)
);
CREATE TABLE `friendships` (
	`userLow` text NOT NULL,
	`userHigh` text NOT NULL,
	`requesterId` text NOT NULL,
	`status` text NOT NULL,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL,
	PRIMARY KEY(`userLow`, `userHigh`)
);
CREATE TABLE `gameserverapikeys` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`keyHash` text
);
CREATE TABLE `gameserverapikeystoregister` (
	`key` text PRIMARY KEY NOT NULL
);
CREATE TABLE `guildinvites` (
	`inviteId` text PRIMARY KEY NOT NULL,
	`guildId` text NOT NULL,
	`inviteeId` text NOT NULL,
	`inviterId` text NOT NULL,
	`createdAt` integer NOT NULL,
	`expiresAt` integer NOT NULL
);
CREATE TABLE `guildmembers` (
	`accountId` text PRIMARY KEY NOT NULL,
	`guildId` text NOT NULL,
	`rank` text NOT NULL,
	`joinedAt` integer NOT NULL,
	`updatedAt` integer NOT NULL
);
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
CREATE TABLE `huntpassselection` (
	`accountId` text PRIMARY KEY NOT NULL,
	`progressionId` text NOT NULL,
	`updatedDate` text NOT NULL
);
CREATE TABLE "huntregions" (
    `userId` text PRIMARY KEY NOT NULL REFERENCES `users`(`userId`),
    `region` text NOT NULL CHECK (`region` IN ('main', 'aus', 'ger', 'us'))
);
CREATE TABLE `inventories` (
	`characterId` text PRIMARY KEY NOT NULL,
	`instancedItems` text NOT NULL,
	`stackedItems` text NOT NULL
);
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
CREATE TABLE `invitecodes` (
	`invitecode` text PRIMARY KEY NOT NULL,
	`usesRemaining` integer NOT NULL,
	`infiniteUses` boolean NOT NULL
);
CREATE TABLE `leaderboardprofiles` (
    `accountId` text PRIMARY KEY NOT NULL,
    `epicId` text NOT NULL,
    `platformId` text NOT NULL,
    `platform` text NOT NULL,
    `displayName` text NOT NULL,
    `updatedDate` text NOT NULL
);
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
CREATE TABLE `loadouts` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`loadouts` text NOT NULL
, `persistent` text NOT NULL default '{}');
CREATE TABLE `loadoutslots` (
	`characterId` text PRIMARY KEY NOT NULL,
	`userId` text NOT NULL,
	`numCharacterSlots` integer NOT NULL,
	`activeIndex` integer NOT NULL,
	`updatedDate` text NOT NULL
);
CREATE TABLE moderation_events (id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, reason TEXT NOT NULL, address TEXT, active INTEGER NOT NULL, created_at INTEGER NOT NULL, actor TEXT NOT NULL, expires_at integer);
CREATE TABLE `objectives` (
	`accountId` text NOT NULL,
	`objectiveId` text NOT NULL,
	`progress` integer NOT NULL,
	`completedCount` integer NOT NULL,
	`createdDate` text NOT NULL,
	`lastModifiedDate` text NOT NULL,
	PRIMARY KEY(`accountId`, `objectiveId`)
);
CREATE TABLE `onetimejobs` (
    `jobId` text PRIMARY KEY NOT NULL,
    `doneDate` text NOT NULL
);
CREATE TABLE player_addresses (account_id TEXT NOT NULL, address TEXT NOT NULL, last_seen INTEGER NOT NULL, PRIMARY KEY(account_id,address));
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
CREATE TABLE recoverychallenges (tokenHash TEXT PRIMARY KEY NOT NULL, discordId TEXT NOT NULL, userId TEXT, keyHash TEXT, expiresAt INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0);
CREATE TABLE recoveryevents (id INTEGER PRIMARY KEY AUTOINCREMENT, discordId TEXT NOT NULL, action TEXT NOT NULL, createdAt INTEGER NOT NULL);
CREATE TABLE recoveryproofs (discordId TEXT PRIMARY KEY NOT NULL, userId TEXT NOT NULL UNIQUE, keyHash TEXT NOT NULL);
CREATE TABLE `slayerlinkinvites` (
	`inviteId` text PRIMARY KEY NOT NULL,
	`senderId` text NOT NULL,
	`targetId` text NOT NULL,
	`senderSlot` integer NOT NULL,
	`createdAt` integer NOT NULL,
	`expiresAt` integer NOT NULL,
	`status` text NOT NULL
);
CREATE TABLE `slayerlinks` (
	`linkId` text PRIMARY KEY NOT NULL,
	`senderId` text NOT NULL,
	`targetId` text NOT NULL,
	`senderSlot` integer NOT NULL,
	`targetSlot` integer NOT NULL,
	`createdAt` integer NOT NULL,
	`endsAt` integer NOT NULL
);
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
CREATE TABLE `trialweeks` (
    `week` integer PRIMARY KEY NOT NULL,
    `finalizedDate` text NOT NULL,
    `awardedAccounts` integer NOT NULL
);
CREATE TABLE `userapikeys` (
	`userId` text PRIMARY KEY NOT NULL,
	`keyHash` text NOT NULL
);
CREATE TABLE `userapikeystoregister` (
	`userId` text PRIMARY KEY NOT NULL,
	`key` text NOT NULL
);
CREATE TABLE "users" (
	`userId` text PRIMARY KEY NOT NULL,
	`name` text NOT NULL,
	`notes` integer NOT NULL,
	`isAdmin` boolean DEFAULT 0 NOT NULL
);
CREATE INDEX active_ban_address ON account_bans(address,active);
CREATE INDEX `blocks_blocked` ON `blocks` (`blockedId`);
CREATE INDEX `characterhistory_character` ON `characterhistory` (`characterId`,`id`);
CREATE INDEX `device_ban_signal_match` ON `device_ban_signals` (`kind`, `signal_hash`);
CREATE INDEX `friendships_high` ON `friendships` (`userHigh`);
CREATE INDEX `guildinvites_invitee` ON `guildinvites` (`inviteeId`);
CREATE UNIQUE INDEX `guildinvites_pair` ON `guildinvites` (`guildId`,`inviteeId`);
CREATE INDEX `guildmembers_guild` ON `guildmembers` (`guildId`);
CREATE UNIQUE INDEX `guilds_name_key` ON `guilds` (`nameKey`);
CREATE UNIQUE INDEX `guilds_nameplate_key` ON `guilds` (`nameplateKey`);
CREATE INDEX `inventorylog_character` ON `inventorylog` (`characterId`,`id`);
CREATE UNIQUE INDEX `inventorytransactions_character_transaction_request` ON `inventorytransactions` (`characterId`,`transactionId`,`requestHash`);
CREATE INDEX `inventorytransactions_created_date` ON `inventorytransactions` (`createdDate`);
CREATE INDEX `loadouthistory_character` ON `loadouthistory` (`characterId`,`id`);
CREATE INDEX `progression_events_account` ON `progression_events` (`accountId`,`id`);
CREATE INDEX recoveryevents_actor_time ON recoveryevents(discordId,createdAt);
CREATE UNIQUE INDEX `slayerlinkinvites_pending_pair` ON `slayerlinkinvites` (`senderId`,`targetId`) WHERE "status" = 'PENDING';
CREATE INDEX `slayerlinkinvites_sender` ON `slayerlinkinvites` (`senderId`);
CREATE INDEX `slayerlinkinvites_target` ON `slayerlinkinvites` (`targetId`);
CREATE INDEX `slayerlinks_sender` ON `slayerlinks` (`senderId`);
CREATE INDEX `slayerlinks_target` ON `slayerlinks` (`targetId`);
CREATE INDEX `storepurchases_account` ON `storepurchases` (`accountId`);
CREATE INDEX `trialruns_board` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`mode`,`completionTime`);
CREATE INDEX `trialruns_group` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`mode`,`groupKey`);
CREATE UNIQUE INDEX `trialruns_week_trial_difficulty_run` ON `trialruns` (`trialWeek`,`trialId`,`difficulty`,`runKey`);
CREATE TRIGGER `inventorylog_no_delete` BEFORE DELETE ON `inventorylog` BEGIN SELECT RAISE(ABORT, 'inventorylog is append-only'); END;
CREATE TRIGGER `inventorylog_no_update` BEFORE UPDATE ON `inventorylog` BEGIN SELECT RAISE(ABORT, 'inventorylog is append-only'); END;
CREATE TRIGGER `progression_events_no_delete` BEFORE DELETE ON `progression_events` BEGIN SELECT RAISE(ABORT, 'progression_events is append-only'); END;
CREATE TRIGGER `progression_events_no_update` BEFORE UPDATE ON `progression_events` BEGIN SELECT RAISE(ABORT, 'progression_events is append-only'); END;
