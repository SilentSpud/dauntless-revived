// The page's bridge to the Tauri backend (MIGRATION_PLAN.md L7): the same calls as preload.ts, with the
// same names and types, over Tauri's invoke and events. Each call goes through the isolation hook
// (isolation/hook.js), which drops anything that is not on the backend's list; a dropped call never
// settles, so every call here has a deadline.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LauncherApi } from "../preload";
import type { CommandName } from "../shared/generated/types";
import type { ActionResult, OfflineStatus, Settings, Snapshot, TaskProgress } from "../shared/types";

// A call that waits on the player (a file dialog) gets an hour; any other a minute.
export const CALL_TIMEOUT_MS = 60_000;
export const DIALOG_TIMEOUT_MS = 3_600_000;

export class BridgeError extends Error {
  constructor(readonly command: string, reason: string) {
    super(`${command}: ${reason}`);
    this.name = "BridgeError";
  }
}

// What a failed call answers when the page expects an answer, as the Electron main process did.
const FAILED: ActionResult = { ok: false, error: { code: "unknown" } };

function call<T>(command: CommandName, args: Record<string, unknown> = {}, timeoutMs = CALL_TIMEOUT_MS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new BridgeError(command, "no answer")), timeoutMs);
    invoke<T>(command, args).then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timer);
        reject(new BridgeError(command, String(error)));
      },
    );
  });
}

// An action: a failure is reported as the page reports any error.
async function act<T = ActionResult>(command: CommandName, args?: Record<string, unknown>, timeoutMs?: number): Promise<T> {
  try {
    return await call<T>(command, args, timeoutMs);
  } catch (error) {
    console.error(error);
    return FAILED as T;
  }
}

// A call with nothing to answer: a failure is only logged.
async function tell(command: CommandName, args?: Record<string, unknown>): Promise<void> {
  try {
    await call<unknown>(command, args);
  } catch (error) {
    console.error(error);
  }
}

function on<T>(event: string, fn: (payload: T) => void): void {
  listen<T>(event, (e) => fn(e.payload)).catch((error: unknown) => console.error(new BridgeError(`listen ${event}`, String(error))));
}

export const api: LauncherApi = {
  getSnapshot: () => call<Snapshot>("get_snapshot"),
  checkInvite: (text) => act("check_invite", { text: String(text) }),
  submitInvite: (text, acceptNewCertificate = false) =>
    act("submit_invite", { text: String(text), acceptNewCertificate: acceptNewCertificate === true }),
  retryConnect: () => act("retry_connect"),
  forgetServer: () => act("forget_server"),
  switchServer: (id) => act("switch_server", { id: String(id) }),
  removeSavedServer: (id) => act("remove_saved_server", { id: String(id) }),
  register: (username) => act("register", { username: String(username) }),
  useExistingKey: (text) => act("use_existing_key", { text: String(text) }),
  importKeyFile: () => act("import_key_file", {}, DIALOG_TIMEOUT_MS),
  saveKeyBackup: () => act("save_key_backup", {}, DIALOG_TIMEOUT_MS),
  dismissBackupOffer: () => tell("dismiss_backup_offer"),
  chooseInstallDir: () => act("choose_install_dir", {}, DIALOG_TIMEOUT_MS),
  useExistingGameFolder: () => act("use_existing_game_folder", {}, DIALOG_TIMEOUT_MS),
  useExistingGamePath: (gamePath) => act("use_existing_game_path", { gamePath: String(gamePath) }),
  startInstall: () => act("start_install"),
  pauseTask: () => tell("pause_task"),
  resumeTask: () => tell("resume_task"),
  cancelTask: () => tell("cancel_task"),
  repair: () => act("repair"),
  play: () => act("play"),
  startOffline: (username) => act("start_offline", username === undefined ? {} : { username: String(username) }),
  stopOffline: () => act("stop_offline"),
  offlineStatus: () => call<OfflineStatus>("offline_status"),
  // Only the settings fields go across, as in preload.ts
  setSettings: (patch: Partial<Settings>) =>
    call<Settings>("set_settings", {
      patch: {
        ...(patch.graphics !== undefined ? { graphics: patch.graphics } : {}),
        ...(patch.exposure !== undefined ? { exposure: patch.exposure } : {}),
        ...(patch.windowed !== undefined ? { windowed: patch.windowed } : {}),
        ...(patch.streamerMode !== undefined ? { streamerMode: patch.streamerMode } : {}),
        ...(patch.showConsole !== undefined ? { showConsole: patch.showConsole } : {}),
        ...(patch.huntRegion !== undefined ? { huntRegion: patch.huntRegion } : {}),
        ...(patch.language !== undefined ? { language: patch.language } : {}),
        ...(patch.gameLanguage !== undefined ? { gameLanguage: patch.gameLanguage } : {}),
      },
    }),
  setStatusPolling: (on) => tell("set_status_polling", { on: on === true }),
  refreshStatus: () => tell("refresh_status"),
  getNews: () => call("get_news"),
  getBranding: () => call("get_branding"),
  openExternal: (target) => act("open_external", { target: String(target) }),
  openGameFolder: () => tell("open_game_folder"),
  logout: () => act("logout"),
  installUpdate: () => tell("install_update"),
  dismissNotice: () => tell("dismiss_notice"),
  dismissError: () => tell("dismiss_error"),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: async () => {
    const window = getCurrentWindow();
    await window.toggleMaximize();
    return window.isMaximized();
  },
  close: () => getCurrentWindow().close(),
  takeInviteLink: () => call<string | null>("take_invite_link"),
  exportOfflineSave: () => act("offline_export_save", {}, DIALOG_TIMEOUT_MS),
  importOfflineSave: () => act("offline_import_save", {}, DIALOG_TIMEOUT_MS),
  onSnapshot: (fn) => on<Snapshot>("snapshot", fn),
  // Progress comes over a channel of its own, already throttled by the backend
  onProgress: (fn) => {
    const channel = new Channel<TaskProgress>();
    channel.onmessage = fn;
    void tell("subscribe_progress", { onProgress: channel });
  },
  onInviteLink: (fn) => on<unknown>("invite-link", () => fn()),
  onWindowState: (fn) => on<unknown>("window-state", (maximized) => fn(maximized === true)),
  onOfflineStatus: (fn) => on<OfflineStatus>("offline-status-changed", fn),
};
