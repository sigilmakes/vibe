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
        let anchors = match mode % PALETTES.len() {
            0 => [self.ansi[4], self.ansi[2], self.ansi[3]],
            1 => [self.ansi[2], self.ansi[2], self.ansi[3]],
            _ => [self.ansi[1], self.ansi[1], self.ansi[3]],
        };
        // Broad pigment regions, with soft seams rather than a rotating rainbow.
        let pigment = if phase < 0.40 {
            anchors[0]
        } else if phase < 0.55 {
            mix(anchors[0], anchors[1], (phase - 0.40) / 0.15)
        } else if phase < 0.80 {
            anchors[1]
        } else {
            mix(anchors[1], anchors[2], (phase - 0.80) / 0.20)
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

/// Query the controlling terminal before starting the event loop. Unsupported
/// terminals and noninteractive runs use the canonical ANSI fallback.
pub fn detect() -> Theme {
    #[cfg(unix)]
    if let Some(theme) = query_terminal() {
        return theme;
    }
    Theme::default()
}

#[cfg(unix)]
fn query_terminal() -> Option<Theme> {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open("/dev/tty")
        .ok()?;
    query_tty(tty)
}

#[cfg(unix)]
fn query_tty(mut tty: std::fs::File) -> Option<Theme> {
    use std::{
        io::{Read, Write},
        os::fd::AsRawFd,
        time::{Duration, Instant},
    };

    struct Restore {
        fd: libc::c_int,
        original: libc::termios,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            // Do not flush: input arriving after the query belongs to the user.
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
            }
        }
    }

    let fd = tty.as_raw_fd();
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // Leave already queued keystrokes untouched; startup detection is optional.
    if unsafe { libc::poll(&mut descriptor, 1, 0) } != 0 {
        return None;
    }
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return None;
    }
    let original = unsafe { original.assume_init() };
    let mut mode = original;
    mode.c_lflag &= !(libc::ICANON | libc::ECHO);
    mode.c_cc[libc::VMIN] = 0;
    mode.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &mode) } != 0 {
        return None;
    }
    let _restore = Restore { fd, original };
    let mut queries = String::new();
    for index in 0..16 {
        queries.push_str(&format!("\x1b]4;{index};?\x1b\\"));
    }
    queries.push_str("\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
    let deadline = Instant::now() + Duration::from_millis(200);
    tty.write_all(queries.as_bytes()).ok()?;
    let mut theme = Theme::default();
    let mut replies = Replies::default();
    let mut buffer = [0; 2048];
    while replies.seen != (1 << 18) - 1 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        descriptor.revents = 0;
        let ready = unsafe { libc::poll(&mut descriptor, 1, remaining.as_millis().max(1) as i32) };
        if ready <= 0 || descriptor.revents & libc::POLLIN == 0 {
            break;
        }
        match tty.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => replies.feed(&buffer[..size], &mut theme),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }
    Some(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unanswered_query_is_bounded_and_restores_terminal() {
        use std::{
            fs::File,
            os::fd::{AsRawFd, FromRawFd},
            time::{Duration, Instant},
        };
        let mut master = -1;
        let mut slave = -1;
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
        let _master = unsafe { File::from_raw_fd(master) };
        let tty = unsafe { File::from_raw_fd(slave) };
        let observer = tty.try_clone().unwrap();
        unsafe {
            libc::fcntl(slave, libc::F_SETFL, libc::O_NONBLOCK);
        }
        let mut before = std::mem::MaybeUninit::<libc::termios>::uninit();
        assert_eq!(unsafe { libc::tcgetattr(slave, before.as_mut_ptr()) }, 0);
        let before = unsafe { before.assume_init() };
        let start = Instant::now();
        assert_eq!(query_tty(tty), Some(Theme::default()));
        assert!(start.elapsed() < Duration::from_secs(1));
        let mut after = std::mem::MaybeUninit::<libc::termios>::uninit();
        assert_eq!(
            unsafe { libc::tcgetattr(observer.as_raw_fd(), after.as_mut_ptr()) },
            0
        );
        let after = unsafe { after.assume_init() };
        assert_eq!(before.c_lflag, after.c_lflag);
        assert_eq!(before.c_cc, after.c_cc);
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
