mod app;
mod audio;
mod config;
mod families;
mod palette;
mod preferences;
mod state;
mod ui;
mod visuals;

use app::{App, Flow, Options};
use crossterm::event::Event;
use std::{
    io,
    time::{Duration, Instant},
};

const DEFAULT_SCENE: &str = "Liquid Topography";

#[derive(Clone, Copy, clap::ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum AudioSourceArg {
    Auto,
    Pipewire,
    Pulse,
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
    after_help = "KEYS\n  m             Open menu   Esc  Close popup\n  ← / →         Previous / next visualisation\n  q             Quit\n\nCaptures the configured output monitor, never your microphone.\nUse --list-audio to discover sinks; --audio-source command/file for other backends.\nTempo needs several seconds of a clear rhythm; half/double-time ambiguity is possible."
)]
struct Args {
    /// Colour output policy
    #[arg(long, value_enum, default_value = "auto")]
    color: ColorMode,
    /// Use a synthetic 120 BPM beat instead of live audio
    #[arg(long)]
    demo: bool,
    /// Audio capture backend
    #[arg(long, value_enum, default_value = "auto", value_name = "SOURCE")]
    audio_source: AudioSourceArg,
    /// Command that writes raw PCM to stdout (used with --audio-source command)
    #[arg(long, value_name = "COMMAND")]
    audio_command: Option<String>,
    /// Raw PCM file (used with --audio-source file)
    #[arg(long, value_name = "PATH")]
    audio_file: Option<std::path::PathBuf>,
    /// Raw PCM sample format
    #[arg(long, value_enum, default_value = "f32le", value_name = "FORMAT")]
    audio_format: audio::SampleFormat,
    /// Raw PCM sample rate
    #[arg(long, default_value_t = 48000, value_parser = clap::value_parser!(u32).range(8_000..=192_000))]
    audio_rate: u32,
    /// Raw PCM channel count
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..=32))]
    audio_channels: u16,
    /// PipeWire/PulseAudio sink node name or serial
    #[arg(long = "audio-target", visible_alias = "target", value_name = "NODE")]
    audio_target: Option<String>,
    /// List available audio sinks and sources, then exit
    #[arg(long)]
    list_audio: bool,
    /// Render frame-rate cap
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=240))]
    fps: u32,
    /// Terminal colour depth (auto detects truecolour/256/16 colours)
    #[arg(long, value_enum, default_value = "auto")]
    color_depth: palette::ColorDepth,
    /// Glyph set; ascii replaces non-ASCII symbols after rendering
    #[arg(long, value_enum, default_value = "auto")]
    glyphs: palette::GlyphMode,
    /// Re-query terminal colours every second instead of only at startup
    #[arg(long)]
    follow_theme: bool,
    /// Starting visualisation; otherwise resume the last one (see --list)
    #[arg(long, value_name = "NAME")]
    scene: Option<String>,
    /// Starting colour palette (see --list)
    #[arg(long, value_name = "NAME")]
    palette: Option<String>,
    /// Visual response strength; does not alter the tempo estimator
    #[arg(long, default_value = "1", value_parser = gain_arg)]
    sensitivity: f32,
    /// List visualisations and terminal-derived palettes
    #[arg(long)]
    list: bool,
    /// Analyze live audio for 1–60 seconds, then print features (default: 12)
    #[arg(long, num_args=0..=1, default_missing_value="12", value_name="SECONDS", value_parser=clap::value_parser!(u64).range(1..=60))]
    check_audio: Option<u64>,
}

fn choice(name: Option<&str>, choices: &[&str], kind: &str) -> io::Result<usize> {
    match name {
        None => Ok(0),
        Some(name) => choices
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown {kind} '{name}'; choose: {}", choices.join(", ")),
                )
            }),
    }
}

fn starting_scene(explicit: Option<&str>, saved: Option<&str>) -> io::Result<usize> {
    if explicit.is_some() {
        return choice(explicit, visuals::SCENES, "visualisation");
    }
    if let Some(index) = saved.and_then(|name| {
        visuals::SCENES
            .iter()
            .position(|scene| scene.eq_ignore_ascii_case(name))
    }) {
        return Ok(index);
    }
    choice(Some(DEFAULT_SCENE), visuals::SCENES, "visualisation")
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
            "No samples received from {source}; check --list-audio and the selected sink"
        )));
    }
    if state.rms <= 0.001 {
        return Err(io::Error::other(format!(
            "Capture from {source} is connected but silent; run --list-audio and choose a real output sink"
        )));
    }
    Ok(())
}

fn main() -> io::Result<()> {
    use clap::Parser;
    let args = Args::parse();
    if args.list {
        println!("Visualisations: {}", visuals::SCENES.join(", "));
        println!("Palettes: {}", palette::PALETTES.join(", "));
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
            println!("  Use --audio-target with a sink name or serial. Sources are inputs, not output monitors.");
        }
        return Ok(());
    }
    let state_path = if args.check_audio.is_none() {
        preferences::state_path(
            std::env::var_os("XDG_STATE_HOME")
                .as_deref()
                .map(std::path::Path::new),
            std::env::var_os("HOME")
                .as_deref()
                .map(std::path::Path::new),
        )
    } else {
        None
    };
    let saved_scene = state_path.as_deref().and_then(preferences::load);
    let initial_scene = starting_scene(args.scene.as_deref(), saved_scene.as_deref())?;
    let initial_palette = choice(args.palette.as_deref(), palette::PALETTES, "palette")?;
    let backend = match args.audio_source {
        AudioSourceArg::Auto if args.audio_command.is_some() => {
            audio::Backend::Command(args.audio_command.clone().expect("checked command"))
        }
        AudioSourceArg::Auto if args.audio_file.is_some() => {
            audio::Backend::File(args.audio_file.clone().expect("checked file"))
        }
        AudioSourceArg::Auto => audio::Backend::Auto,
        AudioSourceArg::Pipewire => audio::Backend::PipeWire,
        AudioSourceArg::Pulse => audio::Backend::Pulse,
        AudioSourceArg::Command => {
            audio::Backend::Command(args.audio_command.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--audio-source command needs --audio-command",
                )
            })?)
        }
        AudioSourceArg::File => audio::Backend::File(args.audio_file.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--audio-source file needs --audio-file",
            )
        })?),
    };
    let format = audio::PcmFormat {
        rate: args.audio_rate,
        channels: args.audio_channels,
        sample: args.audio_format,
    }
    .validate()?;
    let capture_config = audio::CaptureConfig {
        backend,
        target: args.audio_target.clone(),
        format,
    };
    if let Some(seconds) = args.check_audio {
        let capture = audio::Capture::start(capture_config)?;
        check_audio(&capture, Duration::from_secs(seconds))?;
        return Ok(());
    }
    let mut app = App::new(Options {
        scene: initial_scene,
        palette: initial_palette,
        sensitivity: args.sensitivity,
        demo: args.demo,
        capture: capture_config,
    });
    match args.color {
        ColorMode::Auto => {}
        ColorMode::Always => crossterm::style::force_color_output(true),
        ColorMode::Never => crossterm::style::force_color_output(false),
    }

    let mut renderer = visuals::Renderer::default();
    let mut terminal = ratatui::init();
    let frame_time = Duration::from_secs_f32(1.0 / args.fps as f32);
    let color_depth = args.color_depth.resolve();
    let glyph_mode = args.glyphs.resolve();
    let result = (|| -> io::Result<usize> {
        let mut input = palette::TerminalInput::open(args.follow_theme)?;
        let start = Instant::now();
        'frames: loop {
            let frame_start = Instant::now();
            let t = start.elapsed().as_secs_f32();
            {
                let view = app.view(t);
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
                    if let Some(menu) = view.menu {
                        ui::menu(frame, menu.title, menu.items, menu.selected, &input.theme);
                    }
                    if glyph_mode == palette::GlyphMode::Ascii {
                        palette::ascii_buffer(frame.buffer_mut());
                    }
                    palette::quantize_buffer(frame.buffer_mut(), color_depth);
                })?;
            }
            // Handle queued keys in order without rendering intermediate scenes.
            // If a frame overruns, keep a small idle slice instead of polling
            // immediately; `--fps` can lower the target for slow terminals.
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
        }
        Ok(app.scene())
    })();
    ratatui::restore();
    let scene = result?;
    if let Some(path) = state_path {
        if let Err(error) = preferences::save(&path, visuals::SCENES[scene]) {
            eprintln!(
                "Could not save last visualisation to {}: {error}",
                path.display()
            );
        }
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
}
