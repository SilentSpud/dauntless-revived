# Private Discord status worker

A separate, dependency-free Bun worker checks the loopback backend every five
seconds and edits one Discord message no more than once per 15 seconds. It honors
Discord's 429 retry delay and exhausted-bucket reset headers; transient edit failures
use exponential backoff. It never changes game state or restarts game processes.

Only aggregate availability, player count, local backend response time, backend
uptime/start time and last-check time leave the server. No IPs, account names,
keys, server-supplied titles, errors or logs are published. Discord `<t:...:F>` and
`<t:...:R>` timestamps render in each reader's local time. Local response time is
**not player ping**. Backend availability does not prove every game instance or
the public gateway is reachable.

If the entire VPS or its network goes down, this worker cannot update Discord.
The last-check timestamp will become stale. Independent outage alerts require a
monitor on another host. Do not interpret a stale Online message as current health.

## Windows kit installation

Keep the webhook URL in `C:\DauntlessRevived\data\keys\discord-status.key` on the VPS,
never in Git, a command transcript or a public configuration file. Treat it as a
password. Then, from an elevated PowerShell prompt:

```powershell
C:\DauntlessRevived\bin\Install-DiscordStatus.ps1 -Root C:\DauntlessRevived
```

The source must be available in `app\tools\discord-status.ts`. The installer installs the pinned
Bun in `C:\Program Files\Bun` (checked against its SHA-256 pin), copies the worker to a protected
runtime directory as `worker.ts` (removing the Node version's `worker.mjs`), and starts a boot-triggered LocalService task
named `Dauntless Revived Discord status`. It reads the existing owner key locally
to obtain the authenticated aggregate count. The owner key never goes to Discord.
Only `discord-status\state` is writable by the worker. Error logs contain generic
messages and are bounded to approximately 256 KiB.

The message ID is persisted across restarts. If an initial send has an ambiguous
network failure, posting pauses rather than risking duplicate messages. Check the
channel first, then set `state\message.json` to `{"id":"EXISTING_MESSAGE_ID"}` while
the task is stopped. Only remove the state after confirming no message was created.
A deleted message or revoked webhook also pauses delivery; replace the private
configuration and restart the task after review. Logs: `state\errors.log`.

```powershell
Stop-ScheduledTask -TaskName 'Dauntless Revived Discord status'
Disable-ScheduledTask -TaskName 'Dauntless Revived Discord status'
```

## Memory and matching follow-up

The observed roughly 24–29 MiB JavaScript heap sawtooth is not evidence by itself
of a leak or a long GC pause. Reducing heap limits can increase collection frequency;
raising them does not fix game-process CPU contention. Do not force periodic GC or
change heap sizes without measured pauses. The two-vCPU snapshot's multiple native
hunt processes account for most CPU, not the metagame JavaScript process.

Public-hunt backfill is still outstanding: the metagame drops a queue once it has
allocated a server, and later arrivals start a separate queue. Reusing a hunt must
respect public/private access, capacity and authoritative airship departure state;
blind reuse of the last server would be unsafe. This worker does not implement it.

References: [Discord webhooks](https://docs.discord.com/developers/resources/webhook),
[rate limits](https://docs.discord.com/developers/topics/rate-limits),
[Bun](https://bun.com/docs).
