mod audio;
mod ui;
mod visuals;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::{
    io,
    time::{Duration, Instant},
};

const FRAME_TIME: Duration = Duration::from_millis(16);
const SCENE_SECONDS: f32 = 20.0;
const TRANSITION_SECONDS: f32 = 2.5;

fn demo_audio(t: f32) -> audio::AudioState {
    let phase = (t * 2.0).fract();
    let kick = (-phase * 18.0).exp();
    let hat = (-((t * 8.0).fract()) * 20.0).exp();
    let mut state = audio::AudioState::default();
    for (i, band) in state.bands.iter_mut().enumerate() {
        let f = i as f32;
        *band = if i < 9 {
            kick * (0.9 - f * 0.04)
        } else {
            ((t * 1.3 + f * 0.6).sin() * 0.5 + 0.5) * 0.35 + hat * 0.25
        };
    }
    state.energy = 0.2 + kick * 0.65;
    state.peak = state.energy;
    state.rms = state.energy * 0.1;
    state.bass = kick;
    state.mid = 0.25 + (t * 1.3).sin() * 0.15;
    state.treble = hat * 0.7;
    state.onset = kick.max(hat * 0.4);
    state.kick = kick;
    state.beat = kick;
    state.beat_phase = phase;
    state.waveform =
        std::array::from_fn(|i| (i as f32 * 0.4 + t * 12.0).sin() * (0.3 + kick * 0.6));
    state.bpm = Some(120.0);
    state.tempo_confidence = 1.0;
    state.connected = true;
    state
}

fn sensitivity(state: &audio::AudioState, gain: f32) -> visuals::Reactivity {
    let scale = |value: f32| (value * gain).clamp(0.0, 1.0);
    visuals::Reactivity {
        energy: scale(state.energy),
        bass: scale(state.bass),
        mid: scale(state.mid),
        treble: scale(state.treble),
        onset: scale(state.onset),
        kick: scale(state.kick),
        beat: scale(state.beat),
        beat_phase: state.beat_phase,
        waveform: state.waveform.map(|x| (x * gain).clamp(-1.0, 1.0)),
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}
#[derive(Clone, Copy)]
enum MenuKind {
    Root,
    Scenes,
    Palettes,
}
struct Menu {
    kind: MenuKind,
    selected: usize,
}
impl Menu {
    fn items(&self) -> &'static [&'static str] {
        match self.kind {
            MenuKind::Root => &["Scene", "Palette", "Analysis", "Controls"],
            MenuKind::Scenes => visuals::SCENES,
            MenuKind::Palettes => visuals::PALETTES,
        }
    }
    fn title(&self) -> &'static str {
        match self.kind {
            MenuKind::Root => "Chromavibe",
            MenuKind::Scenes => "Scene",
            MenuKind::Palettes => "Palette",
        }
    }
    fn step(&mut self, delta: isize) {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.items().len() as isize) as usize;
    }
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
    after_help = "KEYS\n  m             Open menu   Esc  Close popup\n  ←/→ or Space  Next scene  a    Auto scenes\n  s             Scenes      p    Palettes\n  ?             Controls\n  + / -         Sensitivity d  Demo         r  Reconnect audio\n  q             Quit\n\nCaptures the PipeWire output monitor, never your microphone.\nTempo needs several seconds of a clear rhythm; half/double-time ambiguity is possible."
)]
struct Args {
    /// Colour output policy
    #[arg(long, value_enum, default_value = "auto")]
    color: ColorMode,
    /// Use a synthetic 120 BPM beat instead of live audio
    #[arg(long)]
    demo: bool,
    /// PipeWire output node name or serial
    #[arg(long, value_name = "NODE")]
    target: Option<String>,
    /// Starting scene name (see --list)
    #[arg(long, value_name = "NAME")]
    scene: Option<String>,
    /// Starting colour palette (see --list)
    #[arg(long, value_name = "NAME")]
    palette: Option<String>,
    /// Keep the selected scene instead of cycling
    #[arg(long)]
    manual: bool,
    /// Visual response strength; does not alter the tempo estimator
    #[arg(long, default_value = "1", value_parser = gain_arg)]
    sensitivity: f32,
    /// List scenes and palettes
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

fn snapshot(
    shared: &Option<std::sync::Arc<std::sync::Mutex<audio::AudioState>>>,
) -> audio::AudioState {
    let mut state = shared
        .as_ref()
        .and_then(|s| s.lock().ok().map(|s| s.clone()))
        .unwrap_or_default();
    if state
        .updated
        .is_some_and(|t| t.elapsed() > Duration::from_millis(600))
    {
        state = audio::AudioState {
            error: Some("Audio stalled · r to reconnect".into()),
            ..Default::default()
        };
    }
    state
}

fn main() -> io::Result<()> {
    use clap::Parser;
    let args = Args::parse();
    if args.list {
        println!(
            "Scenes:   {}\nPalettes: {}",
            visuals::SCENES.join(", "),
            visuals::PALETTES.join(", ")
        );
        return Ok(());
    }
    let initial_scene = choice(args.scene.as_deref(), visuals::SCENES, "scene")?;
    let initial_palette = choice(args.palette.as_deref(), visuals::PALETTES, "palette")?;
    let mut demo = args.demo;
    let target = args.target;
    let (mut capture, mut shared, mut capture_error) = (None, None, None);
    if !demo || args.check_audio.is_some() {
        match audio::Capture::start(target.clone()) {
            Ok((handle, state)) => {
                capture = Some(handle);
                shared = Some(state);
            }
            Err(err) => capture_error = Some(err.to_string()),
        }
    }
    if let Some(seconds) = args.check_audio {
        if let Some(error) = capture_error {
            return Err(io::Error::other(error));
        }
        std::thread::sleep(Duration::from_secs(seconds));
        let state = snapshot(&shared);
        println!("connected={} rms={:.4} energy={:.2} bass={:.2} mid={:.2} treble={:.2} onset={:.2} kick={:.2} tempo={} confidence={:.2}", state.connected, state.rms, state.energy, state.bass, state.mid, state.treble, state.onset, state.kick, state.bpm.map(|bpm| format!("{bpm:.0} BPM")).unwrap_or_else(|| "learning".into()), state.tempo_confidence);
        if let Some(error) = state.error {
            return Err(io::Error::other(error));
        }
        if !state.connected {
            return Err(io::Error::other(
                "No samples received from the output monitor",
            ));
        }
        return Ok(());
    }
    match args.color {
        ColorMode::Auto => {}
        ColorMode::Always => crossterm::style::force_color_output(true),
        ColorMode::Never => crossterm::style::force_color_output(false),
    }
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        let start = Instant::now();
        let mut scene_start = 0.0;
        let mut transition_start = None;
        let (mut scene, mut palette) = (initial_scene, initial_palette);
        let mut auto = !args.manual;
        let mut analysis = false;
        let mut help = false;
        let mut menu: Option<Menu> = None;
        let mut gain = args.sensitivity;
        loop {
            let frame_start = Instant::now();
            let t = start.elapsed().as_secs_f32();
            let state = if demo {
                demo_audio(t)
            } else {
                snapshot(&shared)
            };
            if auto
                && transition_start.is_none()
                && t - scene_start >= SCENE_SECONDS
                && (state.kick > 0.5 || state.onset > 0.6 || t - scene_start > SCENE_SECONDS + 4.0)
            {
                transition_start = Some(t);
            }
            let blend = transition_start
                .map(|since| ((t - since) / TRANSITION_SECONDS).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            if blend >= 1.0 {
                scene = (scene + 1) % visuals::SCENES.len();
                scene_start = t;
                transition_start = None;
            }
            let blend = if transition_start.is_some() {
                blend
            } else {
                0.0
            };
            let reactive = sensitivity(&state, gain);
            let bands = state.bands.map(|x| (x * gain).clamp(0.0, 1.0));
            terminal.draw(|frame| {
                let area = frame.area();
                visuals::draw(
                    frame.buffer_mut(),
                    area,
                    t,
                    scene,
                    (scene + 1) % visuals::SCENES.len(),
                    blend,
                    palette,
                    &bands,
                    &reactive,
                );
                ui::draw(
                    frame,
                    analysis,
                    help,
                    &ui::Status {
                        sensitivity: gain,
                        bass: state.bass,
                        mid: state.mid,
                        treble: state.treble,
                        onset: state.onset,
                        kick: state.kick,
                        beat: state.beat,
                        bpm: state.bpm,
                        tempo_confidence: state.tempo_confidence,
                        bands: &state.bands,
                        error: if demo {
                            None
                        } else {
                            capture_error.as_deref().or(state.error.as_deref())
                        },
                    },
                );
                if let Some(menu) = &menu {
                    ui::menu(frame, menu.title(), menu.items(), menu.selected);
                }
            })?;
            if event::poll(FRAME_TIME.saturating_sub(frame_start.elapsed()))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)
                    {
                        break;
                    }
                    if let Some(mut active) = menu.take() {
                        match key.code {
                            KeyCode::Esc => {}
                            KeyCode::Char('q') => break,
                            KeyCode::Up | KeyCode::Char('k') => {
                                active.step(-1);
                                menu = Some(active);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                active.step(1);
                                menu = Some(active);
                            }
                            KeyCode::Enter => match active.kind {
                                MenuKind::Root => match active.selected {
                                    0 => {
                                        menu = Some(Menu {
                                            kind: MenuKind::Scenes,
                                            selected: scene,
                                        })
                                    }
                                    1 => {
                                        menu = Some(Menu {
                                            kind: MenuKind::Palettes,
                                            selected: palette,
                                        })
                                    }
                                    2 => analysis = true,
                                    _ => help = true,
                                },
                                MenuKind::Scenes => {
                                    scene = active.selected;
                                    scene_start = t;
                                    transition_start = None;
                                }
                                MenuKind::Palettes => palette = active.selected,
                            },
                            _ => menu = Some(active),
                        }
                        continue;
                    }
                    if analysis {
                        match key.code {
                            KeyCode::Esc => analysis = false,
                            KeyCode::Char('q') => break,
                            _ => {}
                        }
                        continue;
                    }
                    if help {
                        match key.code {
                            KeyCode::Esc => {
                                help = false;
                                continue;
                            }
                            KeyCode::Char('q') => break,
                            _ => continue,
                        }
                    }
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Right | KeyCode::Char(' ') => {
                            scene = (scene + 1) % visuals::SCENES.len();
                            scene_start = t;
                            transition_start = None;
                        }
                        KeyCode::Left => {
                            scene = (scene + visuals::SCENES.len() - 1) % visuals::SCENES.len();
                            scene_start = t;
                            transition_start = None;
                        }
                        KeyCode::Char('m') => {
                            menu = Some(Menu {
                                kind: MenuKind::Root,
                                selected: 0,
                            })
                        }
                        KeyCode::Char('s') => {
                            menu = Some(Menu {
                                kind: MenuKind::Scenes,
                                selected: scene,
                            })
                        }
                        KeyCode::Char('p') => {
                            menu = Some(Menu {
                                kind: MenuKind::Palettes,
                                selected: palette,
                            })
                        }
                        KeyCode::Char('a') => {
                            auto = !auto;
                            scene_start = t;
                            transition_start = None;
                        }
                        KeyCode::Char('?') => help = true,
                        KeyCode::Char('+') | KeyCode::Char('=') => gain = (gain + 0.1).min(3.0),
                        KeyCode::Char('-') => gain = (gain - 0.1).max(0.25),
                        KeyCode::Char('r') => {
                            drop(capture.take());
                            shared = None;
                            demo = false;
                            match audio::Capture::start(target.clone()) {
                                Ok((handle, state)) => {
                                    capture = Some(handle);
                                    shared = Some(state);
                                    capture_error = None;
                                }
                                Err(err) => capture_error = Some(err.to_string()),
                            }
                        }
                        KeyCode::Char('d') => {
                            demo = !demo;
                            if demo {
                                drop(capture.take());
                                shared = None;
                            } else {
                                match audio::Capture::start(target.clone()) {
                                    Ok((handle, state)) => {
                                        capture = Some(handle);
                                        shared = Some(state);
                                        capture_error = None;
                                    }
                                    Err(err) => capture_error = Some(err.to_string()),
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_scene_renders_on_small_and_large_terminals() {
        let state = sensitivity(&demo_audio(4.2), 1.0);
        for (w, h) in [(1, 1), (80, 24), (160, 50)] {
            for scene in 0..visuals::SCENES.len() {
                let area = ratatui::layout::Rect::new(0, 0, w, h);
                let mut buffer = ratatui::buffer::Buffer::empty(area);
                visuals::draw(
                    &mut buffer,
                    area,
                    4.2,
                    scene,
                    (scene + 1) % visuals::SCENES.len(),
                    0.4,
                    0,
                    &[0.4; 32],
                    &state,
                );
                assert!(buffer.content.iter().any(|cell| cell.symbol() == "▀"));
            }
        }
    }
    #[test]
    fn gain_is_bounded_and_does_not_change_tempo() {
        let state = demo_audio(0.0);
        let features = sensitivity(&state, 3.0);
        assert_eq!(features.kick, 1.0);
        assert!(features.energy <= 1.0);
        assert_eq!(state.bpm, Some(120.0));
    }
    #[test]
    fn menu_navigation_wraps_at_both_ends() {
        let mut menu = Menu {
            kind: MenuKind::Scenes,
            selected: 0,
        };
        menu.step(-1);
        assert_eq!(menu.selected, visuals::SCENES.len() - 1);
        menu.step(1);
        assert_eq!(menu.selected, 0);
    }
    #[test]
    fn stale_audio_is_not_used_for_visuals_or_tempo() {
        let shared = Some(std::sync::Arc::new(std::sync::Mutex::new(
            audio::AudioState {
                energy: 0.8,
                bpm: Some(120.0),
                connected: true,
                updated: Some(Instant::now() - Duration::from_secs(2)),
                ..Default::default()
            },
        )));
        let state = snapshot(&shared);
        assert_eq!(state.energy, 0.0);
        assert!(state.bpm.is_none());
        assert!(state.error.is_some());
    }
    #[test]
    fn invalid_sensitivity_is_rejected() {
        use clap::Parser;
        for value in ["NaN", "-2", "0", "4"] {
            assert!(Args::try_parse_from(["chromavibe", "--sensitivity", value]).is_err());
        }
    }
}
