use ratatui::style::Color;

pub const PALETTES: &[&str] = &["Terminal", "Phosphor", "Ember"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub ansi: [[u8; 3]; 16],
    pub background: [u8; 3],
    pub foreground: [u8; 3],
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            ansi: [
                [0, 0, 0],
                [128, 0, 0],
                [0, 128, 0],
                [128, 128, 0],
                [0, 0, 128],
                [128, 0, 128],
                [0, 128, 128],
                [192, 192, 192],
                [128, 128, 128],
                [255, 0, 0],
                [0, 255, 0],
                [255, 255, 0],
                [0, 0, 255],
                [255, 0, 255],
                [0, 255, 255],
                [255, 255, 255],
            ],
            background: [0, 0, 0],
            foreground: [192, 192, 192],
        }
    }
}

fn mix(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * amount).round() as u8)
}

impl Theme {
    pub fn sample(&self, mode: usize, phase: f32, intensity: f32) -> Color {
        let [r, g, b] = self.rgb(mode, phase, intensity);
        Color::Rgb(r, g, b)
    }

    pub fn rgb(&self, mode: usize, phase: f32, intensity: f32) -> [u8; 3] {
        let intensity = if intensity.is_finite() {
            intensity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if intensity == 0.0 {
            return self.background;
        }
        let phase = if phase.is_finite() {
            phase.rem_euclid(1.0)
        } else {
            0.0
        };
        let pigment = if mode.is_multiple_of(PALETTES.len()) {
            // Follow the terminal's chromatic roles, including their bright
            // accents. Neighboring roles blend rather than forming hard bands.
            let roles = [1, 9, 3, 11, 2, 10, 6, 14, 4, 12, 5, 13];
            let position = phase * roles.len() as f32;
            let index = position.floor() as usize % roles.len();
            let fraction = position.fract();
            let blend = fraction * fraction * (3.0 - 2.0 * fraction);
            mix(
                self.ansi[roles[index]],
                self.ansi[roles[(index + 1) % roles.len()]],
                blend,
            )
        } else {
            let anchors = if mode % PALETTES.len() == 1 {
                [self.ansi[2], self.ansi[2], self.ansi[3]]
            } else {
                [self.ansi[1], self.ansi[1], self.ansi[3]]
            };
            if phase < 0.40 {
                anchors[0]
            } else if phase < 0.55 {
                mix(anchors[0], anchors[1], (phase - 0.40) / 0.15)
            } else if phase < 0.80 {
                anchors[1]
            } else {
                mix(anchors[1], anchors[2], (phase - 0.80) / 0.20)
            }
        };
        let shaded = mix(self.background, pigment, intensity.powf(0.72));
        // Foreground is a glint, never the main pigment or a permanent white core.
        let highlight = ((intensity - 0.94) / 0.06).clamp(0.0, 1.0) * 0.32;
        mix(shaded, self.foreground, highlight)
    }
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        return Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ]);
    }
    let mut parts = value.strip_prefix("rgb:")?.split('/');
    let mut rgb = [0; 3];
    for channel in &mut rgb {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 4 {
            return None;
        }
        let component = u32::from_str_radix(part, 16).ok()?;
        let maximum = (1_u32 << (part.len() * 4)) - 1;
        *channel = ((component * 255 + maximum / 2) / maximum) as u8;
    }
    parts.next().is_none().then_some(rgb)
}

#[derive(Default)]
struct Replies {
    bytes: Vec<u8>,
    seen: u32,
}

impl Replies {
    fn feed(&mut self, input: &[u8], theme: &mut Theme) {
        self.bytes.extend_from_slice(input);
        while let Some(start) = self.bytes.windows(2).position(|w| w == b"\x1b]") {
            let body = start + 2;
            let end = (body..self.bytes.len())
                .find(|&i| self.bytes[i] == 7 || self.bytes.get(i..i + 2) == Some(b"\x1b\\"));
            let Some(end) = end else { break };
            if let Ok(reply) = std::str::from_utf8(&self.bytes[body..end]) {
                let fields: Vec<_> = reply.split(';').collect();
                match fields.as_slice() {
                    ["4", index, color] => {
                        if let (Ok(index), Some(color)) =
                            (index.parse::<usize>(), parse_color(color))
                        {
                            if index < 16 {
                                theme.ansi[index] = color;
                                self.seen |= 1 << index;
                            }
                        }
                    }
                    ["10", color] => {
                        if let Some(color) = parse_color(color) {
                            theme.foreground = color;
                            self.seen |= 1 << 16;
                        }
                    }
                    ["11", color] => {
                        if let Some(color) = parse_color(color) {
                            theme.background = color;
                            self.seen |= 1 << 17;
                        }
                    }
                    _ => {}
                }
            }
            let terminator = if self.bytes[end] == 7 { 1 } else { 2 };
            self.bytes.drain(..end + terminator);
        }
        if self.bytes.len() > 8192 {
            self.bytes.clear();
        }
    }
}

// One owner reads the terminal: crossterm's event reader must not run alongside
// this stream, because both keyboard input and OSC replies arrive on the tty.
const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const ESCAPE_DELAY: std::time::Duration = std::time::Duration::from_millis(30);
const MAX_SEQUENCE: usize = 4096;

#[derive(Default)]
struct Input {
    bytes: Vec<u8>,
    escape_since: Option<std::time::Instant>,
    replies: Replies,
}

impl Input {
    fn next(
        &mut self,
        theme: &mut Theme,
        now: std::time::Instant,
    ) -> Option<crossterm::event::Event> {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        loop {
            let &first = self.bytes.first()?;
            let mut modifiers = KeyModifiers::NONE;
            let (used, code) = if first == 0x1b {
                let since = *self.escape_since.get_or_insert(now);
                if self.bytes.get(1) == Some(&b']') {
                    let end = (2..self.bytes.len()).find(|&i| {
                        self.bytes[i] == 7 || self.bytes.get(i..i + 2) == Some(b"\x1b\\")
                    });
                    if let Some(end) = end {
                        let used = end + if self.bytes[end] == 7 { 1 } else { 2 };
                        self.replies.feed(&self.bytes[..used], theme);
                        self.bytes.drain(..used);
                        self.escape_since = None;
                        continue;
                    }
                    // OSC packets can be fragmented longer than the Escape-key
                    // delay. Bound malformed packets without leaking reply text
                    // into shortcuts (notably the `q` in a query).
                    if self.bytes.len() > MAX_SEQUENCE {
                        self.bytes.clear();
                        self.escape_since = None;
                    }
                    return None;
                }
                if matches!(self.bytes.get(1), Some(b'[' | b'O')) {
                    let end =
                        (2..self.bytes.len()).find(|&i| (0x40..=0x7e).contains(&self.bytes[i]));
                    if let Some(end) = end {
                        let code = match self.bytes[end] {
                            b'A' => Some(KeyCode::Up),
                            b'B' => Some(KeyCode::Down),
                            b'C' => Some(KeyCode::Right),
                            b'D' => Some(KeyCode::Left),
                            b'H' => Some(KeyCode::Home),
                            b'F' => Some(KeyCode::End),
                            _ => None,
                        };
                        let used = end + 1;
                        if let Some(code) = code {
                            (used, code)
                        } else {
                            self.bytes.drain(..used);
                            self.escape_since = None;
                            continue;
                        }
                    } else if now.duration_since(since) < ESCAPE_DELAY {
                        return None;
                    } else {
                        (1, KeyCode::Esc)
                    }
                } else if self.bytes.len() == 1 && now.duration_since(since) < ESCAPE_DELAY {
                    return None;
                } else {
                    (1, KeyCode::Esc)
                }
            } else {
                match first {
                    b'\r' | b'\n' => (1, KeyCode::Enter),
                    b'\t' => (1, KeyCode::Tab),
                    0x7f | 8 => (1, KeyCode::Backspace),
                    1..=26 => {
                        modifiers = KeyModifiers::CONTROL;
                        (1, KeyCode::Char((b'a' + first - 1) as char))
                    }
                    0..=127 => (1, KeyCode::Char(first as char)),
                    _ => {
                        let width = match first {
                            0xc2..=0xdf => 2,
                            0xe0..=0xef => 3,
                            0xf0..=0xf4 => 4,
                            _ => 1,
                        };
                        if self.bytes.len() < width {
                            return None;
                        }
                        match std::str::from_utf8(&self.bytes[..width])
                            .ok()
                            .and_then(|s| s.chars().next())
                        {
                            Some(ch) => (width, KeyCode::Char(ch)),
                            None => {
                                self.bytes.remove(0);
                                continue;
                            }
                        }
                    }
                }
            };
            self.bytes.drain(..used);
            self.escape_since = None;
            return Some(Event::Key(KeyEvent::new(code, modifiers)));
        }
    }
}

pub struct TerminalInput {
    tty: std::fs::File,
    input: Input,
    pub theme: Theme,
    next_refresh: std::time::Instant,
}

impl TerminalInput {
    /// Call after entering raw mode. This is the only terminal reader for the
    /// lifetime of the UI; unsupported OSC queries simply leave colors intact.
    pub fn open() -> std::io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
            .open("/dev/tty")?;
        Ok(Self::new(tty))
    }

    fn new(tty: std::fs::File) -> Self {
        Self {
            tty,
            input: Input::default(),
            theme: Theme::default(),
            next_refresh: std::time::Instant::now(),
        }
    }

    fn refresh(&mut self, now: std::time::Instant) -> std::io::Result<()> {
        use std::io::Write;
        if now >= self.next_refresh {
            let mut queries = String::new();
            for index in 0..16 {
                queries.push_str(&format!("\x1b]4;{index};?\x1b\\"));
            }
            queries.push_str("\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
            // A nonblocking write cannot stall rendering. A full output queue
            // can skip this query; the next refresh retries the whole palette.
            if let Err(error) = self.tty.write_all(queries.as_bytes()) {
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(error);
                }
            }
            self.next_refresh = now + REFRESH_INTERVAL;
        }
        Ok(())
    }

    pub fn read(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::io::Result<Option<crossterm::event::Event>> {
        use std::{io::Read, os::fd::AsRawFd, time::Instant};
        let deadline = Instant::now() + timeout;
        let mut polled = false;
        loop {
            let now = Instant::now();
            self.refresh(now)?;
            if let Some(event) = self.input.next(&mut self.theme, now) {
                return Ok(Some(event));
            }
            if polled && now >= deadline {
                return Ok(None);
            }
            let mut wait = deadline.saturating_duration_since(now);
            if let Some(since) = self.input.escape_since {
                if !self.input.bytes.starts_with(b"\x1b]") {
                    wait = wait.min(ESCAPE_DELAY.saturating_sub(now.duration_since(since)));
                }
            }
            let mut descriptor = libc::pollfd {
                fd: self.tty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let millis = if wait.is_zero() {
                0
            } else {
                wait.as_millis().max(1) as i32
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, millis) };
            polled = true;
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "terminal closed",
                ));
            }
            if ready > 0 {
                let mut buffer = [0; 2048];
                match self.tty.read(&mut buffer) {
                    Ok(0) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "terminal closed",
                        ))
                    }
                    Ok(size) => self.input.bytes.extend_from_slice(&buffer[..size]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_negative_phase_wraps_to_first_terminal_role() {
        let theme = Theme::default();
        // rem_euclid can round a tiny negative phase to exactly 1.0.
        assert_eq!(theme.rgb(0, -1e-9, 0.8), theme.rgb(0, 0.0, 0.8));
    }

    fn pty() -> (std::fs::File, TerminalInput) {
        use std::os::fd::FromRawFd;
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
        unsafe {
            assert_eq!(libc::tcgetattr(slave, mode.as_mut_ptr()), 0);
            let mut mode = mode.assume_init();
            libc::cfmakeraw(&mut mode);
            assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &mode), 0);
            assert_ne!(libc::fcntl(slave, libc::F_SETFL, libc::O_NONBLOCK), -1);
            assert_ne!(libc::fcntl(master, libc::F_SETFL, libc::O_NONBLOCK), -1);
            (
                std::fs::File::from_raw_fd(master),
                TerminalInput::new(std::fs::File::from_raw_fd(slave)),
            )
        }
    }

    fn key(event: Option<crossterm::event::Event>) -> crossterm::event::KeyEvent {
        match event.unwrap() {
            crossterm::event::Event::Key(key) => key,
            other => panic!("expected key, got {other:?}"),
        }
    }

    #[test]
    fn live_refresh_changes_rgb_and_preserves_mixed_keyboard_input() {
        use crossterm::event::{KeyCode, KeyModifiers};
        use std::{
            io::{Read, Write},
            time::{Duration, Instant},
        };
        let (mut terminal, mut input) = pty();
        let wait = Duration::from_millis(5);
        assert!(input.read(wait).unwrap().is_none());
        let mut queries = [0; 1024];
        let size = terminal.read(&mut queries).unwrap();
        assert!(queries[..size].windows(7).any(|s| s == b"\x1b]4;2;?"));
        terminal
            .write_all(b"m\x1b]4;2;#112233\x07\x1b[C+-?\x03\r")
            .unwrap();
        let expected = [
            KeyCode::Char('m'),
            KeyCode::Right,
            KeyCode::Char('+'),
            KeyCode::Char('-'),
            KeyCode::Char('?'),
            KeyCode::Char('c'),
            KeyCode::Enter,
        ];
        for code in expected {
            let event = key(input.read(wait).unwrap());
            assert_eq!(event.code, code);
            if code == KeyCode::Char('c') {
                assert_eq!(event.modifiers, KeyModifiers::CONTROL);
            }
        }
        let before = input.theme.rgb(1, 0.2, 0.8);
        let old_background = input.theme.background;
        // Advance only the refresh schedule: the second request is made by the
        // same running reader, with its previous palette and input intact.
        input.next_refresh = Instant::now();
        assert!(input.read(wait).unwrap().is_none());
        assert!(terminal.read(&mut queries).unwrap() > 0);
        terminal
            .write_all(b"\x1b]4;2;rgb:eeee/aaaa/5555\x1b")
            .unwrap();
        assert!(input.read(wait).unwrap().is_none());
        assert_eq!(input.theme.rgb(1, 0.2, 0.8), before);
        terminal.write_all(b"\\\x1b[").unwrap();
        assert!(input.read(wait).unwrap().is_none());
        terminal.write_all(b"Dq").unwrap();
        assert_eq!(key(input.read(wait).unwrap()).code, KeyCode::Left);
        assert_eq!(key(input.read(wait).unwrap()).code, KeyCode::Char('q'));
        assert_ne!(input.theme.rgb(1, 0.2, 0.8), before);
        assert_eq!(input.theme.ansi[2], [238, 170, 85]);
        assert_eq!(input.theme.background, old_background);
        let updated = input.theme.clone();
        input.next_refresh = Instant::now();
        let start = Instant::now();
        assert!(input.read(wait).unwrap().is_none());
        assert!(start.elapsed() < Duration::from_millis(100));
        assert_eq!(
            input.theme, updated,
            "unanswered refresh retains the last theme"
        );
    }

    #[test]
    fn standalone_escape_and_fragmented_arrow_are_distinct() {
        use crossterm::event::KeyCode;
        use std::time::Instant;
        let mut input = Input::default();
        let mut theme = Theme::default();
        let now = Instant::now();
        input.bytes.extend_from_slice(b"\x1b");
        assert!(input.next(&mut theme, now).is_none());
        assert_eq!(
            key(input.next(&mut theme, now + ESCAPE_DELAY)).code,
            KeyCode::Esc
        );
        input.bytes.extend_from_slice(b"\x1b[");
        assert!(input.next(&mut theme, now).is_none());
        input.bytes.extend_from_slice(b"A\x1bOB");
        assert_eq!(key(input.next(&mut theme, now)).code, KeyCode::Up);
        assert_eq!(key(input.next(&mut theme, now)).code, KeyCode::Down);
    }

    #[test]
    fn parses_terminal_color_formats() {
        assert_eq!(parse_color("rgb:ffff/8080/0000"), Some([255, 128, 0]));
        assert_eq!(parse_color("rgb:f/8/0"), Some([255, 136, 0]));
        assert_eq!(parse_color("#a0B1c2"), Some([160, 177, 194]));
        for bad in [
            "rgb:ffff/0000",
            "rgb:fffff/0/0",
            "rgb:1/2/3/4",
            "#xyz123",
            "#123",
            "rgb:/1/2",
        ] {
            assert_eq!(parse_color(bad), None);
        }
    }

    #[test]
    fn replies_are_incremental_and_preserve_unreported_colors() {
        let mut theme = Theme::default();
        let original = theme.clone();
        let mut replies = Replies::default();
        replies.feed(b"\x1b]4;2;rgb:1234/5678/9abc\x1b", &mut theme);
        assert_eq!(theme, original);
        replies.feed(b"\\\x1b]11;#102030\x07\x1b]10;#aabbcc\x07", &mut theme);
        assert_eq!(theme.ansi[2], [18, 86, 154]);
        assert_eq!(theme.ansi[3], original.ansi[3]);
        assert_eq!(theme.background, [16, 32, 48]);
        assert_eq!(theme.foreground, [170, 187, 204]);
    }

    #[test]
    fn terminal_palette_uses_all_chromatic_roles_and_bright_accents() {
        let theme = Theme::default();
        let roles = [1, 9, 3, 11, 2, 10, 6, 14, 4, 12, 5, 13];
        for (step, role) in roles.iter().enumerate() {
            let color = theme.rgb(0, step as f32 / roles.len() as f32, 0.8);
            assert_eq!(
                color,
                mix(theme.background, theme.ansi[*role], 0.8_f32.powf(0.72))
            );
        }
        assert_eq!(theme.rgb(0, 0.0, 0.8), theme.rgb(0, 1.0, 0.8));
    }

    #[test]
    fn shades_preserve_background_and_stay_within_theme_bounds() {
        let theme = Theme {
            ansi: [[70, 90, 110]; 16],
            background: [20, 30, 40],
            foreground: [150, 160, 170],
        };
        for mode in 0..PALETTES.len() {
            assert_eq!(theme.rgb(mode, 0.4, 0.0), theme.background);
            assert_eq!(theme.rgb(mode, 0.4, f32::NAN), theme.background);
            for phase in 0..100 {
                for intensity in 0..100 {
                    let color = theme.rgb(mode, phase as f32 / 100.0, intensity as f32 / 99.0);
                    for (i, channel) in color.iter().enumerate() {
                        assert!(*channel >= theme.background[i] && *channel <= theme.foreground[i]);
                    }
                }
            }
        }
    }
}
