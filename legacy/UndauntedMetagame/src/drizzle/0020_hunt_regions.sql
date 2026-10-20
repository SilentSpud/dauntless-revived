CREATE TABLE `huntregions` (
    `userId` text PRIMARY KEY NOT NULL REFERENCES `users`(`userId`),
    `region` text NOT NULL CHECK (`region` IN ('main', 'aus'))
);
