use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub last_scene: String,
    /// The name of the active playlist, if any.
    pub active_playlist: String,
}

/// `$XDG_STATE_HOME/vibe`, or `~/.local/state/vibe`.
pub fn state_dir(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_state_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local/state")))
        .map(|base| base.join("vibe"))
}

/// `$XDG_STATE_HOME/vibe/state.toml`, or `~/.local/state/vibe/state.toml`.
pub fn state_path(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    state_dir(xdg_state_home, home).map(|dir| dir.join("state.toml"))
}

/// The pre-state.toml `last-scene` file, migrated once when `state.toml` is absent.
pub fn legacy_scene_path(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    state_dir(xdg_state_home, home).map(|dir| dir.join("last-scene"))
}

/// Load state, migrating the old `last-scene` file when `state.toml` is absent.
/// Malformed or unreadable files fall back to defaults with a warning.
pub fn load(state_path: &Path, legacy: Option<&Path>) -> (State, Vec<String>) {
    let mut warnings = Vec::new();
    match fs::read_to_string(state_path) {
        Ok(text) => match toml::from_str::<State>(&text) {
            Ok(state) => (state, warnings),
            Err(error) => {
                warnings.push(format!("state: {}: {error}", state_path.display()));
                (State::default(), warnings)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut state = State::default();
            if let Some(legacy) = legacy {
                if let Ok(text) = fs::read_to_string(legacy) {
                    state.last_scene = text.trim().to_owned();
                }
            }
            (state, warnings)
        }
        Err(error) => {
            warnings.push(format!("state: {}: {error}", state_path.display()));
            (State::default(), warnings)
        }
    }
}

/// Serialize and atomically write a state file (temp file + rename).
pub fn save(path: &Path, state: &State) -> io::Result<()> {
    let text = toml::to_string_pretty(state).map_err(io::Error::other)?;
    atomic_write(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn state_path_prefers_xdg_then_home() {
        assert_eq!(
            state_path(Some(Path::new("/state")), Some(Path::new("/home/willow"))),
            Some(PathBuf::from("/state/vibe/state.toml"))
        );
        assert_eq!(
            state_path(None, Some(Path::new("/home/willow"))),
            Some(PathBuf::from("/home/willow/.local/state/vibe/state.toml"))
        );
        assert_eq!(
            state_path(Some(Path::new("")), Some(Path::new("/home/willow"))),
            state_path(None, Some(Path::new("/home/willow")))
        );
        assert_eq!(state_path(None, None), None);
    }

    #[test]
    fn legacy_scene_path_points_next_to_state() {
        assert_eq!(
            legacy_scene_path(Some(Path::new("/state")), None),
            Some(PathBuf::from("/state/vibe/last-scene"))
        );
    }

    #[test]
    fn state_roundtrips_through_save_and_load() {
        let directory =
            std::env::temp_dir().join(format!("vibe-state-{}-{}", std::process::id(), unique()));
        let path = directory.join("vibe/state.toml");
        let state = State {
            last_scene: "Julia".into(),
            active_playlist: String::new(),
        };
        save(&path, &state).unwrap();
        let (loaded, warnings) = load(&path, None);
        assert!(warnings.is_empty());
        assert_eq!(loaded, state);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_scene_migrates_only_when_state_is_absent() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-state-migrate-{}-{}",
            std::process::id(),
            unique()
        ));
        let state_path = directory.join("vibe/state.toml");
        let legacy = directory.join("vibe/last-scene");
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        fs::write(&legacy, "Waveform Scope\n").unwrap();

        let (loaded, warnings) = load(&state_path, Some(&legacy));
        assert!(warnings.is_empty());
        assert_eq!(loaded.last_scene, "Waveform Scope");

        // Once state.toml exists it wins over the legacy file.
        save(
            &state_path,
            &State {
                last_scene: "Julia".into(),
                active_playlist: String::new(),
            },
        )
        .unwrap();
        let (loaded, _) = load(&state_path, Some(&legacy));
        assert_eq!(loaded.last_scene, "Julia");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_state_falls_back_to_defaults_with_a_warning() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-state-bad-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/state.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not toml [\n").unwrap();
        let (loaded, warnings) = load(&path, None);
        assert_eq!(loaded, State::default());
        assert_eq!(warnings.len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
