// Generated from the launcher backend (src-tauri/src/bridge); do not edit.
// Regenerate with: DR_UPDATE_GENERATED=1 cargo test -p launcher generated
window.__DR_COMMANDS__ = {
  "cancel_task": {},
  "check_invite": {
    "text": {
      "kind": "text",
      "max": 2048
    }
  },
  "choose_install_dir": {},
  "dismiss_backup_offer": {},
  "dismiss_error": {},
  "dismiss_notice": {},
  "forget_server": {},
  "get_branding": {},
  "get_news": {},
  "get_snapshot": {},
  "import_key_file": {},
  "install_update": {},
  "logout": {},
  "offline_export_save": {},
  "offline_import_save": {},
  "offline_status": {},
  "open_external": {
    "target": {
      "kind": "text",
      "max": 32
    }
  },
  "open_game_folder": {},
  "pause_task": {},
  "play": {},
  "refresh_status": {},
  "register": {
    "username": {
      "kind": "text",
      "max": 32
    }
  },
  "remove_saved_server": {
    "id": {
      "kind": "text",
      "max": 24
    }
  },
  "repair": {},
  "resume_task": {},
  "retry_connect": {},
  "save_key_backup": {},
  "set_settings": {
    "patch": {
      "kind": "settingsPatch"
    }
  },
  "set_status_polling": {
    "on": {
      "kind": "flag"
    }
  },
  "start_install": {},
  "start_offline": {
    "username": {
      "kind": "optionalText",
      "max": 32
    }
  },
  "stop_offline": {},
  "submit_invite": {
    "acceptNewCertificate": {
      "kind": "flag"
    },
    "text": {
      "kind": "text",
      "max": 2048
    }
  },
  "subscribe_progress": {
    "onProgress": {
      "kind": "channel"
    }
  },
  "switch_server": {
    "id": {
      "kind": "text",
      "max": 24
    }
  },
  "take_invite_link": {},
  "use_existing_game_folder": {},
  "use_existing_game_path": {
    "gamePath": {
      "kind": "text",
      "max": 260
    }
  },
  "use_existing_key": {
    "text": {
      "kind": "text",
      "max": 8192
    }
  }
};
window.__DR_SETTINGS_KEYS__ = [
  "huntRegion",
  "graphics",
  "exposure",
  "windowed",
  "language",
  "gameLanguage",
  "showConsole",
  "streamerMode"
];
