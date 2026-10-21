// Generated from the launcher backend (src-tauri/src/bridge); do not edit.
// Regenerate with: DR_UPDATE_GENERATED=1 cargo test -p launcher generated

export type ServerMode = "private" | "public";
export type PlayerWhere = "menu" | "city" | "hunt" | "dojo" | "tutorial" | "unknown";
export type InstanceKind = "city" | "hunt" | "dojo" | "tutorial";
export type RegistrationMode = "OPEN" | "INVITECODE" | "NONE";
export type StatusPlayer = { name: string, where: PlayerWhere, instance: string | null, };
export type StatusInstance = { id: string, kind: InstanceKind, title: string, map: string, behemoth: string | null, players: number, maxPlayers: number, 
/**
 * ISO string, or null when the server sent something unreadable
 */
startedAt: string | null, };
export type ServerStatus = { name: string, online: boolean, version: string, commit: string, 
/**
 * Only https URLs survive parsing
 */
sourceUrl: string | null, registration: RegistrationMode | null, playersOnline: number, players: Array<StatusPlayer>, instances: Array<StatusInstance>, contentPort: number | null, uptimeSeconds: number, 
/**
 * The server hid who is online (playersOnline 0, no players, no instances): its answer to a
 * launcher without an account key there, or with one it did not accept.
 */
limited: boolean, };
export type Language = "en" | "fi" | "de" | "es" | "fr" | "it" | "ja" | "pt" | "ru";
export type ExposureMode = "game" | "basic";
export type GameLanguage = "auto" | "en-US" | "de-DE" | "es-ES" | "fr-FR" | "it-IT" | "ja-JP" | "pt-BR" | "ru-RU";
export type HuntRegion = "auto" | "main" | "ger" | "us";
export type Phase = "loading" | "join" | "connect" | "register" | "install" | "installing" | "update" | "ready" | "running";
export type ConnectProblem = "tailscale_missing" | "unreachable" | "bad_answer" | "cert_mismatch";
export type ErrorCode = "account_banned" | "invite_invalid_format" | "invite_invalid" | "username_invalid" | "username_taken" | "registration_closed" | "bad_request" | "server_unreachable" | "server_error" | "key_invalid" | "key_rejected" | "key_storage_unavailable" | "no_content_server" | "manifest_mismatch" | "manifest_unavailable" | "disk_space" | "folder_invalid" | "game_folder_not_found" | "folder_not_empty" | "download_failed" | "file_missing_on_server" | "file_different_on_server" | "verify_failed" | "dll_failed" | "game_files_invalid" | "already_running" | "launch_failed" | "windows_runtime_missing" | "compat_runtime_missing" | "config_failed" | "busy" | "offline_save_invalid" | "cancelled" | "cert_mismatch" | "cert_changed" | "rate_limited" | "relay_port_busy" | "relay_failed" | "unknown" | "offline_unavailable" | "offline_insufficient_memory";
export type NoticeCode = "keep_open" | "backup_saved" | "repair_done" | "install_done";
export type LauncherError = { code: ErrorCode, 
/**
 * Extra detail for the message, e.g. a file path or a byte count. Never a key.
 */
detail?: string, };
export type OfflinePhase = "preparing" | "starting-metagame" | "starting-deploy" | "needs-username" | "launching-game" | "running" | "stopping" | "stopped" | "error";
export type OfflineStatus = { phase: OfflinePhase, 
/**
 * 0..1 when meaningful
 */
progress?: number, error?: LauncherError, };
export type TaskKind = "download" | "verify" | "repair" | "dlls";
export type TaskProgress = { kind: TaskKind, paused: boolean, totalBytes: number, doneBytes: number, filesTotal: number, filesDone: number, bytesPerSecond: number, etaSeconds: number | null, currentFile: string | null, };
export type ServerInfo = { mode: ServerMode, host: string, port: number, name: string, hasShare: boolean, 
/**
 * An invite code is stored and not used yet
 */
hasPendingInvite: boolean, loopback: boolean, 
/**
 * Public mode: the pinned certificate fingerprint (not secret)
 */
fingerprint: string | null, };
export type SavedServerInfo = { 
/**
 * The account key slot id (not secret)
 */
id: string, mode: ServerMode, host: string, port: number, name: string, current: boolean, username: string | null, };
export type Settings = { huntRegion?: HuntRegion, graphics: GraphicsPreset, exposure: ExposureMode, windowed: boolean, language: Language, gameLanguage: GameLanguage, 
/**
 * The DLL's log window, normally hidden (DR_SHOW_CONSOLE=1)
 */
showConsole: boolean, 
/**
 * Hide addresses, fingerprints, paths and player names on screen
 */
streamerMode?: boolean, };
export type SnapshotConnect = { checking: boolean, problem: ConnectProblem | null, tailscaleInstalled: boolean, lastCheckedAt: string | null, };
export type SnapshotAccount = { username: string | null, hasKey: boolean, offerBackup: boolean, };
export type SnapshotInstall = { dir: string, defaultDir: string, 
/**
 * Every file present with the right size, and the DLLs in place
 */
installed: boolean, missingFiles: number, 
/**
 * This folder's files were fully checked or downloaded by the launcher
 */
verified: boolean, dllsOk: boolean, freeBytes: number | null, requiredBytes: number, totalBytes: number, vcRuntimeMissing: Array<string>, directXMissing?: Array<string>, contentAvailable: boolean, };
export type SnapshotGame = { running: boolean, relayPort: number | null, };
export type SnapshotApp = { version: string, packaged: boolean, updateReady: boolean, };
export type Snapshot = { phase: Phase, busy: boolean, server: ServerInfo | null, savedServers: Array<SavedServerInfo>, connect: SnapshotConnect, account: SnapshotAccount, install: SnapshotInstall, task: TaskProgress | null, game: SnapshotGame, settings: Settings, app: SnapshotApp, status: ServerStatus | null, 
/**
 * The host answers but has no ServerStatus endpoint yet
 */
statusUnsupported: boolean, lastError: LauncherError | null, notice: NoticeCode | null, };
export type NewsItem = { date: string | null, title: string, body: string, };
export type Background = { 
/**
 * An app-internal dr-art URL
 */
url: string, credit: string | null, };
export type Branding = { backgrounds: Array<Background>, accent: string | null, };
export type ExternalTarget = "tailscale_download" | "tailscale_share" | "vc_redist" | "directx_runtime" | "server_source" | "project_source" | "project_license" | "project_contributors" | "upstream_source" | "upstream_contributors" | "eugamehost" | "eugamehost_game1" | "eugamehost_game2" | "eugamehost_game3" | "eugamehost_game5" | "eugamehost_5800x" | "discord" | "patreon";
export type GraphicsPreset = -1 | 0 | 1 | 2 | 3 | 4;
export type ActionResult = { ok: true } | { ok: false; error: LauncherError };
export type InviteCheck = { ok: true; certificateChanged: boolean; previousFingerprint: string | null } | { ok: false; error: LauncherError };
export type RegisterOutcome = { ok: true; username: string } | { ok: false; error: LauncherError };
export type CommandName = "get_snapshot" | "check_invite" | "submit_invite" | "retry_connect" | "forget_server" | "switch_server" | "remove_saved_server" | "register" | "use_existing_key" | "import_key_file" | "save_key_backup" | "dismiss_backup_offer" | "choose_install_dir" | "use_existing_game_folder" | "use_existing_game_path" | "start_install" | "pause_task" | "resume_task" | "cancel_task" | "repair" | "play" | "start_offline" | "stop_offline" | "offline_status" | "set_settings" | "set_status_polling" | "refresh_status" | "get_news" | "get_branding" | "open_external" | "open_game_folder" | "logout" | "install_update" | "take_invite_link" | "dismiss_notice" | "dismiss_error" | "offline_export_save" | "offline_import_save" | "subscribe_progress";
