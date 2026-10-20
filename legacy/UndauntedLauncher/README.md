# Dauntless Revived Launcher

*[Suomeksi](README.fi.md)*

The Windows and Linux app friends install to play on a Dauntless Revived server. It joins a server from an
invite, creates the account, downloads and checks the Dauntless 1.4.4 game files from the host's
own server, sets everything up and starts the game. English and Finnish.

**Linux:** native launcher packages are available for x86_64; the Windows 1.4.4 game client runs through Proton or Wine. See the [distro-by-distro Linux guide](https://mixutin.github.io/dauntless-revived/setup/linux.html) or [LINUX.md](LINUX.md).

**No game files are in this app or in this repository.** They come from the host's content server,
and every file is checked against the list of 410 files, sizes and SHA-256 hashes compiled into the
launcher (`legacy/UndauntedContent/data/dauntless-1.4.4.json`). A server that offers anything else is refused.

**Community:** [Join the Dauntless Revived Discord](https://discord.gg/ZJRprHzsgu) for help, players and project updates. The launcher also keeps a Discord button in its sidebar.

## For friends

1. Install a launcher package from the project's release. On Windows use
   `DauntlessRevivedLauncher-Setup.exe`. On Linux use the AppImage, `.deb`, `.rpm`, or portable
   ZIP described in [LINUX.md](LINUX.md). Check the download against `SHA256SUMS.txt` from the same
   release. The Windows installer is not code-signed yet, so SmartScreen may ask you to confirm
   (More info, then Run anyway); on a PC set to block unrecognised apps, verify the hash, unblock the
   file (right-click > Properties > Unblock, or `Unblock-File`) and run it again. Code-signing is
   roadmap item 4.16.
2. Open the invite link your host sent you, or paste it on the Play page and press **JOIN**.
3. Pick a username and press **REGISTER**. Save a backup of your key when the launcher offers it.
4. Press **INSTALL** (about 11 GB). You can pause, close the launcher and continue later. If you
   already have Dauntless 1.4.4, choose **I already have the game files** instead and paste the path
   of your `BaseGame144` folder (or the `Dauntless` folder inside it), or browse to it. The launcher
   uses the game where it is: it checks every file against its pinned manifest, replaces any file
   that differs from 1.4.4, and puts its own `dxgi.dll` and `UndauntedInternalServer.dll` in
   `Archon\Binaries\Win64`. Missing or different files are downloaded from your host, so that works
   only if your host has switched on game downloads. Copy the folder first if another setup still
   uses it.
5. Press **PLAY**.

At the bottom of the left-hand rail, on every page (also before you join a server), the GitHub
button opens this repository in your browser, and **Credits** lists the people who made the launcher
and the server, the open-source software they use, and the license.

## Two kinds of server

| | Private (invite v1) | Public (invite v2) |
|---|---|---|
| Where | the host's PC | a server with a public IP |
| How friends connect | Tailscale | directly, over TLS to the server's gateway |
| Invite | `dauntless-revived://join?v=1&host=…&port=61000&code=…&name=…[&share=…]` | `dauntless-revived://join?v=2&mode=public&host=…&port=443&fp=…&code=…&name=…` |

In public mode the invite carries `fp`, the SHA-256 of the server's TLS certificate. Every connection
(status, registration, downloads and the game itself) is checked against it before anything is sent.
A server with a different certificate gets nothing, and the launcher says "this is not your friend's
server".

The account key for a public server is tied to that fingerprint. An invite for the same address with
a different certificate never gets the key: the launcher shows both fingerprints and asks first, and
the key moves to the new certificate only if you confirm (do that only when your host tells you they
set the server up again). Private (v1) invites work only with Tailscale addresses (100.64.0.0/10) and
`*.ts.net` names, because that connection is plain HTTP inside the tailnet. (Loopback addresses such as
`127.0.0.1` are accepted too, for testing against a server on the same PC.)

The 1.4.4 game only speaks plain HTTP, so in public mode the launcher runs a **local relay** while the
game runs: the game talks to `http://127.0.0.1:61000` on your own PC, and the relay carries every
request (and the chat WebSocket) over pinned TLS to the server. Keep the launcher open while you play;
it asks before closing while the game runs. The relay listens on 127.0.0.1 only and refuses requests
that come from a web page.

## Security

- The page is sandboxed with context isolation and no Node access. It cannot reach the network at
  all; the main process makes every request, only to the invite's host. The page talks to the main
  process through a small typed API (`src/preload.ts`), and every argument is checked again.
- Strict Content-Security-Policy. Images come only from the app itself or from the host's art pack,
  fetched by the main process over the pinned connection and served through an internal scheme.
- Navigation and new windows are blocked. Links open in the browser only from a fixed allow-list:
  the page names a link (such as `project_source`), never a URL, and the main process opens that
  link's fixed URL (`src/main/links.ts`).
- The account key is stored through Electron `safeStorage` using the operating system's secure
  storage. It is never shown, never written to a plain file except the backup you choose to save,
  and is masked in the log.
- The key goes only to the invite's host (in public mode only over the pinned connection): with the
  account check, the downloads and the server status. The server shows who is online to registered
  players only, so before you register the server panel says to sign in instead of listing anyone.
  Launchers released before that change send no key and do not know that answer: against a server
  that has it they show 0 players online and no worlds until they update themselves. Update a server
  from an older version only after the launcher release with the change is out.
- Electron fuses: no `ELECTRON_RUN_AS_NODE`, no `NODE_OPTIONS`, no inspector flags, app code only
  from the integrity-checked asar.
- The two DLLs the game needs ship with the launcher and are checked against pinned hashes before
  they are copied and before every launch, like `friend-kit/play.ps1`.

## Development

Requirements: Windows or x86_64 Linux and Node.js 24. Linux packaging also needs `rpmbuild`,
`fakeroot` and `mksquashfs` (the Ubuntu CI job installs `rpm`, `fakeroot` and `squashfs-tools`).

```text
npm ci
npm run typecheck
npm test            # unit tests (node:test), local test servers on ports 62012-62013 and 624xx only
npm run make        # Windows: Squirrel installer + portable zip
npm run make:linux  # Linux: AppImage + deb + rpm + portable zip
npm run collect:linux
```

`npm test -- relay` runs only the test files whose name contains "relay". The tests cover invite
parsing, the relay (pinned certificate, wrong certificate, headers, bodies, streaming, keep-alive,
WebSocket), downloads (resume, verify, repair) over plain HTTP and pinned TLS, Engine.ini, launch
arguments, the links the launcher may open, the Credits page's data and the whole public-mode flow
through the controller, and the look: the brand colours and images. They make throwaway self-signed
certificates in a temp folder.

`DAUNTLESS_REVIVED_RELAY_PORT` moves the relay off 61000 for rehearsals on a PC where 61000 is
taken. The server must then point `QOS_TARGET_URL` at the same port.

The look follows the project's brand (`brand/`, see its README): the eight colours of
`brand/palette.json` as `--dr-*` tokens in `src/renderer/styles.css`, the logo and emblem, and an
original background drawn by `src/renderer/scene.ts` (mountains, pine forest, mist, a faint aurora and
snow) that a host's art pack replaces. It moves only whole layers, slowly; it holds still while the
game runs, and does not move at all when the operating system requests reduced motion.
`npm run icon` copies the icons and in-app images from `brand/`, where `python3 brand/build.py` makes
them. `npm test` fails when a copy is out of date, when the stylesheet has a colour that is not a
token, or when a colour pair it uses fails WCAG AA.

## Releases and updates

CI (`.github/workflows/ci.yml`) builds and tests the launcher on Windows and Linux on every push.
It keeps the Windows installer/ZIP and Linux AppImage/DEB/RPM/ZIP as artifacts for a week.
`.github/workflows/launcher-release.yml` publishes those exact tested files from `dauntless-revived`
as `launcher-v<version>`, with one combined `SHA256SUMS.txt`, a build provenance attestation for
every listed file, and a tag at the commit that was built. To release, raise the version:

- by default, a push to `dauntless-revived` that passes every check and has a version with no
  `launcher-v<version>` release yet publishes the installer CI built. Set the repository variable
  `LAUNCHER_AUTO_RELEASE` to `false` (Settings > Secrets and variables > Actions > Variables) to pause
  that;
- or run Actions > **Launcher release** > **Run workflow** on `dauntless-revived`.

After publishing, the release workflow points the rolling `launcher-updates` release at the new
Windows Squirrel package; installed Windows launchers check it every hour. Linux users update through
their package format or download a newer release. A version is published only if it is newer than
every earlier one, and never replaced. A prerelease version (such as `0.2.0-beta.1`) becomes a
GitHub prerelease and never reaches `launcher-updates`. If a run fails halfway, re-run its failed
jobs: it finishes what it started. Running the workflow for a version that is published already only
brings the Windows update feed up to it. Keep GitHub's immutable releases setting off, because
`launcher-updates` is updated in place.

## License

AGPL-3.0-only. Based on the Undaunted launcher by gwog (Gregory Morford) and contributors. An
unofficial fan project, not affiliated with or endorsed by Phoenix Labs or Epic Games.

The license texts of the third-party software in the launcher and its two DLLs (among them MinHook,
Electron and the GitHub Octicons mark) ship with it in `THIRD-PARTY-NOTICES.txt`. The Credits page
reads its list from `src/shared/credits.ts`.
