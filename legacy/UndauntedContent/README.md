# Dauntless Revived content server

The content server, in the `legacy/UndauntedContent/` folder (roadmap item 1.16). It gives the friend
launcher the verified 1.4.4 game files, file by file, over Tailscale, and only to registered
accounts. It also serves the host's optional art pack and news for the launcher.

It is a separate process from the metagame so that 10.9 GB downloads never slow down the game
backend. It has no database and needs no secrets of its own: it checks each account key by asking
the metagame.

**Game files never go into this repository, GitHub Releases or any public URL.** The repository only
holds the manifest (`data/dauntless-1.4.4.json`): paths, sizes and SHA-256 hashes, no game content.
The files are read at run time from the host's own install.

## Endpoints

All are `GET` (and `HEAD`). Anything else is `404`; there is no directory listing.

| Route | Key needed | Answer |
|---|---|---|
| `/content/v1/manifest` | no | `{"build", "totalBytes", "files": [{"path", "size", "sha256"}]}`, the 410 files of the verified 1.4.4 build |
| `/content/v1/files/<path>` | yes | the file's bytes. `<path>` must be exactly a manifest path |
| `/content/v1/branding` | no | `{"backgrounds": [{"url", "credit"}], "accent"}` from the host's art pack (empty without one) |
| `/content/v1/branding/<file>` | no | one of those images (jpg, png or webp) |
| `/content/v1/news` | no | `{"items": [{"date", "title", "body"}]}`, newest first; `body` is plain text |

### Downloading a file

```
GET /content/v1/files/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe
x-undaunted-user-api-key: <the account's key>
Range: bytes=0-1048575                (optional)
If-Range: "<sha256>"                  (optional, when resuming)
```

| Status | When |
|---|---|
| `200` / `206` | the file, or the requested part of it. `Content-Type: application/octet-stream`, `ETag: "<sha256>"`, `Accept-Ranges: bytes`, `Content-Range` on `206` |
| `304` | `If-None-Match` has the file's ETag |
| `400 {"error":"bad_path"}` | backslashes, `.` or `..` segments, empty segments, percent-encoded `/` `\` `.` or NUL, double encoding, control characters or `: * ? " < > \|` |
| `401 {"error":"unauthorized"}` | no key, or the metagame doesn't know it |
| `404 {"error":"not_found"}` | not a manifest path (the match is exact, letter case included) |
| `416 {"error":"range_not_satisfiable"}` | a range that is past the end, backwards, not in bytes, or several ranges at once. `Content-Range: bytes */<size>` |
| `429 {"error":"too_many_streams"}` | the account already has 6 downloads running. `Retry-After: 2` |
| `503 {"error":"file_unavailable"}` | the file is missing on the host, the wrong size, or changed since the server started |
| `503 {"error":"auth_unavailable"}` | the metagame can't be reached to check the key. `Retry-After: 5` |
| `503 {"error":"server_busy"}` | the server-wide download limit is reached. `Retry-After: 10` |

A stale `If-Range` (a different ETag or a date) gets the whole file with `200`. `HEAD` needs the key
too but does not use a download slot. The launcher must still check every file's SHA-256 against the
manifest compiled into it; the server only promises to send the bytes on the host's disk.

## How it protects the host

- **Only manifest files.** A request path is looked up in an exact-match table built from the
  manifest, and the file opened is always built from the manifest's own path. The DLLs, logs or
  anything else in the game folder are never served.
- **Only registered accounts.** Every file request carries the account key. The server asks the
  metagame (`GET /undaunted/api/GetUserInfo`) and caches the answer for 5 minutes (refused keys
  for 30 seconds), keyed by the key's SHA-256. The key itself is never stored or logged. An invite
  code alone can register an account, but can't download.
- **Only over Tailscale.** `BIND_HOST` accepts loopback and Tailscale addresses (100.64.0.0/10,
  fd7a:115c:a1e0::/48) and nothing else, unless `CONTENT_ALLOW_ANY_BIND=1`.
- **Read-only.** The game folder is opened for reading only.
- **Limits.** At most 6 downloads per account and 48 in total (both configurable). A client that
  stops reading for 2 minutes is dropped, which frees its slot.
- **A log line per download**: account id, username, path, range, bytes sent, whether it finished,
  time and the client's address. One JSON object per line on stdout (warnings on stderr).

At startup the server checks that every manifest file exists with the right size and logs what is
missing or wrong. Those files are answered with `503`, the rest are served. A file whose size or
modification time changes while the server runs is also refused from then on. The full hash check
is `npm run verify`.

## Setup

Requirements: Node.js 24 and a verified 1.4.4 install (see `docs/setup/host.md`, step 2).

```powershell
cd UndauntedContent
npm install
npm run build
copy .env.example .env      # then set CONTENT_GAME_DIR, and BIND_HOST for friends
npm run verify              # full SHA-256 of the install against the manifest (10 s to a few minutes, by disk)
npm start
```

| Variable | Default | |
|---|---|---|
| `PORT` | `61002` | |
| `BIND_HOST` | `127.0.0.1` | comma-separated, e.g. `127.0.0.1,100.101.102.103` |
| `METAGAME_URL` | `http://127.0.0.1:61000` | where account keys are checked |
| `CONTENT_GAME_DIR` | (required) | the folder that contains `Archon\` |
| `CONTENT_BRANDING_DIR` | none | the art pack folder, see below |
| `CONTENT_NEWS_FILE` | none | the news file, see below |
| `CONTENT_MAX_STREAMS_PER_ACCOUNT` | `6` | |
| `CONTENT_MAX_STREAMS_TOTAL` | `48` | |
| `CONTENT_AUTH_CACHE_SECONDS` | `300` | how long a good key is trusted without asking the metagame again |
| `CONTENT_MANIFEST` | `data/dauntless-1.4.4.json` | |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` or `error` |

For friends, also allow TCP 61002 from 100.64.0.0/10 on the Tailscale adapter, like 61000
(roadmap 1.3).

### Art pack

The repository ships no artwork. A host who wants backgrounds in the launcher puts their own
images in a folder and sets `CONTENT_BRANDING_DIR`. Every `.jpg`, `.jpeg`, `.png` and `.webp`
there is used in name order (names: letters, digits, `_`, `-` and `.`; at most 25 MB each; the
file's first bytes must match its extension). For order, credits and an accent colour, add
`branding.json`:

```json
{
  "accent": "#c8a24a",
  "backgrounds": [
    { "file": "harbour-dusk.jpg", "credit": "Screenshot by a friend" },
    "second-image.webp"
  ]
}
```

The folder is re-read every 10 seconds, so images can be added without a restart. Only share art
you have the right to share.

### News

`CONTENT_NEWS_FILE` points at a JSON file:

```json
{ "items": [ { "date": "2026-09-26", "title": "Friends night on Friday", "body": "Ramsgate at 20:00.\nBring a friend." } ] }
```

It is re-read when it changes. Bodies are plain text (new lines are kept; HTML is not interpreted).
If an edit breaks the file, the last good version stays up and a warning is logged.

## The manifest

`data/dauntless-1.4.4.json` is generated from the verified game zip by
`tools/make-game-manifest.js` (from the repository root):

```powershell
node tools/make-game-manifest.js --zip <BaseGame144.zip> --compare-dir C:\D144\Dauntless
```

It hashes the whole zip first and stops unless it matches the pinned SHA-256
(`556B9A64…BC6D`). Then it hashes every file inside (checking each file's CRC-32 and size against
the zip's own records), checks the paths, checks that `Version.txt` names the 1.4.4 build and that
the exe is `D3D41E61…CFF4`, and writes the files sorted by path. `--compare-dir` then checks an
extracted install against it: every file's size, and full hashes of the exe and a spread of others
(`--compare-all` for every file).

Generated on 2026-09-21: **410 files, 10,893,512,875 bytes**. The install at `C:\D144\Dauntless`
matches it: 410/410 sizes and 14/14 sampled hashes at generation, and 410/410 full hashes with
`npm run verify` (11.5 s).

The friend launcher compiles this same file in (its Vite alias `@game-manifest` points here) and
refuses any file whose path, size or hash is not in it, so a compromised content server can't push
different files. If you regenerate it, rebuild the launcher.

## Tests

```powershell
npm test                    # unit tests: paths, ranges, auth cache, limits, manifest, branding, news, the handler
npm run test:integration    # the built server on 127.0.0.1:62002 against a real install and a mock
                            # metagame on 127.0.0.1:62003 (CONTENT_IT_GAME_DIR, default C:\D144\Dauntless)
```

The integration test covers the manifest, `401` without a key, a whole-file download with a hash
check, the exe's first megabyte as a range, 36 traversal and lookalike paths (all `400` or `404`),
the 6-stream limit, branding and news, the key cache, a metagame outage and the log. It starts and
stops its own processes and never touches ports 61000-61002.

## License

AGPL-3.0-only, like the rest of the repository.
