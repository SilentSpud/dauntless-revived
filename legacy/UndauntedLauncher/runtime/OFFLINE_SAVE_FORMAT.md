# Portable Offline saves (v1)

The backend exposes `exportSave(file)` and `importSave(file)`, returning the shared
`ActionResult`. Main-process UI adapters must call Controller's forwarding methods
so saves share its busy/task/game/shutdown lock. No renderer-controlled path API is
provided. Production uses an Electron utility process and packaged better-sqlite3.

The binary container consists of:

| Offset | Encoding | Meaning |
| --- | --- | --- |
| 0 | 8 bytes | ASCII `DRSAVE\r\n` |
| 8 | uint32, big endian | Version, currently 1 |
| 12 | uint32, big endian | UTF-8 metadata size (2–65,536 bytes) |
| 16 | uint64, big endian | SQLite size (512 bytes–512 MiB) |
| 24 | UTF-8 JSON | Metadata |
| following JSON | raw bytes | Exactly one SQLite database |
| final 32 bytes | SHA-256 | Digest of header, metadata and database |

Metadata contains `format: "dauntless-revived-offline-save"`, `version: 1`,
canonical ISO `createdAt`, `account: {userId, username}`, `database: {bytes, sha256}`,
and `credential: {version: 1, kind: "uuk-sha256", key}`. There are no stored paths,
archive entries, compression, or machine-bound encryption. Digests detect damage;
they do not authenticate an author against an attacker who can rewrite the file.
The save is sensitive: its backup-only player credential authenticates its database.

Export requires a stopped stack and rejects SQLite sidecar journal state. SQLite's
backup API produces a consistent private copy; only the copy is rekeyed with a
random 192-bit UUK for the same userId. The copy clears bootstrap queues,
game-server credentials, machine-bound account recovery ciphertext, old-key
recovery proofs/challenges, and increments that account's authentication
epoch. Secure-delete plus VACUUM removes freed credential pages from the copy.
Player tables and recovery audit/rate-limit events are preserved. The live database
and installed credential do not change.

Import streams into one fixed private file, verifies lengths/digests, SQLite
integrity, and a nonempty contiguous migration-history prefix matching packaged
SQL hashes/timestamps. The actual schema (including legitimate triggers) must
match that prefix. Pending packaged migrations run transactionally on the private
copy; the resulting current schema and account/key relationship are verified before
replacement. Future/unknown migration lineages, incompatible structures and future
container versions are rejected independently. Runtime secrets, online keys and
settings are never copied.

Export validates a complete sibling temporary file before rename. Import keeps
durable copies of the previous DB and encrypted Offline slot plus a prepared
journal, replaces the DB, saves through destination KeyStore/safeStorage, verifies
and flushes that slot, then publishes a committed journal. Ordinary failures restore
both previous files and publish a rolled-back journal before deleting snapshots.
Startup resolves prepared journals by retryable rollback; committed and rolled-back
journals require cleanup only, even when some snapshots are already missing.
Recovery runs before Offline services start. Rollback failures retain recovery
material and block startup rather than discard it.

This is recovery across two files, not a single atomic two-file filesystem operation.
Power loss can still defeat filesystem metadata ordering/durability guarantees.
An abrupt export interruption can leave an unpublished sibling temporary file;
it cannot replace the live profile. Private staging is removed on completion or
next recovery. File modes are restrictive where supported; Windows uses the
current profile's inherited ACL. Secure deletion on SSDs is not guaranteed.

Malformed saves return `offline_save_invalid`; lifecycle conflicts return `busy`;
KeyStore encryption failures return `key_storage_unavailable`; ordinary I/O errors
use `unknown`. Errors contain no credential or database detail. Private job files,
not credentials, cross process arguments; helpers suppress output.

After `npm run package`, run `node scripts/portable-save-smoke.cjs` on Windows.
It uses disposable profiles, different fake encryption contexts, the real Electron
helper and physically packaged services/native addons. It does not launch a game
or read the developer's Offline profile. Unit tests exercise Controller ownership
and restored identity reuse separately with a fake game and backend.
