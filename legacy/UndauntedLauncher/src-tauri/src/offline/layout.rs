//! Where offline play keeps its state (`offline-layout.ts`): the same folders under `userData/offline` as
//! the Electron launcher, so a profile's database and secrets carry over. The services are this binary,
//! so nothing is staged in the resources folder any more (L44).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineLayout {
    pub state_root: PathBuf,
    pub database_path: PathBuf,
    pub runtime_state_dir: PathBuf,
    pub metagame_env_path: PathBuf,
    pub deploy_env_path: PathBuf,
    pub ready_dir: PathBuf,
    /// The app's resources: a save is never written inside them
    pub resources_dir: PathBuf,
}

impl OfflineLayout {
    pub fn new(user_data: &Path, resources_dir: &Path) -> Self {
        let state_root = user_data.join("offline");
        let runtime_state_dir = state_root.join("runtime");
        Self {
            database_path: state_root.join("undaunted.db"),
            metagame_env_path: runtime_state_dir.join("metagame.env"),
            deploy_env_path: runtime_state_dir.join("deploy.env"),
            ready_dir: state_root.join("ready"),
            runtime_state_dir,
            state_root,
            resources_dir: resources_dir.to_path_buf(),
        }
    }

    /// The game-server key file Deploy uses and the metagame registers.
    pub fn gameserver_key_path(&self) -> PathBuf {
        self.runtime_state_dir.join("gameserver.key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_electron_launchers_folders() {
        let layout = OfflineLayout::new(Path::new("/u"), Path::new("/r"));
        assert_eq!(layout.database_path, Path::new("/u/offline/undaunted.db"));
        assert_eq!(layout.metagame_env_path, Path::new("/u/offline/runtime/metagame.env"));
        assert_eq!(layout.deploy_env_path, Path::new("/u/offline/runtime/deploy.env"));
        assert_eq!(layout.ready_dir, Path::new("/u/offline/ready"));
        assert_eq!(
            layout.gameserver_key_path(),
            Path::new("/u/offline/runtime/gameserver.key")
        );
    }
}
