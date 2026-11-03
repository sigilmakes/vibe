mod app;
mod audio;
mod config;
mod families;
mod flow;
mod menu;
mod now_playing;
mod palette;
mod state;
mod ui;
mod visuals;

use app::{App, Flow, Options};
use crossterm::event::Event;
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

const DEFAULT_SCENE: &str = "Liquid Topography";

#[derive(Clone, Copy, clap::ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum AudioSourceArg {
    Auto,
    Pipewire,
    Pulse,
    Microphone,
    Command,
    File,
}

fn gain_arg(input: &str) -> Result<f32, String> {
    let value: f32 = input
        .parse()
        .map_err(|_| "expected a number from 0.25 to 3".to_string())?;
    if (0.25..=3.0).contains(&value) {
        Ok(value)
    } else {
        Err("sensitivity must be from 0.25 to 3".to_string())
    }
}

#[derive(clap::Parser)]
#[command(
    version,
    about = "A psychedelic, audio-reactive terminal playground",
    after_help = "KEYS\n  m             Open menu   Esc  Close popup\n  ← / →         Previous / next visualisation\n  q             Quit\n\nCaptures the configured output monitor by default; microphone capture is opt-in.\nUse --list-audio to discover sinks and sources; --audio-source command/file for other backends.\nTempo needs several seconds of a clear rhythm; half/double-time ambiguity is possible."
)]
struct Args {
    /// Colour output policy
    #[arg(long, value_enum, default_value = "auto")]
    color: ColorMode,
    /// Use a synthetic 120 BPM beat instead of live audio
    #[arg(long)]
    demo: bool,
    /// Audio capture backend
    #[arg(long, value_enum, value_name = "SOURCE")]
    audio_source: Option<AudioSourceArg>,
    /// Command that writes raw PCM to stdout (used with --audio-source command)
    #[arg(long, value_name = "COMMAND")]
    audio_command: Option<String>,
    /// Raw PCM file (used with --audio-source file)
    #[arg(long, value_name = "PATH")]
    audio_file: Option<PathBuf>,
    /// Raw PCM sample format
    #[arg(long, value_enum, value_name = "FORMAT")]
    audio_format: Option<audio::SampleFormat>,
    /// Raw PCM sample rate
    #[arg(long, value_parser = clap::value_parser!(u32).range(8_000..=192_000))]
    audio_rate: Option<u32>,
    /// Raw PCM channel count
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=32))]
    audio_channels: Option<u16>,
    /// PipeWire/PulseAudio sink node name or serial
    #[arg(long = "audio-target", visible_alias = "target", value_name = "NODE")]
    audio_target: Option<String>,
    /// List available audio sinks and sources, then exit
    #[arg(long)]
    list_audio: bool,
    /// Render frame-rate cap
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=240))]
    fps: Option<u32>,
    /// Terminal colour depth (auto detects truecolour/256/16 colours)
    #[arg(long, value_enum)]
    color_depth: Option<palette::ColorDepth>,
    /// Glyph set; ascii replaces non-ASCII symbols after rendering
    #[arg(long, value_enum)]
    glyphs: Option<palette::GlyphMode>,
    /// Re-query terminal colours every second instead of only at startup
    #[arg(long, action = clap::ArgAction::SetTrue)]
    follow_theme: Option<bool>,
    /// Starting visualisation; otherwise resume the last one (see --list)
    #[arg(long, value_name = "NAME")]
    scene: Option<String>,
    /// Starting colour palette (see --list)
    #[arg(long, value_name = "NAME")]
    palette: Option<String>,
    /// Visual response strength; does not alter the tempo estimator
    #[arg(long, value_parser = gain_arg)]
    sensitivity: Option<f32>,
    /// Path to a config file (defaults to $XDG_CONFIG_HOME/vibe/config.toml)
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// List visualisations and palettes, one per line, then exit
    #[arg(long)]
    list: bool,
    /// Analyze live audio for 1–60 seconds, then print features (default: 12)
    #[arg(long, num_args=0..=1, default_missing_value="12", value_name="SECONDS", value_parser=clap::value_parser!(u64).range(1..=60))]
    check_audio: Option<u64>,
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn config_path(args: &Args) -> PathBuf {
    args.config.clone().unwrap_or_else(|| {
        config::config_path(
            env_path("XDG_CONFIG_HOME").as_deref(),
            env_path("HOME").as_deref(),
        )
        .unwrap_or_else(|| PathBuf::from(".config/vibe/config.toml"))
    })
}

/// Case-insensitive name match. ASCII names match without allocating; accented
/// names (for example "Macramé Knots") fall back to Unicode lowercasing.
fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right) || left.to_lowercase() == right.to_lowercase()
}

fn choice(name: Option<&str>, choices: &[&str], kind: &str) -> io::Result<usize> {
    match name {
        None => Ok(0),
        Some(name) => choices
            .iter()
            .position(|c| same_name(c, name))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, unknown(kind, name, choices))
            }),
    }
}

/// Error text for an unknown name. The catalogue is too long to print on one
/// line, so offer a few close names and point at the full listing.
fn unknown(kind: &str, name: &str, choices: &[&str]) -> String {
    let needle = name.trim().to_lowercase();
    let close: Vec<&str> = choices
        .iter()
        .copied()
        .filter(|choice| {
            let candidate = choice.to_lowercase();
            needle.len() >= 2 && (candidate.contains(&needle) || needle.contains(&candidate))
        })
        .take(3)
        .collect();
    if close.is_empty() {
        format!("unknown {kind} '{name}'; run `vibe --list` for the full catalogue")
    } else {
        format!(
            "unknown {kind} '{name}'; did you mean: {}?",
            close.join(", ")
        )
    }
}

fn starting_scene(explicit: Option<&str>, saved: Option<&str>) -> io::Result<usize> {
    if explicit.is_some() {
        return choice(explicit, visuals::SCENES, "visualisation");
    }
    if let Some(index) = saved.and_then(|name| {
        visuals::SCENES
            .iter()
            .position(|scene| same_name(scene, name))
    }) {
        return Ok(index);
    }
    choice(Some(DEFAULT_SCENE), visuals::SCENES, "visualisation")
}

/// `--list` output: one entry per line so a long catalogue stays readable and
/// each name can be copied straight into `--scene`.
fn catalogue() -> String {
    let mut out = format!("Visualisations ({}):\n", visuals::SCENES.len());
    for scene in visuals::SCENES {
        out.push_str(&format!("  {scene}\n"));
    }
    out.push_str(&format!("Palettes ({}):\n", palette::PALETTES.len()));
    for name in palette::PALETTES {
        out.push_str(&format!("  {name}\n"));
    }
    out
}

fn color_depth_name(depth: palette::ColorDepth) -> &'static str {
    match depth {
        palette::ColorDepth::Auto => "auto",
        palette::ColorDepth::Truecolor => "truecolor",
        palette::ColorDepth::Ansi256 => "ansi256",
        palette::ColorDepth::Ansi16 => "ansi16",
        palette::ColorDepth::Mono => "mono",
    }
}

fn glyphs_name(glyphs: palette::GlyphMode) -> &'static str {
    match glyphs {
        palette::GlyphMode::Auto => "auto",
        palette::GlyphMode::Unicode => "unicode",
        palette::GlyphMode::Ascii => "ascii",
    }
}

fn source_name(source: AudioSourceArg) -> &'static str {
    match source {
        AudioSourceArg::Auto => "auto",
        AudioSourceArg::Pipewire => "pipewire",
        AudioSourceArg::Pulse => "pulse",
        AudioSourceArg::Microphone => "microphone",
        AudioSourceArg::Command => "command",
        AudioSourceArg::File => "file",
    }
}

/// Seed the in-memory config from explicit CLI flags. These are session
/// overrides: they replace the file value but are never written back.
fn apply_cli(config: &mut config::Config, args: &Args) {
    if let Some(palette) = &args.palette {
        config.appearance.palette = palette.clone();
    }
    if let Some(fps) = args.fps {
        config.appearance.fps = fps;
    }
    if let Some(depth) = args.color_depth {
        config.appearance.color_depth = color_depth_name(depth).to_owned();
    }
    if let Some(glyphs) = args.glyphs {
        config.appearance.glyphs = glyphs_name(glyphs).to_owned();
    }
    if args.follow_theme == Some(true) {
        config.appearance.follow_theme = true;
    }
    if let Some(sensitivity) = args.sensitivity {
        config.appearance.sensitivity = sensitivity;
    }
    let explicit_source = args.audio_source.is_some();
    if let Some(source) = args.audio_source {
        config.audio.source = source_name(source).to_owned();
    }
    if let Some(command) = &args.audio_command {
        config.audio.command = command.clone();
        if !explicit_source {
            config.audio.source = "command".into();
        }
    }
    if let Some(file) = &args.audio_file {
        config.audio.file = file.display().to_string();
        if !explicit_source && args.audio_command.is_none() {
            config.audio.source = "file".into();
        }
    }
    if let Some(format) = args.audio_format {
        config.audio.format = format.name().to_owned();
    }
    if let Some(rate) = args.audio_rate {
        config.audio.rate = rate;
    }
    if let Some(channels) = args.audio_channels {
        config.audio.channels = channels;
    }
    if let Some(target) = &args.audio_target {
        config.audio.target = target.clone();
    }
}

/// Cross-flag checks that clap cannot express on a single argument.
fn validate_args(args: &Args) -> io::Result<()> {
    if args.audio_source == Some(AudioSourceArg::Command) && args.audio_command.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--audio-source command needs --audio-command",
        ));
    }
    if args.audio_source == Some(AudioSourceArg::File) && args.audio_file.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--audio-source file needs --audio-file",
        ));
    }
    if let Some(name) = &args.palette {
        if !palette::PALETTES
            .iter()
            .any(|p| p.eq_ignore_ascii_case(name))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "unknown palette '{name}'; choose: {}",
                    palette::PALETTES.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

fn check_audio(capture: &audio::Capture, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut state = capture.snapshot();
    while Instant::now() < deadline {
        state = capture.snapshot();
        if state.error.is_some() || state.rms > 0.001 {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let source = state.source.describe();
    println!(
        "source={} connected={} signal={:.4} rms={:.4} energy={:.2} bass={:.2} mid={:.2} treble={:.2} onset={:.2} kick={:.2} tempo={} confidence={:.2} silent_for={:.1}s",
        source,
        state.connected,
        state.peak,
        state.rms,
        state.energy,
        state.bass,
        state.mid,
        state.treble,
        state.onset,
        state.kick,
        state
            .bpm
            .map(|bpm| format!("{bpm:.0} BPM"))
            .unwrap_or_else(|| "learning".into()),
        state.tempo_confidence,
        state.silent_for,
    );
    if let Some(error) = state.error {
        return Err(io::Error::other(error));
    }
    if !state.connected {
        return Err(io::Error::other(format!(
            "No samples received from {source}; check --list-audio and the selected device"
        )));
    }
    if state.rms <= 0.001 {
        return Err(io::Error::other(format!(
            "Capture from {source} is connected but silent; check the selected device and whether it is playing"
        )));
    }
    Ok(())
}

fn main() -> io::Result<()> {
    use clap::Parser;
    let args = Args::parse();
    if args.list {
        print!("{}", catalogue());
        return Ok(());
    }
    if args.list_audio {
        let devices = audio::list_audio()?;
        let mut sinks = devices
            .iter()
            .filter(|device| device.kind == "sink")
            .peekable();
        let mut sources = devices
            .iter()
            .filter(|device| device.kind == "source")
            .peekable();
        if sinks.peek().is_none() {
            println!("Sinks: none. PipeWire has no audio output; check `wpctl status`.");
        } else {
            println!("Sinks:");
            for device in sinks {
                let marker = if device.default { "*" } else { " " };
                println!(
                    "  {marker} {}{} — {}{}{}",
                    device.name,
                    device
                        .serial
                        .map(|serial| format!(" [serial {serial}]"))
                        .unwrap_or_default(),
                    device.description,
                    device
                        .channels
                        .map(|channels| format!(" · {channels} ch"))
                        .unwrap_or_default(),
                    device
                        .rate
                        .map(|rate| format!(" · {rate} Hz"))
                        .unwrap_or_default(),
                );
            }
        }
        if sources.peek().is_some() {
            println!("Sources:");
            for device in sources {
                let marker = if device.default { "*" } else { " " };
                println!(
                    "  {marker} {}{} — {}{}{}",
                    device.name,
                    device
                        .serial
                        .map(|serial| format!(" [serial {serial}]"))
                        .unwrap_or_default(),
                    device.description,
                    device
                        .channels
                        .map(|channels| format!(" · {channels} ch"))
                        .unwrap_or_default(),
                    device
                        .rate
                        .map(|rate| format!(" · {rate} Hz"))
                        .unwrap_or_default(),
                );
            }
            println!(
                "  Use --audio-target with a sink or source name or serial; microphone capture is opt-in with --audio-source microphone."
            );
        }
        return Ok(());
    }

    validate_args(&args)?;

    let config_path = config_path(&args);
    let mut warnings = Vec::new();
    let (file_config, config_warnings) = config::load(&config_path);
    warnings.extend(config_warnings);
    let mut config = file_config.clone();
    apply_cli(&mut config, &args);

    let state_path = state::state_path(
        env_path("XDG_STATE_HOME").as_deref(),
        env_path("HOME").as_deref(),
    );
    let legacy = state::legacy_scene_path(
        env_path("XDG_STATE_HOME").as_deref(),
        env_path("HOME").as_deref(),
    );
    let mut state_data = state::State::default();
    if let Some(path) = &state_path {
        let (loaded, state_warnings) = state::load(path, legacy.as_deref());
        state_data = loaded;
        warnings.extend(state_warnings);
    }

    for warning in &warnings {
        eprintln!("{warning}");
    }

    if let Some(seconds) = args.check_audio {
        let capture = audio::Capture::start(audio::capture_config(&config.audio))?;
        check_audio(&capture, Duration::from_secs(seconds))?;
        return Ok(());
    }

    let saved_scene = (!state_data.last_scene.is_empty()).then_some(state_data.last_scene.as_str());
    let initial_scene = starting_scene(args.scene.as_deref(), saved_scene)?;
    let active_playlist = state_data.active_playlist;

    let mut app = App::new(Options {
        scene: initial_scene,
        config,
        persisted_config: file_config,
        demo: args.demo,
        config_path: config_path.clone(),
        state_path: state_path.clone(),
        active_playlist,
    });
    match args.color {
        ColorMode::Auto => {}
        ColorMode::Always => crossterm::style::force_color_output(true),
        ColorMode::Never => crossterm::style::force_color_output(false),
    }

    let mut renderer = visuals::Renderer::default();
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        let mut input = palette::TerminalInput::open(false)?;
        let start = Instant::now();
        'frames: loop {
            let frame_start = Instant::now();
            let t = start.elapsed().as_secs_f32();
            let frame_time = {
                let view = app.view(t);
                let frame_time = Duration::from_secs_f32(1.0 / view.fps as f32);
                let color_depth = view.color_depth.resolve();
                let glyph_mode = view.glyphs.resolve();
                input.set_follow_theme(view.follow_theme);
                terminal.draw(|frame| {
                    let area = frame.area();
                    renderer.draw(
                        frame.buffer_mut(),
                        area,
                        t,
                        view.scene,
                        view.palette,
                        view.bands,
                        view.reactivity,
                        &input.theme,
                    );
                    ui::draw(frame, view.analysis, &view.status(), &input.theme);
                    if let Some(menu) = &view.menu {
                        ui::menu(frame, menu, &input.theme);
                    }
                    if glyph_mode == palette::GlyphMode::Ascii {
                        palette::ascii_buffer(frame.buffer_mut());
                    }
                    palette::quantize_buffer(frame.buffer_mut(), color_depth);
                })?;
                frame_time
            };
            app.tick(t);
            // Handle queued keys in order without rendering intermediate scenes.
            let elapsed = frame_start.elapsed();
            let mut timeout = if elapsed < frame_time {
                frame_time - elapsed
            } else {
                (frame_time / 4).max(Duration::from_millis(2))
            };
            while let Some(event) = input.read(timeout)? {
                timeout = Duration::ZERO;
                let Event::Key(key) = event else { continue };
                if app.handle_key(key) == Flow::Quit {
                    break 'frames;
                }
            }
            app.maybe_persist(Instant::now());
        }
        Ok(())
    })();
    ratatui::restore();
    app.flush();
    result?;
    for error in app.take_save_errors() {
        eprintln!("{error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_cli_overrides_saved_name_and_missing_or_stale_state_uses_liquid() {
        use clap::Parser;
        let args = Args::try_parse_from(["vibe"]).unwrap();
        assert!(args.scene.is_none());
        for saved in [None, Some("retired visualisation"), Some("")] {
            let index = starting_scene(None, saved).unwrap();
            assert_eq!(visuals::SCENES[index], DEFAULT_SCENE);
        }
        let resumed = starting_scene(None, Some("Julia")).unwrap();
        assert_eq!(visuals::SCENES[resumed], "Julia");
        let args = Args::try_parse_from(["vibe", "--scene", "Waveform Scope"]).unwrap();
        let override_scene = starting_scene(args.scene.as_deref(), Some("Julia")).unwrap();
        assert_eq!(visuals::SCENES[override_scene], "Waveform Scope");
        assert!(starting_scene(Some("missing scene"), Some("Julia")).is_err());
    }

    #[test]
    fn unknown_names_get_short_actionable_errors() {
        let message = unknown("visualisation", "nope", visuals::SCENES);
        assert!(message.len() < 160, "{message}");
        assert!(message.contains("--list"), "{message}");

        let close = unknown("visualisation", "jul", visuals::SCENES);
        assert_eq!(close, "unknown visualisation 'jul'; did you mean: Julia?");
        assert!(unknown("visualisation", "wave", visuals::SCENES).contains("Waveform Scope"));
    }

    #[test]
    fn catalogue_prints_one_entry_per_line() {
        let listing = catalogue();
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(
            lines.first().copied(),
            Some(format!("Visualisations ({}):", visuals::SCENES.len()).as_str())
        );
        assert!(lines.iter().all(|line| line.chars().count() <= 40));
        for scene in visuals::SCENES {
            assert!(lines.contains(&format!("  {scene}").as_str()), "{scene}");
        }
        for name in palette::PALETTES {
            assert!(lines.contains(&format!("  {name}").as_str()), "{name}");
        }
    }

    #[test]
    fn accented_scene_names_match_case_insensitively() {
        let scene = visuals::SCENES
            .iter()
            .position(|name| *name == "Macramé Knots")
            .unwrap();
        assert!(same_name("Macramé Knots", "MACRAMÉ KNOTS"));
        assert_eq!(starting_scene(None, Some("MACRAMÉ KNOTS")).unwrap(), scene);
        assert_eq!(starting_scene(Some("macramé knots"), None).unwrap(), scene);
    }

    #[test]
    fn invalid_sensitivity_and_fps_are_rejected() {
        use clap::Parser;
        for value in ["NaN", "-2", "0", "4"] {
            assert!(Args::try_parse_from(["vibe", "--sensitivity", value]).is_err());
        }
        for value in ["0", "241", "many"] {
            assert!(Args::try_parse_from(["vibe", "--fps", value]).is_err());
        }
        for value in ["0", "192001"] {
            assert!(Args::try_parse_from(["vibe", "--audio-rate", value]).is_err());
        }
    }

    #[test]
    fn cli_flags_seed_the_config_and_config_path_defaults_to_xdg() {
        use clap::Parser;
        let args = Args::try_parse_from([
            "vibe",
            "--fps",
            "30",
            "--palette",
            "Ember",
            "--audio-source",
            "command",
            "--audio-command",
            "parec",
        ])
        .unwrap();
        let mut config = config::Config::default();
        apply_cli(&mut config, &args);
        assert_eq!(config.appearance.fps, 30);
        assert_eq!(config.appearance.palette, "Ember");
        assert_eq!(config.audio.source, "command");
        assert_eq!(config.audio.command, "parec");
        // Absent flags leave the config defaults in place.
        assert_eq!(config.appearance.sensitivity, 1.0);
        assert_eq!(config.audio.rate, 48_000);
    }

    #[test]
    fn explicit_command_and_file_sources_need_their_payloads() {
        use clap::Parser;
        let command = Args::try_parse_from(["vibe", "--audio-source", "command"]).unwrap();
        let error = validate_args(&command).unwrap_err();
        assert!(error.to_string().contains("--audio-command"));

        let file = Args::try_parse_from(["vibe", "--audio-source", "file"]).unwrap();
        let error = validate_args(&file).unwrap_err();
        assert!(error.to_string().contains("--audio-file"));

        let with_payload = Args::try_parse_from([
            "vibe",
            "--audio-source",
            "command",
            "--audio-command",
            "parec",
        ])
        .unwrap();
        assert!(validate_args(&with_payload).is_ok());

        let unknown_palette = Args::try_parse_from(["vibe", "--palette", "Nope"]).unwrap();
        assert!(validate_args(&unknown_palette).is_err());
    }

    #[test]
    fn command_and_file_payload_flags_imply_their_source() {
        use clap::Parser;
        let mut config = config::Config::default();
        let args = Args::try_parse_from(["vibe", "--audio-command", "parec"]).unwrap();
        apply_cli(&mut config, &args);
        assert_eq!(config.audio.source, "command");

        let mut config = config::Config::default();
        let args = Args::try_parse_from(["vibe", "--audio-file", "track.raw"]).unwrap();
        apply_cli(&mut config, &args);
        assert_eq!(config.audio.source, "file");

        let mut config = config::Config::default();
        let args =
            Args::try_parse_from(["vibe", "--audio-source", "auto", "--audio-command", "parec"])
                .unwrap();
        apply_cli(&mut config, &args);
        assert_eq!(config.audio.source, "auto");
    }
}
