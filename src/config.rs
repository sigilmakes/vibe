use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScenesConfig {
    /// "all", "favourites", or "tags".
    pub filter: String,
    /// Selected tags when `filter` is "tags".
    pub filter_tags: Vec<String>,
    /// Favourited scene names.
    pub favourites: Vec<String>,
}

impl Default for ScenesConfig {
    fn default() -> Self {
        Self {
            filter: "all".into(),
            filter_tags: Vec::new(),
            favourites: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoAdvanceConfig {
    /// "off", "phrases", or "timer".
    pub mode: String,
    /// Seconds before the next scene; clamped to 5–300.
    pub transition_secs: f32,
    /// "all", "favourites", or "playlist".
    pub within: String,
}

impl Default for AutoAdvanceConfig {
    fn default() -> Self {
        Self {
            mode: "off".into(),
            transition_secs: 20.0,
            within: "all".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// "auto", "pipewire", "pulse", "microphone", "command", or "file".
    pub source: String,
    /// Sink or source node name or serial; empty means the system default.
    pub target: String,
    /// Raw PCM command, used when `source` is "command".
    pub command: String,
    /// Raw PCM file, used when `source` is "file".
    pub file: String,
    /// "f32le", "s16le", or "s32le".
    pub format: String,
    pub rate: u32,
    pub channels: u16,
    /// Restart a dropped live capture automatically.
    pub auto_reconnect: bool,
    /// Loop a file source after EOF.
    pub loop_file: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source: "auto".into(),
            target: String::new(),
            command: String::new(),
            file: String::new(),
            format: "f32le".into(),
            rate: 48_000,
            channels: 2,
            auto_reconnect: true,
            loop_file: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    /// One of the names in `palette::PALETTES`.
    pub palette: String,
    pub follow_theme: bool,
    pub fps: u32,
    /// "auto", "truecolor", "ansi256", "ansi16", or "mono".
    pub color_depth: String,
    /// "auto", "unicode", or "ascii".
    pub glyphs: String,
    pub sensitivity: f32,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            palette: "Terminal".into(),
            follow_theme: false,
            fps: 60,
            color_depth: "auto".into(),
            glyphs: "auto".into(),
            sensitivity: 1.0,
        }
    }
}

/// A named, ordered set of visualisations the user can activate and edit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub name: String,
    pub scenes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub scenes: ScenesConfig,
    pub auto_advance: AutoAdvanceConfig,
    pub audio: AudioConfig,
    pub appearance: AppearanceConfig,
    pub playlists: Vec<Playlist>,
}

/// Copy only fields that differ from `base` into `target`.
///
/// `target` is the on-disk config; `base` is the effective config at the start
/// of the session (file values plus CLI overrides); `current` is the effective
/// config after menu edits. CLI-only values match `base`, so they are not
/// copied back.
pub fn merge_changed(target: &mut Config, base: &Config, current: &Config) {
    fn copy<T: Clone + PartialEq>(target: &mut T, base: &T, current: &T) {
        if current != base {
            *target = current.clone();
        }
    }

    copy(
        &mut target.scenes.filter,
        &base.scenes.filter,
        &current.scenes.filter,
    );
    copy(
        &mut target.scenes.filter_tags,
        &base.scenes.filter_tags,
        &current.scenes.filter_tags,
    );
    copy(
        &mut target.scenes.favourites,
        &base.scenes.favourites,
        &current.scenes.favourites,
    );

    copy(
        &mut target.auto_advance.mode,
        &base.auto_advance.mode,
        &current.auto_advance.mode,
    );
    copy(
        &mut target.auto_advance.transition_secs,
        &base.auto_advance.transition_secs,
        &current.auto_advance.transition_secs,
    );
    copy(
        &mut target.auto_advance.within,
        &base.auto_advance.within,
        &current.auto_advance.within,
    );

    copy(
        &mut target.audio.source,
        &base.audio.source,
        &current.audio.source,
    );
    copy(
        &mut target.audio.target,
        &base.audio.target,
        &current.audio.target,
    );
    copy(
        &mut target.audio.command,
        &base.audio.command,
        &current.audio.command,
    );
    copy(
        &mut target.audio.file,
        &base.audio.file,
        &current.audio.file,
    );
    copy(
        &mut target.audio.format,
        &base.audio.format,
        &current.audio.format,
    );
    copy(
        &mut target.audio.rate,
        &base.audio.rate,
        &current.audio.rate,
    );
    copy(
        &mut target.audio.channels,
        &base.audio.channels,
        &current.audio.channels,
    );
    copy(
        &mut target.audio.auto_reconnect,
        &base.audio.auto_reconnect,
        &current.audio.auto_reconnect,
    );
    copy(
        &mut target.audio.loop_file,
        &base.audio.loop_file,
        &current.audio.loop_file,
    );

    copy(
        &mut target.appearance.palette,
        &base.appearance.palette,
        &current.appearance.palette,
    );
    copy(
        &mut target.appearance.follow_theme,
        &base.appearance.follow_theme,
        &current.appearance.follow_theme,
    );
    copy(
        &mut target.appearance.fps,
        &base.appearance.fps,
        &current.appearance.fps,
    );
    copy(
        &mut target.appearance.color_depth,
        &base.appearance.color_depth,
        &current.appearance.color_depth,
    );
    copy(
        &mut target.appearance.glyphs,
        &base.appearance.glyphs,
        &current.appearance.glyphs,
    );
    copy(
        &mut target.appearance.sensitivity,
        &base.appearance.sensitivity,
        &current.appearance.sensitivity,
    );

    copy(&mut target.playlists, &base.playlists, &current.playlists);
}

/// `$XDG_CONFIG_HOME/vibe/config.toml`, or `~/.config/vibe/config.toml`.
pub fn config_path(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_config_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))
        .map(|base| base.join("vibe/config.toml"))
}

/// Load a config file, returning defaults plus warnings for unreadable or
/// malformed files. A missing file is the normal first-run case, not a warning.
pub fn load(path: &Path) -> (Config, Vec<String>) {
    let mut warnings = Vec::new();
    match fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(config) => (config, warnings),
            Err(error) => {
                warnings.push(format!("config: {}: {error}", path.display()));
                (Config::default(), warnings)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Config::default(), warnings),
        Err(error) => {
            warnings.push(format!("config: {}: {error}", path.display()));
            (Config::default(), warnings)
        }
    }
}

/// Serialize and atomically write a config file (temp file + rename).
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
    atomic_write(path, text.as_bytes())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".into());
    let temp = path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    match result {
        Ok(()) => fs::rename(&temp, path),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_prefers_xdg_then_home() {
        assert_eq!(
            config_path(Some(Path::new("/cfg")), Some(Path::new("/home/willow"))),
            Some(PathBuf::from("/cfg/vibe/config.toml"))
        );
        assert_eq!(
            config_path(None, Some(Path::new("/home/willow"))),
            Some(PathBuf::from("/home/willow/.config/vibe/config.toml"))
        );
        assert_eq!(
            config_path(Some(Path::new("")), Some(Path::new("/home/willow"))),
            config_path(None, Some(Path::new("/home/willow")))
        );
        assert_eq!(config_path(None, None), None);
    }

    #[test]
    fn defaults_roundtrip_through_save_and_load() {
        let directory =
            std::env::temp_dir().join(format!("vibe-config-{}-{}", std::process::id(), unique()));
        let path = directory.join("vibe/config.toml");
        let config = Config::default();
        save(&path, &config).unwrap();
        let (loaded, warnings) = load(&path);
        assert!(warnings.is_empty());
        assert_eq!(loaded, config);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_file_falls_back_to_defaults_without_warning() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-config-missing-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        let (loaded, warnings) = load(&path);
        assert!(warnings.is_empty());
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn malformed_file_falls_back_to_defaults_with_a_warning() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-config-bad-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[audio\nsource = 3\n").unwrap();
        let (loaded, warnings) = load(&path);
        assert_eq!(loaded, Config::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("config:"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn auto_advance_defaults_and_partial_values_fill_in() {
        assert!(AudioConfig::default().auto_reconnect);
        let auto = AutoAdvanceConfig::default();
        assert_eq!(auto.mode, "off");
        assert_eq!(auto.transition_secs, 20.0);
        assert_eq!(auto.within, "all");

        let directory = std::env::temp_dir().join(format!(
            "vibe-config-auto-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[auto_advance]\nmode = \"timer\"\ntransition_secs = 20\n",
        )
        .unwrap();
        let (loaded, warnings) = load(&path);
        assert!(warnings.is_empty());
        assert_eq!(loaded.auto_advance.mode, "timer");
        assert_eq!(loaded.auto_advance.transition_secs, 20.0);
        assert_eq!(loaded.auto_advance.transition_secs, 20.0);
        assert_eq!(loaded.auto_advance.within, "all");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn partial_file_keeps_explicit_values_and_fills_the_rest() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-config-partial-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[audio]\nsource = \"file\"\nrate = 44100\n").unwrap();
        let (loaded, _) = load(&path);
        assert_eq!(loaded.audio.source, "file");
        assert_eq!(loaded.audio.rate, 44100);
        assert_eq!(loaded.audio.channels, 2);
        assert_eq!(loaded.appearance, AppearanceConfig::default());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_scenes_config_ignores_the_removed_playlists_field() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-config-legacy-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[scenes]\nfilter = \"favourites\"\nplaylists = [\"A\", \"B\"]\n",
        )
        .unwrap();
        let (loaded, warnings) = load(&path);
        assert!(warnings.is_empty());
        assert_eq!(loaded.scenes.filter, "favourites");
        assert!(loaded.scenes.favourites.is_empty());
        assert!(loaded.scenes.filter_tags.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn playlists_roundtrip_and_fill_in_defaults() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-config-playlists-{}-{}",
            std::process::id(),
            unique()
        ));
        let path = directory.join("vibe/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[[playlists]]\nname = \"Chill\"\nscenes = [\"Julia\", \"Waveform Scope\"]\n",
        )
        .unwrap();
        let (loaded, warnings) = load(&path);
        assert!(warnings.is_empty());
        assert_eq!(loaded.playlists.len(), 1);
        assert_eq!(loaded.playlists[0].name, "Chill");
        assert_eq!(
            loaded.playlists[0].scenes,
            vec!["Julia".to_owned(), "Waveform Scope".to_owned()]
        );
        assert_eq!(loaded.audio, AudioConfig::default());
        fs::remove_dir_all(directory).unwrap();
    }

    fn unique() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
    #[test]
    fn merge_changed_keeps_cli_overrides_out_of_the_file() {
        let file = Config::default();
        let mut effective = file.clone();
        effective.appearance.palette = "Ember".into();
        effective.appearance.fps = 30;
        let mut edited = effective.clone();
        edited.audio.rate = 44_100;
        edited.appearance.glyphs = "ascii".into();

        let mut saved = file.clone();
        merge_changed(&mut saved, &effective, &edited);
        assert_eq!(saved.appearance.palette, "Terminal");
        assert_eq!(saved.appearance.fps, 60);
        assert_eq!(saved.audio.rate, 44_100);
        assert_eq!(saved.appearance.glyphs, "ascii");
    }
}
