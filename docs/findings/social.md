---
title: Friends, parties and guilds
parent: Findings
nav_order: 9
description: "How the Dauntless 1.4.4 client does friends, parties, guilds and Slayer Links against its backend, read from the executable and the live server: the identity chain, the exact replies, why the first two-player test showed nothing, and what is still unconfirmed."
lang: en
ref: findings/social
---

{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign contract_page = site.pages | where: "path", "findings/backend-contract.md" | first %}
{% assign rev_page = site.pages | where: "path", "findings/json-reversing.md" | first %}
{% assign awakening_page = site.pages | where: "path", "findings/awakening-2-1-1.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign chat_page = site.pages | where: "path", "findings/chat.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "findings/harmonic-fork.md" | first %}

# Friends, parties and guilds in 1.4.4
{: .no_toc }

This page describes how the **1.4.4** client handles the Social panel: friends, blocked players,
parties and party invites, guilds and Slayer Links. It records what the client sends, what it needs
back, and why the first test with two players on 22 September 2026 showed nothing. It also lists the
fixes we built in the metagame.

**Status (22 September 2026): built, tested without the game and running on our rented server (60955e1), not yet tried by two players.** Every
fix below passes HTTP tests that replay the client's own requests and check each reply against a model
of the client's parsing. The next two-player test on the rented server will confirm or correct them;
[How to verify](#how-to-verify) lists its steps and the log lines to expect. Text chat is built too,
and has a page of its own: [Text chat]({{ chat_page.url | relative_url }}).

**Update (23 September 2026), from the port of Harmonic's 1.4.4 fork
([The Harmonic port]({{ harmonic_page.url | relative_url }})), built and tested without the game, not
deployed yet:** [Slayer Links](#slayer-links) (on by default), friends' online status inside the chat
server (off by default, [Text chat]({{ chat_page.url | relative_url }}#presence)), a
[party accept that arrives twice](#a-repeated-accept) now succeeds, and the client's
[session check](#verify) names the player's own account.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Evidence and confidence

Each fact is marked with where it comes from and how sure we are.

| Label | Source |
|:------|:-------|
| **B** | The 1.4.4 executable (`Dauntless-Win64-Shipping.exe`, 103 MB): its strings (ASCII and UTF-16), and disassembly of the functions that build requests and parse replies. Addresses are virtual addresses with image base `0x140000000`. |
| **L** | The live server: a census of every route the real clients and game servers called on 22 September 2026, with counts and unanswered routes, and what the two players reported. |
| **R** | Requests the 2.1.1 client sent to our research server (the same Phoenix services, recorded with their bodies). |
| **K** | Reflected class and struct names (the SDK dump of the game's classes), and the endpoint table in `native/internal-server/dllmain.cpp`. |
| **C** | Our own code and tests. |
| **G** | A design decision of ours, where nothing in the client decides it. |

Confidence: **H** high (read in the code, or seen live), **M** medium (one link not traced), **L** low.

The method for reading JSON field names out of the executable is on
[Reading the JSON contract from the binary]({{ rev_page.url | relative_url }}).

## What the players saw, and why

| What the players saw | Cause | Fix | Conf |
|:---------------------|:------|:----|:-----|
| A party invite reached the other player's client (the invite poll answered it twice) but never appeared under PARTY INVITES. | Before the panel shows an invite, the client asks `POST /accountinfo/public` about the sender. Our reply, inherited from upstream, described the **caller** instead: the caller's id in `accountId` and in `linkedAccounts`. The client files user info under the reply's `accountId` and keeps the first reply per id, so the sender never got any user info, was invalidated, and the invite was dropped (B `0x140b74870`, `0x1409ede50`). | `/accountinfo/public` answers for the asked account. | H in the code, M that nothing else stands in the way |
| Add Friends "found nothing". | Add Friends is not a search. It is a name box and an Add button. The name was found (`GET /account/api/public/account/displayName/<name>`), but the next step, `POST /account/mapping`, first got 404 and then a reply shape the client does not read. With no mapping, the client drops the friend request without a message. No friend request ever reached the server (0 calls, L). | `/account/mapping` answers in the shape the client parses. | H |
| Everyone, the players themselves included, showed as Offline. | Online status comes only from XMPP presence, pushed over the chat connection. There is no HTTP presence route, and our chat server sent no presence outside chat rooms. | Built on 23 September 2026, on by default since 30 September: friends' presence in the chat server (`CHAT=1` and `CHAT_PRESENCE=1`, [Text chat]({{ chat_page.url | relative_url }}#presence)), waiting for the two-player test that the party's automatic kick stays asleep. | H for the cause, M for the fix |
| "Unable to create guild." | `POST /guild/validate` (from the client) and `POST /guild` (from the **game server**) both got 404. | The eleven v2 guild routes. | H |
| Nobody used the in-game party Invite (both invites of the night came from the host's fallback). | The players could not find each other: Hunt Members and friends need the same user-info step as an invite's sender. | Same fix as the first row. | M |

**After the fixes are deployed, both players must restart the game once.** The client keeps the
first user info and the first mapping per id for the whole session, so wrong entries from before stay
until it restarts.

**A correction.** Our earlier notes, and roadmap item 1.11, blamed `/account/mapping` for both the
friend search and the party invite. That is only half right. Add Friends did stop at
`/account/mapping` (and would next have stopped at `/accountinfo/public`). The received party invite
never uses the mapping: its sender id is already a Phoenix id, and it was dropped at
`/accountinfo/public`. The mapping calls seen around the invite that night fit other callers: the
two Add Friends attempts and the client's own mapping of the local player at login. That login call
rests on the executable's log string and the 2.1.1 capture (B, R); the 1.4.4 census shows only 6
mapping calls for 8 logins and 2 Add Friends attempts (L), so it does not happen at every login. The
live test counts them (see [How to verify](#how-to-verify)).

## One id, two steps: the identity chain

Every entry in every Social list (friends, friend requests, blocked players, party members, Hunt
Members, party invites, guild members, guild inviters) is a "social user" keyed by its **Phoenix**
account id (B `0x1415af620`). It only appears once two lookups have succeeded:

```
Epic id (friends list, block list, Add Friends, /invite <name>, the guild add-member box)
   |  POST /account/mapping {"srcAccountType": "epic", "ids": [<epic id>]}      -> Phoenix id
   v
Phoenix id (party invite sender, party members, Hunt Members, guild members and inviters)
   |  POST /accountinfo/public {"accountId": <phoenix id>}                      -> user info
   |     the reply's accountId must be the asked id (cache key, first reply wins)
   |     linkedAccounts [{accountType: "epic", accountId: X}] sets the user's Epic id to X
   v
the social toolkit checks that the user's Epic id is the Epic id the action started from
   v
the entry is shown, or the queued action runs (send the friend request, show the invite ...)
```

When the mapping yields nothing, the action is dropped quietly (B, log string "Mapping primary Id for
unknown, unmapped external Id [%s] for user action"). When the user info yields nothing, the user is
invalidated (B, "SocialToolkit - HandleUserInvalidated called for [%s]").

**On this server every account has exactly one id**, `UID-<uuid>`. It is at once the Epic account id
(the login answers it as `account_id`), the Phoenix account id (party and guild member ids) and the
chat id. So every mapping is the identity. In the original service the Epic id was a separate
32-character id (R: the 2.1.1 client mapped such an id at login), and the client still walks both
steps, so both must answer correctly even though the ids are equal.

### `POST /account/mapping`

The client's `QueryAccountMappingsEndpoint` (K `dllmain.cpp`), in Phoenix's own user service.

- **Request** (B, R, L): `{"srcAccountType": "epic", "ids": ["<id>"]}`, JSON with the player's bearer
  token, at most 100 ids per request. The 2.1.1 client sent exactly this at login (R), and so did the
  live 1.4.4 clients (L), though not at every login (6 calls in 8 logins, 2 of them Add Friends).
- **Reply the parser reads** (B `0x140b09f60`..`0x140b0afd4`): a root key `accountMappings` holding an
  **object keyed by each asked id**. Each value is an object with non-empty strings `accountId` and
  `accountType`; `accountType` is compared without case with `epic` and `phoenix`. Nothing else in an
  entry is read. The HTTP status is not checked; an empty body fails ("Empty response payload") and so
  does invalid JSON ("Invalid response payload"). Extra root keys are ignored.
- **Our reply**: `{"accountMappings": {"<id>": {"accountId": "<id>", "accountType": "phoenix"}}, "code": "OK",
  "message": "", "payload": {"accountMappings": {...}}}`. The wrapped copy under `payload` costs nothing
  and covers a reader that expects the Phoenix envelope. For `srcAccountType` `phoenix` the entries say
  `epic`. Unknown ids are left out; without a valid token the map is empty.
- **Why the two earlier replies failed**: the first was an object keyed by id at the root, with no
  `accountMappings` key, so the parser found nothing. The second sent `accountMappings` as an **array**,
  which the parser reads as an empty object. Either way no mapping was cached. Our client model
  reproduces both failures (see [Testing without the game](#testing-without-the-game)).
- Confidence **H** for the shape (the parser was traced end to end); not yet seen working in game.

### `POST /accountinfo/public`

The client's `PublicAccountInfoEndpoint`, one call per player to show (73 calls live, all answered,
L). This is the step every other player in the Social panel depends on.

- **Request** (B, R): `{"accountId": "<id>"}`, or from a second request builder `{"displayname": "<name>"}`.
- **Reply** (B `0x140b69ac0`, a plain object, no envelope, like the other account services on
  [Backend contract]({{ contract_page.url | relative_url }})):

  ```json
  {
    "accountId": "UID-B",
    "username": "Bravo",
    "linkedAccounts": [ { "accountId": "UID-B", "accountType": "epic" } ],
    "isSubscribed": true,
    "language": null
  }
  ```

- `accountId` must be the **looked-up** account: it is the key the client files the user info under,
  and the first reply per key wins (B `0x140b74870`, the early return at `0x140b748f9`).
- The `epic` entry of `linkedAccounts` becomes the user's Epic id (B `0x140b7499f`), which the
  toolkit compares with the Epic id the action started from.
- An unknown id or name answers **404 `{}`**: the client counts a failed lookup and caches nothing (B,
  "Batch Succeeded, Total = %d, FailureCount = %d"). With the old reply it cached nothing useful either.
- `isSubscribed` and `language` are not read by the client; they stay because they were harmless live.
- **Why upstream's reply failed**: it put the caller's own id in `accountId` and `linkedAccounts`. At
  login the client looks up its own id first, so every later lookup of another player landed on the
  caller's existing entry and was ignored. The other player never got user info.
- Confidence **H** for the shape and the cache keying; **M** that this alone makes invite senders,
  Hunt Members and friends appear.

### Lookups by name (unchanged)

| Route | Used by | Conf |
|:------|:--------|:-----|
| `GET /account/api/public/account/displayName/:name` → `{id, displayName, externalAuths: {}}` or 404 | Add Friends, the chat's `/invite <name>`, the guild add-member box (B, L: 2 calls) | M |
| `GET /account/api/public/account?accountId=A&accountId=B` → an array of the same | The Epic-side name of a social user (B) | M |

A typed name that contains `@` goes to an email lookup, which stays unanswered: our usernames cannot
contain `@`, and the 404 gives the normal "not found" message.

## Friends

The Epic friends service (`[OnlineSubsystemMcp.OnlineFriendsMcp]`, B), with our player token as the
bearer. Friendships and blocks are stored in SQLite (built for roadmap 1.9). The routes are on
[HTTP API]({{ api_page.url | relative_url }}#friends).

| Call | When | Request and reply | Evidence | Conf |
|:-----|:-----|:------------------|:---------|:-----|
| `GET /friends/api/public/friends/:id?includePending=true` | once per login | A bare **array** of `{accountId, status: ACCEPTED or PENDING, direction: INBOUND or OUTBOUND, created}` (the client wraps it as `{"friends": ...}` itself) | B, L: 8 logins, 8 calls | H |
| `GET /friends/api/public/blocklist/:id` | once per login | `{"blockedUsers": [...]}` | B, L | H |
| `POST /friends/api/public/friends/:me/:them` | Add Friends, the Add Friend menu item, Accept | no body (Accept sends an empty JSON body); any 2xx | B | H |
| `DELETE /friends/api/public/friends/:me/:them` | Decline, Remove Friend | any 2xx | B | H |
| `POST` (and now also `PUT`) `/friends/api/public/blocklist/:me/:them` | Block, after its dialog | any 2xx | B, the verb inferred | M |
| `DELETE /friends/api/public/blocklist/:me/:them` | Unblock | any 2xx | B | H |

The client reads both lists **only at login** (L). A friend request or an accept therefore shows for
the other player at their next login. With friends' online status on (`CHAT=1` and
`CHAT_PRESENCE=1`), an accept is also pushed at once over the chat connection
([Text chat]({{ chat_page.url | relative_url }}#presence)); a new request is not.

**What the player should see** (M unless marked):

| Step | Calls | Result |
|:-----|:------|:-------|
| Log in | friends list, block list; per listed id: mapping, user info | Accepted friends under OFFLINE (never under EPIC FRIENDS without presence); a pending request someone sent, with Accept and Decline; blocked players under BLOCKED; requests you sent are not listed |
| Add Friends: type a name, press Add | name lookup (404: the "unknown player" message; your own name: the "you cannot invite yourself" message), mapping, user info, `POST .../friends/<me>/<them>` | a "friend invite sent" message; nothing new in the lists |
| The other player logs in again | the login calls | the request, with Accept and Decline (where exactly it is listed is still to be seen) |
| Accept | `POST .../friends/<me>/<them>` | the friend shows under OFFLINE after the next login |
| Decline, Remove Friend | `DELETE .../friends/<me>/<them>` | the entry goes (H) |
| Block, Unblock | `POST`/`DELETE .../blocklist/<me>/<them>` | the entry moves to or from BLOCKED |

**Limits we added** (G): at most 50 unanswered requests sent per account, and at most 20 new requests
per 10 minutes (409, which the client shows as a failure). Accepting a request is never limited. The
existing limits stay: 200 friendships and 200 blocks per account. A block also removes the party and
guild invites pending between the two players (G), and the invite lists leave out any invite between
players who blocked each other.

## Parties

The party service was already built (roadmap 1.9) and matched the client's requests and replies. The
live census shows it working: 399 party polls and 399 invite polls, and the invite delivered to the
other player's poll (L). **The only server change parties needed is the `/accountinfo/public` fix**;
since 23 September 2026 [a repeated accept](#a-repeated-accept) is also tolerated.

| Call | When | Evidence | Conf |
|:-----|:-----|:---------|:-----|
| `POST /party` `{buildId, featureOverrides: []}` | the poll, about every 10 seconds, and at login | B `0x140b57600`, L: 399 calls | H |
| `GET /party/invites` → `{"invitations": [{recipientPlayerId, sendingPlayerId, partyId, sendingPlatform, sendingDisplayName}]}` | the invite poll | B `0x140b675d0`, L: 399 calls | H |
| `PUT /party/invite` `{recipientPlayerId, partyId, buildId, featureOverrides}` | Invite to Party; the chat's `/invite <name>` | B | H |
| `PUT /party/invite/accept/:partyId` | Accept; the id is the party's, not an invite id (B `0x140b35384`) | B | H |
| `DELETE /party/invite` | Decline | B | H |
| `DELETE /party/member`, `DELETE /party/member/:id`, `PUT /party/member/promote/:id` | leave (also at every login), kick, promote | B, L: 12 leaves in 8 logins | H |
| `POST /candidate/join` from the leader | the leader picks a hunt; the members follow through their party poll | C, tests | M |

**What the player should see** once the fix is deployed: an invite appears under PARTY INVITES
within about 10 seconds (the next invite poll), after the client has looked up the sender. Accepting
shows both names in both party panels. When the leader picks a hunt, the whole party lands on one
hunt server. Parties live in memory: a metagame restart leaves everyone in a party of one, which the
client treats as normal.

Two things we deliberately left alone: invites the caller **sent** are not listed in
`GET /party/invites` (the client might misread them as received), and a party of one keeps the old
placeholder values, which were harmless live for queueing hunts.

**One risk for the in-game invite: the placeholder may read as "matchmaking".** Before it sends
`PUT /party/invite`, the client refuses when the inviter is the invitee, is not the leader, or has a
party that is not idle (B `0x1415b2280`; the last check calls `0x1415a98c0` at `0x1415b27aa` and
logs "Player %s tried to send an invite to player %s, but party %s was matchmaking"). That state is
read from the party's candidate, and the placeholder says `QUEUED_FOR_START` with a candidate id. Solo
players queued hunts without trouble with it (L: 27 queued joins), and one link of the state reading
was not traced, so whether it greys out Invite to Party is open (M). Nobody used the in-game invite
live (L: 0 `PUT /party/invite`), so nothing contradicts it either. If it does, `PARTY_SOLO_STUB=0`
answers a party of one with no candidate (`candidateState: null`, which reads as idle) instead; see
[Configuration]({{ config_page.url | relative_url }}#metagame-social).

**Limits we added** (G): a player sends at most 20 invites in 10 minutes, and after a player declines
someone's invite, that sender cannot invite them again for 2 minutes (both 409, a failure to the
client). A block removes the pending invites between the two.

### A repeated accept {#a-repeated-accept}

An accept can arrive twice: the client retries a request whose answer was lost, and Harmonic's fork
tolerated that (G for how often it happens in 1.4.4). Since 23 September 2026, a
`PUT /party/invite/accept/<id>` that matches no live invite still answers **200 with the caller's
party** (the same body as the party poll) when the caller is already in a party of two or more and the
id is that party's id, or the id of another member of it (for example the invite's sender). The log
line is `party: accept by <A> id=<id>: already in P=<party> size=<n>; answering that party (a repeated
accept)`. A party of one, a party the caller has left, the caller's own id and a stranger's id still get
404, as before. No switch: it only answers where the reply used to be 404.

### The automatic kick {#the-automatic-kick}

The client's automatic kick of "offline" party members never runs without presence (B `0x1415f6f60`, a
10-second threshold). The chat server keeps it that way: by default it sends no presence outside chat
rooms at all, and with friends' online status switched on (`CHAT_PRESENCE=1`) it still never sends a
player a presence from their own account, not even from their second session
([Text chat]({{ chat_page.url | relative_url }}#party-safety)).

## Guilds

### The contract

- **Endpoints** (K `dllmain.cpp`, the cooked `DefaultGame.ini`): eleven `*_v2` keys. The older `v1`
  keys are compiled in but never called, so we built only v2.
- **Every reply** is read through the Phoenix envelope `{"code": string, "message": string, "payload":
  object}` (B `0x140b12170`). Success is a 2xx status **and** a parseable JSON body (B `0x140aae300`),
  so a 204 is a failure. `GET /guild`'s 204 for "not in a guild" relies on exactly that: the client then
  clears its guild (B `0x1415c2c90`; L: 34 calls answered 204).
- **On an error status** the client still reads `code` and maps it to its guild error (B
  `0x140ade3f0`, K `EGuildRequestError`). An unknown or empty code shows as "Unable to create guild."
- **Payload fields** are strings, except `maximum_guild_members` (a whole number). Ranks are `Member`,
  `Officer` and `Leader`, compared without case; the client puts the rank in the URL in lower case.
- **Creation goes through the game server.** The Create button sends the RPC `ServerCreateGuild` to the
  Ramsgate game server (B, K), which sends `POST /guild` with its key (L: one such call, answered 404).
  The RPC's validation returns true without checking any item, so **creating a guild costs nothing** in
  1.4.4.
- There is no message of the day, banner, hall, perk or guild XP in v2. Guild chat is the XMPP room
  `Guild-<guildId>` (built, for the guild's members only: [Text chat]({{ chat_page.url | relative_url }}#rooms)). The `[TAG]` over players' heads works by itself once `GET /guild`
  returns a nameplate.

We send every reply wrapped and also copy the payload's fields to the root (G). The envelope reader
ignores extra root keys, so the copies cost nothing and cover a flat reader in case our reading of the
code is wrong.

### Error codes

The server sends the `code`; the client shows its own text (from the create-guild widget).

| `code` | Client error | Status we send | Text the client shows |
|:-------|:-------------|:---------------|:----------------------|
| `SlyAdorableQuillshot` | InvalidPermission | 403 | |
| `ExcludedAdorableQuillshot` | NotInAGuild | 404 | |
| `ObedientAdorableQuillshot` | GuildNameInvalidLength | 400 | Name is invalid. Must contain 4-15 english letters and digits. |
| `SeizedAdorableQuillshot` | GuildNameTaken | 409 | Guild name already in use. |
| `NastyAdorableQuillshot` | GuildNameProfane | 400 | Guild name contains profanity. |
| `NumberedAdorableQuillshot` | GuildNameTooManyNumbers | 400 | Guild name must have 6 numbers or less. |
| `LetteredAdorableQuillshot` | GuildNameTooManyLetters | 400 | Guild name must not have more than 6 of the same letter in a row. |
| `DutifulAdorableQuillshot` | GuildNameplateInvalidLength | 400 | Nameplate is invalid. Must contain 2-6 english letters and digits. |
| `CapturedAdorableQuillshot` | GuildNameplateTaken | 409 | Guild nameplate already in use. |
| `DirtyAdorableQuillshot` | GuildNameplateProfane | 400 | Guild nameplate contains profanity. |
| `OccupiedAdorableQuillshot` | YouAlreadyInAGuild | 409 | Unable to create guild. You are already in a guild. |
| `ClonedAdorableQuillshot` | TargetAlreadyInYourGuild | 409 | |
| `RedundantAdorableQuillshot` | TargetAlreadyHasGuildInvite | 409 | |
| `StuffedAdorableQuillshot` | GuildIsFull | 409 | |
| `UninvitedAdorableQuillshot` | GuildInviteNotFound | 404 | |
| `ChiefAdorableQuillshot` | GuildLeaderCannotLeaveGuild | 409 | |
| `DocileAdorableQuillshot` | InvalidGuildRank | 400 | |
| `""` (empty) | Unknown | as fits | Unable to create guild. |

The seventeen code strings sit together in the executable (B, file offsets `0x447b950` to
`0x447bca0`), in the same order as the error enum in the SDK (K). The pairing comes from the mapper
(B, H). The name rules come from the texts above and from the client's own length limits (4 and 15
for names, 2 and 6 for nameplates, B). The executable also holds older texts with other limits ("4-32"
and "2-7"); they belong to the unused v1 code.

### Name and nameplate rules

Checked in this order; the first failure decides.

1. The caller (for a create, the leader) is not already in a guild: `Occupied`.
2. Name: 4 to 15 English letters and digits, nothing else (a space is invalid): `Obedient`.
3. Name: at most 6 digits: `Numbered`.
4. Name: at most 6 of the same letter in a row, regardless of case: `Lettered`.
5. Name: no word from the deny list (G): `Nasty`.
6. Name: not taken, regardless of case (G): `Seized`.
7. Nameplate: empty, or 2 to 6 English letters and digits: `Dutiful`. An empty nameplate is allowed
   (the client skips its own check when it is empty, B), and several guilds may have none.
8. Nameplate: no word from the deny list: `Dirty`.
9. Nameplate: not taken, regardless of case: `Captured`.

The client never calls the profanity service (the cooked config turns it off), so the server keeps a
short built-in deny list; `GUILD_NAME_DENYLIST` adds words (see
[Configuration]({{ config_page.url | relative_url }})). A few short offensive words are refused as the
whole name or nameplate, with the same codes. **Reserved words** (G) keep a guild from posing as the
server's staff or the project: `admin`, `moderator`, `official`, `staff` and a few more anywhere, and
nameplates such as `GM`, `DEV` and `MOD`. They answer "already in use" (`Seized` or `Captured`);
`GUILD_RESERVED_NAMES=0` allows them. The full lists are on
[HTTP API]({{ api_page.url | relative_url }}#guilds).

### The routes

| Route (endpoint key) | Caller | What it does | Conf |
|:---------------------|:-------|:-------------|:-----|
| `GET /guild` (`GuildEndpoint_v2`) | the client, at login, at each world load and after every guild action (L: 34 calls in 8 logins) | the caller's guild, or 204 | H |
| `GET /guild/invite/player` (`GuildViewInvitesEndpoint_v2`) | the same moments | the caller's open invites: `{id, guild_id, guild_name, inviter_account_id}` | H |
| `POST /guild/validate` (`GuildCreateValidateEndpoint_v2`) | the client, while typing in CREATE A GUILD (L: 2 calls) | `{leader_account_id, name, nameplate}`: checks the rules | H |
| `POST /guild` (`GuildEndpoint_v2`) | the **game server**, after `ServerCreateGuild` (L: 1 call) | the create; answers the new guild, which the client applies straight away | H |
| `DELETE /guild/:guildId` (`GuildDisbandEndpoint_v2`) | DISBAND GUILD, leader only | removes the guild | H |
| `PUT /guild/invite/:accountId` (`GuildInviteEndpoint_v2`) | the add-member box, "Invite to Guild" in any player's menu | invites; Leader or Officer | H |
| `POST /guild/invite/accept/:guild_invite_id` | Accept Guild Invite | joins as a Member | H |
| `DELETE /guild/invite/:guild_invite_id` | Decline Guild Invite | removes the invite | H |
| `DELETE /guild/player` (`GuildLeaveEndpoint_v2`) | Leave Guild | leaves; not the leader | H |
| `DELETE /guild/player/:accountId` (`GuildKickEndpoint_v2`) | Kick From Guild, leader only | removes a member | H |
| `PUT /guild/rank/:accountId/:rank` (`GuildChangeRankEndpoint_v2`) | Promote To Guild Officer (`officer`), Demote To Guild Member (`member`), Promote To Guild Leader (`leader`) | changes a rank; `leader` hands the guild over | H (the route), M (the old leader becoming an Officer is our choice) |

The guild object in the replies is `{id, name, nameplate, leader_account_id, members: [{phx_account_id,
rank}], maximum_guild_members}` (B `0x140b13de0`, K `FGuildData`). The exact checks and codes of each
route are on [HTTP API]({{ api_page.url | relative_url }}#guilds).

### Creating a guild through the game server

The Create button sends the RPC `ServerCreateGuild(LeaderPlayerId, name, nameplate)` to the Ramsgate
game server (K `Archon_parameters.hpp`), whose validation returns true, and the game server sends
`POST /guild` with `leader_account_id` set to that id. **The game server sends no token of the
player.** `CreateGuild` (B `0x140ac7270`) takes its token at `0x140ac78a4` from `0x140b461d0`, which
asks the subsystem's identity interface for the token of the subsystem's **own** local user
(`Subsystem+0x2c0`); the request gets an `Authorization` header only when that token is not empty
(`0x140b3b561`). Game servers never log in to Phoenix (L: every `POST /login` came from a client), so
normally the create carries no token, and if one ever did, it would be the game server's own, the same
for every player on it (H). So the leader id is only what some client claimed, and the metagame ties
the create to the leader's own action instead (G):

- `POST /guild` accepts only the game-server key, only from this machine. A player's token alone is
  refused.
- A bearer token that comes along is only logged ("the game server's token names ...", and the
  created line ends "a token of X came along" or "no token"); a bad one is ignored instead of failing.
- **The leader must have validated this very name and nameplate** with their own token
  (`POST /guild/validate`, which the create window sends while they type) in the last 15 minutes. The
  last five validated pairs per player count, regardless of case, in case Create is pressed before the
  last check has come back. Otherwise the create is refused with an empty code, which the client shows
  as "Unable to create guild." (the message in our body never reaches the screen: for an error status
  the client builds its message from the HTTP status, B `0x140aae447`), and logged as "no validate of
  this name and nameplate by the leader in the last 15 minutes". A name the rules refuse anyway gets
  that rule's own text. So a modified client cannot make another player the leader of a guild that
  player never named, and being online is not enough.
- `GUILD_CREATE_ACTIVITY_FALLBACK=1` also accepts a leader who validated another name or was heard
  from in the last minute, with a warning in the log. It exists only for the case where the live test
  shows the client never validates the final name.
- At most one new guild per leader per 10 minutes, and an admin can disband any guild. A successful
  create uses up the leader's validated names.

### Storage, limits and permissions

Guilds, members and invites are stored in SQLite (migration `0013_guilds`, three new tables; see
[Files and data]({{ files_page.url | relative_url }})), so a guild survives restarts and an invite waits
for a player who is offline.

| Limit | Value |
|:------|:------|
| Members per guild | `GUILD_MAX_MEMBERS`, default 100 (the client takes whatever number we send) |
| Invite lifetime | `GUILD_INVITE_TTL_DAYS`, default 7 |
| Open invites per guild | 50 |
| Invites sent per inviter | 30 per hour |
| Open invites per player | 20; the oldest is dropped (one guild holds at most one of them) |
| Re-inviting a player who declined | the same guild waits 24 hours |
| Guilds created per leader | 1 per 10 minutes |

| Action | Who |
|:-------|:----|
| Read your guild and your invites | you |
| Invite | Leader, Officer |
| Accept, decline | the invited player |
| Leave | Member, Officer |
| Kick, change ranks, disband | Leader |

A block in either direction refuses a guild invite and removes the open ones between the two. An
Officer's invites are removed when the Officer is demoted to Member, kicked or leaves; the list leaves
out, and an accept refuses (`Uninvited`), any invite whose inviter is no longer a Leader or Officer of
that guild. A player in another guild can be invited but must leave that guild before accepting (the
client says so itself, B).

**Nothing is pushed.** Other members and invitees see a change at their next `GET /guild` (login,
world load, or their own guild action). The panels do not refresh themselves (B).

## Slayer Links {#slayer-links}

Slayer Links (the client's name is Linked Slayers; the My Links tab of the Social panel, K
`USocialPanelTabLinkedSlayers`) let two friends link up for a week. Each player has three link slots.
One player invites a friend into a slot, the friend accepts into one of theirs, and the link runs for
168 hours.

**Status (23 September 2026): built and tested without the game, on by default (`SLAYER_LINKS`), not
yet tried in game.** The contract comes from Harmonic's fork, which had the first working version; we
checked every route and body against the executable and corrected five points (below). No Slayer
Link call appeared in the census of 22 September 2026 (L), so the client may keep the tab hidden
(`ULinkedSlayersFeature`); if it never calls these routes, nothing changes for players.

### The routes {#slayer-link-routes}

Every reply uses the Phoenix envelope `{"code": null, "message": "OK", "payload": ...}`. Each route
acts as the account of the bearer token; ids in a body or path only name the other player.

| Route (endpoint key) | Request | Payload of the reply | Evidence |
|:---------------------|:--------|:---------------------|:---------|
| `GET /slayerlink/status_good` (`LinkedSlayersStatusEndpoint`) | none | `{invites: [...], links: [...], config: {link_duration_hours: 168, invite_expiry_hours: 24}}` | B: `FOnlineLinkedSlayer::GetStatusUpdate` `0x1415e5990` and its handler `0x1415e8b40` read an object with the keys `invites`, `links` and `config` (serializer `0x141600510`); config reads the two hours (`0x1415ffc90`). That this object is the envelope's payload: S. The client polls for news (S: that the poll is this route). |
| `GET /slayerlink/invites` (`LinkedSlayersAllInvitesEndpoint`) | none | `{invites: [{account_id, slot, direction, status, expires, link_id}]}`: pending, unexpired invites; `account_id` is the other player, `direction` is `Sent` or `Received`, `status` is `Pending`, `expires` an ISO date | B `0x1415e7d00`, entries `0x1415ff170`; the enum spellings from the SDK (S) |
| `GET /slayerlink/links` (`LinkedSlayersAllLinkSlotsDataEndpoint`) | none | `{links: [{account_id, linked_account_id, slot, ends, link_id, prize_pool: []}]}`: running links, by the caller's slot | B `0x1415e8690`, entries `0x141600170` read `account_id`, `slot`, `ends`, `prize_pool`; the status reply's link entries read `linked_account_id`, `slot`, `ends`, `link_id`, `prize_pool`. Every entry carries both sets. |
| `PUT /slayerlink/invite` (`LinkedSlayersInviteEndpoint`) | `{account_id, slot, action_source}` | `{link_id}`: the invite's id; inviting the same player again answers the same id | B `0x1415fcba2`, body `0x1415ff0e0` |
| `POST /slayerlink/invite` (`LinkedSlayersInviteAcceptDeclineEndpoint`) | `{account_id, action, slot, action_source}`, `action` = `accept`, `reject` or `cancel` | `{link_id}` | B: accept `0x1415d4aa6`, cancel `0x1415d96cf`, reject `0x1415dadd6`, body `0x1415fdc40` |
| `DELETE /slayerlink/invites/:account_id` (`LinkedSlayersDeleteAllInvitesEndpoint`) | none | `{}` | B `0x1415db890`; whose id the client puts in the path was not traced (G), so both meanings are handled |
| `DELETE /slayerlink/links` (the same key as the links list) | `{account_id, slot, delete_pair}`, or the same as a query | `{}` | B: `FOnlineLinkedSlayer::DeleteLinks` `0x1415dc442`, body `0x141600eb0` |
| `POST /slayerlink/availability` (`LinkedSlayersGetFriendsAvailabilityEndpoint`) | `{account_ids: [...]}`, at most 50 | `{availability: [{account_id, available}]}` | B `0x1415e3608`, `0x1415fdf30`, `0x1415fdf90` |

Errors are `{"code": "<status>", "message": ..., "payload": null}`: 400 (no account id, a slot outside
1-3, an unknown action), 403 (not friends, or blocked), 404 (no such account or invite), 409 (yourself,
the slot is linked or has a waiting invite, already linked, the other player already invited you, no
free slot, more than 20 new invites in 10 minutes, the invite ran out or was already answered). No
token gets 401; a game server's key alone
gets 403.

**Not answered:** the reward routes `PUT /slayerlink/links/rewards` and
`GET /slayerlink/links/rewards/:account_id/:slot` stay 404 until they are traced, because a false
success could lose a reward. The keys `LinkedSlayersInviteCancelEndpoint`, `...AllLinksProgressEndpoint`,
`...AddLinkProgressEndpoint`, `...DeleteInviteDataEndpoint` (`/slayerlink/link`),
`...SetEndTimeEndpoint` and `...SetRemainingTimeEndpoint` have no reference in the executable (B): the
client never sends them.

### The rules {#slayer-link-rules}

- **Who.** Both players must be accepted friends, and neither may have blocked the other.
- **Slots.** Three per player, numbered 1 to 3 as the client numbers them, and one waiting invite per
  slot. The client fills its slot map with the keys 1 up to its `MaxLinkSlotsCount`, which is 3
  (B: the loop `0x1415ea22d`-`0x1415ea3a5`, the count set at `0x1415ce5f6`), and looks every slot up
  by that key; a slot it does not hold logs "Invalid Slot Id" (B `0x1415ef4a0`). A slot of 0 is
  therefore refused (400) and never stored. An accept uses the slot in the body when it is free,
  otherwise the first free one.
- **Invite limit.** A player sends at most 20 new invites in 10 minutes (409 after that, the same
  limit as friend requests), counted from the stored invites, so a restart does not reset it. Inviting
  the same player again answers the waiting invite and does not count.
- **Answers.** Accept and reject belong to the invited player (`account_id` = the sender), cancel to the
  sender (`account_id` = the invited player). A `link_id` or `invite_id` in the body, if the client ever
  sends one, is tried first. Repeating an answer of the same kind is 200 again.
- **Times.** An invite lasts 24 hours and a link 168 hours; an expired invite turns `EXPIRED` the next
  time it is looked at.
- **Clearing invites.** `DELETE /slayerlink/invites/<own id>` withdraws every invite the caller sent and
  declines every one they received; with another player's id it does that only for the invites
  between the two.
- **Removing a link** ends the caller's link in that slot, or with that player, for both players (there
  is one row per pair; `delete_pair` is only logged). Nothing to remove is still 200.
- **Unfriending or a block** cancels the waiting invites between the two, inside the same database
  transaction. A running link stays until it ends: whether an unfriend should also end it is the
  owner's decision.
- Declined, cancelled and expired invites are deleted once their 24 hours are over, accepted invites
  and ended links 30 days later (both when the next new invite is made). Invites and links are stored
  in SQLite (migration `0016_slayer_links`, tables `slayerlinkinvites` and `slayerlinks`; see
  [Files and data]({{ files_page.url | relative_url }})).

### What we corrected from Harmonic's version {#slayer-link-corrections}

| Harmonic's fork | The client (B) |
|:---------|:---------------|
| The invite list names the other player `linked_account_id` | It reads `account_id` (`0x1415ff170`) |
| A link is removed at `/slayerlink/link` | That key is never used; the client sends `DELETE /slayerlink/links` with a body (`0x1415dc442`) |
| No route for `DELETE /slayerlink/invites/<id>` | The client sends it (`0x1415db890`) |
| The status reply was flat | It nests `invites`, `links` and `config` (`0x141600510`) |
| Slots are numbered 0 to 2 | The slot map's keys are 1 to 3 (`0x1415ea22d`), and every lookup uses them |

**Logs.** Every action is one `slayerlink:` line, for example
`slayerlink: invite by=<A> to=<B> slot=1 -> sent id=<id>` or
`slayerlink: accept by=<B> other=<A> id=<id> -> accepted (slots 1 and 2)`; a refusal ends in
`refused <status>: <reason>`. `SLAYER_LINKS=0` makes every route fall through to the 404 it got
before; the stored invites and links stay. The routes are also on
[HTTP API]({{ api_page.url | relative_url }}#slayer-links).

## The session check (`oauth/verify`) {#verify}

While it runs, the client regularly checks its session with `GET /account/api/oauth/verify` (about every
30 seconds, G for the exact period). Upstream answered every caller with the same placeholder account,
which the client's handler cannot match to its user: the executable logs "Verify auth response ignored,
can't find UserAccount for %s" for that case (B `0x140965ab8`, in the handler at `0x140964040`).

Since 23 September 2026 the reply names the account of the token it came with, as the real service
did:

- a valid player token: `account_id` is that player's own account;
- a missing, malformed, expired or foreign token: the old placeholder reply, still with **200**. It never
  answers 401: after the token's 24 hours a 401 could log the player out (G). A bad or expired token is
  logged at most once a minute ("with a bad or expired token: answering the static reply"), which is
  expected for sessions older than 24 hours;
- `expires_at` stays far in the future and `active` true.

With the player's own id the reply takes the client's "found the account" path; which fields that path
reads, and whether it updates the expiry, was not traced (G). `VERIFY_STUB_ACCOUNT=1` puts the
placeholder back for everyone. The live test is a session of more than 30 minutes, ideally past the
token's 24 hours, with no logout and no reconnect loop.

## Testing without the game {#testing-without-the-game}

The fixes are tested over HTTP against the real metagame, with the client's own bodies and headers:

- **A model of the client** (`UndauntedMetagame/test/socialclient.ts`) reimplements the parse rules
  above: the mapping parser, the user-info cache with its first-reply-wins key, the toolkit's Epic id
  check, the Phoenix envelope, the guild error mapping, the friends list and the party invitations.
  Fed the replies our server sent on 22 September 2026, it reproduces what the players saw: the array
  mapping maps nothing, and the old user-info reply leaves the invite's sender invalidated. That checks
  the model, and guards against the same mistakes coming back.
- **The social flows** (`test/socialflow.test.ts`): Add Friends by name through the whole chain, a
  pending request seen at the other player's login, blocked players, a received party invite whose
  sender is set up, accept, every party member resolving to their own name, the leader queueing with
  every member expected exactly once, user info filed under the right id in any order, the name form of
  `/accountinfo/public`, the mapping details, both rollback switches, the new friend limits, and the
  party invite rules (a block removes invites both ways, the pause after a decline, the sender's limit).
  `test/partyhttp.test.ts` also covers `PARTY_SOLO_STUB=0`.
- **Guilds** (`test/guildhttp.test.ts`): every route and error code, the reserved words, the game
  server's create tied to the leader's own validate of that exact name (another name, nameplate or
  leader is refused; a token that comes along changes nothing; the activity fallback switch), invites,
  expiry, the limits, ranks and hand-over, kick, leave and disband (including the route order), what
  a block, a decline and a demoted, kicked or departed Officer do to open invites, a restart,
  permissions and `GUILDS=0`. Each reply is checked as exact JSON and through the model.
- **Slayer Links** (`test/slayerlinks.test.ts`, rewritten from Harmonic's case with the client's exact
  paths and bodies): every route and its keys, both directions of the invite list, accept with the
  chosen or the first free slot, reject and cancel by `account_id`, both meanings of the invite delete,
  a link removed for both players, the three slots numbered 1 to 3 (0 and 4 refused, the third slot
  used) and one waiting invite per slot, the invite limit and the clean-up of answered invites, expiry
  with a test clock, an unfriend or a block cancelling waiting invites while a running link stays, 401 and 403 on
  all eight routes, `SLAYER_LINKS=0` and the unanswered reward routes.
- **The smaller fixes** (`test/socialported.test.ts`): the repeated party accept (by the party id and by
  the sender; not for one's own id, a stranger, a party of one or a party left), and `oauth/verify`
  (the caller's own id with a valid token; the placeholder with 200 for no token, a bad, expired or
  foreign one; `VERIFY_STUB_ACCOUNT=1`).

## Open questions, and the live test

To be checked in the next two-player test ([How to verify](#how-to-verify) has the steps):

1. Does the `/accountinfo/public` fix alone make received party invites show? The log should show the
   invite poll, then `accountinfo/public by <recipient> for <sender> -> found`.
2. Where does an incoming friend request appear, and is that section hidden while empty?
3. Does the Block menu reach our friends routes at all (a `POST` or `PUT .../blocklist/...` line)?
4. Does the game server's `POST /guild` carry any `Authorization` header (the created line ends "no
   token" or "a token of X came along"), and does the client validate the final name and nameplate
   before Create (a "no validate of this name" refusal says it did not)?
5. Does `GET /guild` run at every world load (34 calls in 8 logins suggest so)?
6. Nobody should be kicked from a party after a minute (no `DELETE /party/member/<id>` or
   `/party/leader/<id>` lines).
7. Does Invite to Party work for a leader on their own with the placeholder candidate (a
   `PUT /party/invite` line appears), or does it need `PARTY_SOLO_STUB=0`?
8. How many `account/mapping` calls does each login make (the census had 6 in 8 logins)?
9. Does the client ever send a party accept twice (a `(a repeated accept)` line)?
10. Does the My Links tab call the Slayer Link routes at all, and how often does it poll
    `GET /slayerlink/status_good`?
11. Does a session of more than 30 minutes stay logged in with the new `oauth/verify` reply?

**Every player hits three changed replies at each login**, whether or not they use the Social panel:
`POST /accountinfo/public` (now about the asked account, 404 for an unknown one), `POST /account/mapping`
(now maps the local player where it used to map nothing), and `GET /guild/invite/player` (the new
envelope `{"code": "OK", "message": "", "payload": {"invites": []}, "invites": []}` instead of the old
stub `{"code": null, "message": "OK", "payload": {"invites": []}}`). Our client model reads all three
as intended, but none has been seen in a real game. The rollback switches, if one disturbs something:
`ACCOUNTINFO_PUBLIC_LEGACY=1`, `ACCOUNT_MAPPING=0` and `GUILDS=0` (see
[Configuration]({{ config_page.url | relative_url }})). Since 23 September 2026 a fourth reply changes
for everyone: `oauth/verify` names the player's own account (`VERIFY_STUB_ACCOUNT=1` is its rollback).

## How to verify {#how-to-verify}

A test with two players, A and B, on the rented server. Each step names what to do, what the
metagame's log (`data\logs\metagame.out.log` on a kit server) should show, and what to do if it does
not. `<A>` and `<B>` stand for the two account ids (`UID-...`); the request log prints every call as
`METHOD /path gs=0|1` (`gs=1`: from a game server).

**0. Before the session (host).**

1. Deploy the update. At the first start the metagame applies migration `0013_guilds` and starts
   listening as usual; no `guild:` line appears until someone uses guilds.
2. Optional: set `LOG_BODIES=1` in the metagame's settings for this session only, to record the
   request bodies (account ids and guild names; the kit turns it off again in public mode). Delete
   the body log afterwards.
3. Both players **quit the game completely and start it again** from the launcher. The client keeps
   the old answers until it restarts.

**1. Login (each player).** Expect, per player X: `POST /login`, `friends: list for <X>: 0 friend(s),
0 pending`, `GET /friends/api/public/blocklist/<X>`, `accountinfo/public by <X> for <X> -> found`,
`GET /guild gs=0` and `GET /guild/invite/player gs=0` (and **no** "Guild invites (stubbed)" line),
`POST /party gs=0` about every 10 seconds, and `party: poll by=<X> P=... size=1 leader=<X>` (a poll
line is logged again only when the party changes). Count the
`account/mapping by <X>: ... -> 1 of 1 mapped` lines (question 8). The player reaches Ramsgate and
the Social panel opens; everyone shows as Offline, which is expected.
If a login hangs or the Social panel breaks, turn on `ACCOUNTINFO_PUBLIC_LEGACY=1`, restart the
metagame and both games, and retry; then `ACCOUNT_MAPPING=0`, then `GUILDS=0`, one at a time, to find
the change at fault.

**2. A invites B to a party.** A: Social, find B under Hunt Members (or type `/invite <B's name>` in
chat), Invite to Party. Expect `PUT /party/invite gs=0` and `party: invite P=<PA> from=<A> to=<B>`,
then within about 10 seconds `party: invites for <B> -> 1 (P=<PA> from=<A>)` and
`accountinfo/public by <B> for <A> -> found`. B sees a toast and an entry under PARTY INVITES
(question 1).

- No `PUT /party/invite` line at all, and the menu item greyed out or silent: the placeholder reads as
  matchmaking (question 7). Set `PARTY_SOLO_STUB=0`, restart the metagame (the games can stay open;
  parties start over), and retry.
- `party: invite by=<A> to=<B> refused ...`: the reason is in the line.
- The invite poll shows 1 and `accountinfo/public ... -> found` appears, but B sees nothing: the
  diagnosis is wrong somewhere; note it, and use the host's `PartyInvite` to go on with the other steps.

**3. B accepts.** Expect `party: accept by <B> matched=partyId P=<PA> size=2`, then
`party: poll by=<A> P=<PA> size=2 leader=<A> members=<A>,<B>`. Both party panels show both names.
A second accept of the same party, if the client sends one, logs `... (a repeated accept)` and is
answered with the party (question 9). (Declining instead logs `party: decline by=<B> ... removed=1`, and A cannot invite B again for 2
minutes.)

**4. A hunt together.** A (the leader) picks a hunt. Expect `mm: party P=<PA> candidate <C> mode=...
hunt=... members=<A>,<B> by=<A>` and then `mm: party P=<PA> candidate <C> ready at <host>:<port> for 2
member(s)`; both land on the same hunt, and come back to Ramsgate with the leader. Afterwards the
polls still show `size=2`. For the whole session there should be no `DELETE /party/member/<id>` or
`DELETE /party/leader/<id>` line (question 6).

**5. Add Friends.** A: Social, Add Friends, type B's username, Add. Expect
`EOS Account by name by <A>: <B>`, `account/mapping by <A>: ... ids=[1]; ... -> 1 of 1 mapped`,
`accountinfo/public by <A> for <B> -> found` and `friends: request by=<A> to=<B> -> requested`; A sees
"friend invite sent". B restarts the game (the lists are read only at login): expect
`friends: list for <B>: 0 friend(s), 1 pending`; note where the request shows (question 2). B
accepts: `friends: request by=<B> to=<A> -> accepted`; after the next login both show each other
under OFFLINE (`1 friend(s)`).

- `EOS Account by name by <A>: not found`: the name was typed wrong (it must be exact, any case).
- No `account/mapping` line after the name lookup, or `0 of 1 mapped`: the mapping step failed; note
  it (`ACCOUNT_MAPPING` must not be `0`).
- Optional, question 3: B blocks A from A's menu; expect a `POST` or `PUT`
  `/friends/api/public/blocklist/<B>/<A>` line and `friends: block by=<B> target=<A> -> blocked`; note
  which verb. Unblock logs `-> unblocked`.

**6. A creates a guild.** A: Guilds tab, CREATE GUILD, type a name (4-15 letters and digits) and a
nameplate, **wait a second**, press Create. Expect one or more
`guild: validate by <A> name="..." tag="..." -> ok`, then `POST /guild gs=1` and
`guild: created G=<id> name=... tag=... leader=<A> (validated name, no token)`. Note "no token" or "a
token of X came along" (question 4). A sees the guild view ("Members: 1 / 100") and the `[TAG]` over
their head.

- `guild: create for <A> ... refused 403 (no code): no validate of this name and nameplate ...`: the
  create named something A had not validated. Retype, wait until the window has checked it, and
  press Create again. If it keeps happening, the client does not validate the final name: set
  `GUILD_CREATE_ACTIVITY_FALLBACK=1`, restart the metagame and retry (question 4).
- A validate refused with a code (for example `409 SeizedAdorableQuillshot`): the window shows the
  reason; pick another name.
- No `POST /guild gs=1` line at all: the game server never sent the create; check the Ramsgate game
  server's log.
- `guild: create for <someone else>`: the leader id is not A's own; note it.

**7. Guild invite, accept, ranks.** A: the add-member box in the Guilds tab, B's username (or Invite to
Guild in B's menu). Expect `EOS Account by name by <A>: <B>`, a mapping line and
`guild: invite by=<A> to=<B> -> 200`. B travels (to a hunt and back) or logs in again: expect
`GET /guild/invite/player gs=0` and `accountinfo/public by <B> for <A> -> found`; B sees the invite
under GUILD INVITES. B accepts: `guild: accept by=<B> G=<id> -> 200`, and B sees the guild view. A
sees B after A's next `GET /guild` (a world load). Then, as wanted: Promote To Guild Officer
(`guild: rank by=<A> target=<B> rank="officer" -> 200`), Leave Guild (`guild: leave by=<B> -> 200`),
DISBAND GUILD (`guild: disband G=<id> by=<A> -> 200`). Count the `GET /guild` lines per world load
(question 5).

**8. Slayer Links (after steps 1-7, the two being friends; once the update of 23 September 2026 is
deployed).** A: Social, My Links, invite B into a slot. Expect `PUT /slayerlink/invite gs=0` and
`slayerlink: invite by=<A> to=<B> slot=<n> -> sent id=<id>`. B opens My Links: expect
`GET /slayerlink/invites` (or `/slayerlink/status_good`) and sees the invite with A's name. B accepts:
`slayerlink: accept by=<B> other=<A> id=<id> -> accepted (slots X and Y)`; both see the link. Then, as
wanted: remove the link (`slayerlink: delete link by=... -> removed <id> (with ...)`), and in a new round
decline (`-> declined`) and cancel (`-> canceled`). If no `/slayerlink` line appears at all, the client
keeps the tab hidden: note it. If anything misbehaves, set `SLAYER_LINKS=0` and restart the metagame.

**9. Afterwards.** Look for social routes answered 404 in the request log, and for any
`refused` line you did not expect. Turn `LOG_BODIES` off again and delete the body log. Write down the
answers to the open questions above; the corrections go into this page and the roadmap.

## Deferred {#deferred}

| Item | Why |
|:-----|:----|
| **Online status by default** (presence over the chat connection) | Online status, EPIC FRIENDS, "In Ramsgate" and a new friend showing without a new login all ride on XMPP presence. It is built into the chat server since 23 September 2026 ([Text chat]({{ chat_page.url | relative_url }}#presence)) and never sends a player their own presence, but it stays off (`CHAT_PRESENCE=0`) until chat itself is live and a party of two has kept both members for a minute with it on (roadmap 3.10). |
| Recent players from the friends service | Never called live; the in-game Recent Players list is kept in the character data. |
| Other friends routes (settings sources, delete all, email lookups) | Never called live. |
| Listing sent party invites | The client might misread them as received. |
| Party finder, console sessions, Phoenix's presence socket | Never called; polling carries all party state. |
| Voice | Vivox is gone; guild voice is not implemented in 1.4.4 itself. |
| Slayer Link rewards | The two reward routes are not traced yet; a false success could lose a reward. The rest of [Slayer Links](#slayer-links) is built. |
| The v1 guild API | Never called by 1.4.4. |

The [2.1.1 standalone attempt]({{ awakening_page.url | relative_url }}) describes the same Phoenix
services from the final client, including the wrapped and flat reply forms.
[Join as a friend]({{ friends_page.url | relative_url }}) explains to players what works today.
