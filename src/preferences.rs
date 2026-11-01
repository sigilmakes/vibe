use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn state_path(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_state_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local/state")))
        .map(|base| base.join("vibe/last-scene"))
}

pub fn load(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|name| name.trim().to_owned())
}

pub fn save(path: &Path, scene: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{scene}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_path_prefers_xdg_then_home() {
        assert_eq!(
            state_path(Some(Path::new("/state")), Some(Path::new("/home/willow"))),
            Some(PathBuf::from("/state/vibe/last-scene"))
        );
        assert_eq!(
            state_path(None, Some(Path::new("/home/willow"))),
            Some(PathBuf::from(
                "/home/willow/.local/state/vibe/last-scene"
            ))
        );
        assert_eq!(
            state_path(Some(Path::new("")), Some(Path::new("/home/willow"))),
            state_path(None, Some(Path::new("/home/willow")))
        );
        assert_eq!(state_path(None, None), None);
    }

    #[test]
    fn last_scene_roundtrips_in_an_injected_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("vibe-prefs-{}-{unique}", std::process::id()));
        let path = state_path(Some(&directory), None).unwrap();
        assert_eq!(load(&path), None);
        save(&path, "Waveform Scope").unwrap();
        assert_eq!(load(&path).as_deref(), Some("Waveform Scope"));
        save(&path, "Julia").unwrap();
        assert_eq!(load(&path).as_deref(), Some("Julia"));
        assert!(save(&path.join("impossible"), "Julia").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
