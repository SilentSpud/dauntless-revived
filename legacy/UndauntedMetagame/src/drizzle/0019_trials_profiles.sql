CREATE TABLE `leaderboardprofiles` (
    `accountId` text PRIMARY KEY NOT NULL,
    `epicId` text NOT NULL,
    `platformId` text NOT NULL,
    `platform` text NOT NULL,
    `displayName` text NOT NULL,
    `updatedDate` text NOT NULL
);
