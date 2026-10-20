# Temporary account suspensions

The dashboard defaults to seven days (168 hours), permits 1–720 hours, requires
a reason, and supports revocation. These are operator-issued account suspensions,
not automatic cheat detections. Account-only is the default. The optional IP
checkbox resolves the account's recently observed address from the backend,
ignoring submitted addresses; the backend revalidates the observation at write
time. Missing or expired observations reject the IP request. IP restrictions
share the account row's expiry/revocation and can affect unrelated shared-IP users.
They block backend authentication/access while active; they do not forcibly
disconnect an already-running native hunt.

`BanFor` checks expiry on each access, so no timer or cron job is needed. Existing
permanent bans retain a NULL expiry. History retains the original expiry, reason,
actor account ID and issue/revoke time. Expired suspensions disappear from the
active list without deleting history. Both lists require administrator auth and
page 50 records at a time. UI values use textContent, and responses are not cached.

Migration `0029_temporary_bans` adds nullable expires_at columns to account_bans
and moderation_events. Back up the database using SQLite's backup API and verify
integrity before applying it; back up changed runtime files and configuration too.
Live installations may have a different migration journal numbering: preserve
their existing entries and append this migration with a later timestamp.

Validate backend health, administrator-only history access, seven-day expiry and
dashboard controls after restarting only the metagame and dashboard. No game
worker restart is required for this feature.

Rollback must not restore an old whole database over new player progress. Keep
the added columns, disable the new UI if necessary, and retain the column-aware
moderation writer and expiry predicate. Older INSERT INTO account_bans VALUES
statements assume six columns and are incompatible with the added column; do not
blindly restore that controller. Revoke a mistaken suspension through moderation
so the reversal is audited.

Validation: 758 backend tests, 12 dashboard tests, and backend build passed.
Native RPC hardening is documented separately in anticheat-2026-10-15.md.

## October 15 rollout status

The initial online attempt was unsuccessful. The live database
was approximately 20 GB. An online backup did not finish; a full-step retry
coincided with backend health timeouts. Both backup helpers were stopped, health
returned HTTP 200, and only their incomplete destination files were removed.
Do not retry an unbounded full-step backup during active play. A maintenance
pause and verified backup were used for the subsequent successful deployment.

The live migration journal ended at `0027_us_region`, unlike this checkout's
`0028_us_region`. The staged patch preserves the live journal and appends only
the expiry-column migration; replacing the whole journal would risk replaying
incompatible migrations. The live dashboard also has newer fleet changes, so
the staged patch replaces only its moderation sections.

The maintenance backup passed integrity verification and the expiry migration
and dashboard were deployed. Backend HTTP 200, authenticated history HTTP 200,
unauthenticated history HTTP 401, no-store responses and seven-day default were
verified. Maintenance flag was removed and the supervisor resumed.

Optional IP scope was subsequently deployed as four targeted runtime files,
preserving the live fleet UI and requiring no new migration. Prior files and
configuration are in `backups/ipban-20261015`; the verified database recovery
point is `backups/softban-20261015`. Only metagame and dashboard were restarted;
game processes were retained. Focused validation: 25 backend moderation/access
tests, 13 dashboard tests and backend build passed. IP expiry tests failed before
the change and passed afterward. No real player was banned to test deployment.

HWID enforcement is intentionally omitted by operator choice. The native login
still exchanges a reusable account key directly, so a launcher-only fingerprint
header would be bypassable. See `legacy/UndauntedMetagame/src/routes/eos.ts` and
`legacy/UndauntedLauncher/src/main/launch.ts`; adding a fingerprint alone would provide
no hardware-backed identity assurance.
