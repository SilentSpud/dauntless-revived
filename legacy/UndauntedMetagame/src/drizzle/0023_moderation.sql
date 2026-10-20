CREATE TABLE player_addresses (account_id TEXT NOT NULL, address TEXT NOT NULL, last_seen INTEGER NOT NULL, PRIMARY KEY(account_id,address));
--> statement-breakpoint
CREATE TABLE account_bans (account_id TEXT PRIMARY KEY NOT NULL, reason TEXT NOT NULL, address TEXT, active INTEGER NOT NULL, updated_at INTEGER NOT NULL, actor TEXT NOT NULL);
--> statement-breakpoint
CREATE INDEX active_ban_address ON account_bans(address,active);
--> statement-breakpoint
CREATE TABLE moderation_events (id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, reason TEXT NOT NULL, address TEXT, active INTEGER NOT NULL, created_at INTEGER NOT NULL, actor TEXT NOT NULL);
