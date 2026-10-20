CREATE TABLE `discordlinks` (
    `discordId` text PRIMARY KEY NOT NULL,
    `userId` text NOT NULL UNIQUE REFERENCES `users`(`userId`),
    `linkedAt` text NOT NULL
);
