ALTER TABLE account_bans ADD COLUMN expires_at integer;
--> statement-breakpoint
ALTER TABLE moderation_events ADD COLUMN expires_at integer;
