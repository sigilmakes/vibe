use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub artist: String,
    pub title: String,
}

impl NowPlaying {
    pub fn display(&self) -> String {
        match (self.artist.is_empty(), self.title.is_empty()) {
            (false, false) => format!("{} - {}", self.artist, self.title),
            (true, false) => self.title.clone(),
            (false, true) => self.artist.clone(),
            (true, true) => String::new(),
        }
    }
}

/// Read the active MPRIS player, if `playerctl` is installed and something is
/// playing or paused. Missing tools and stopped players are not errors.
pub fn read() -> Option<NowPlaying> {
    let output = Command::new("playerctl")
        .args(["metadata", "--format", "{{status}}\t{{artist}}\t{{title}}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

fn parse(text: &str) -> Option<NowPlaying> {
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let mut fields = line.split('\t');
    let status = fields.next()?.trim();
    if status != "Playing" && status != "Paused" {
        return None;
    }
    let artist = fields.next().unwrap_or("").trim().to_owned();
    let title = fields.next().unwrap_or("").trim().to_owned();
    if artist.is_empty() && title.is_empty() {
        None
    } else {
        Some(NowPlaying { artist, title })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_playing_and_paused_metadata_only() {
        assert_eq!(
            parse("Playing\tAphex Twin\tXtal\n"),
            Some(NowPlaying {
                artist: "Aphex Twin".into(),
                title: "Xtal".into()
            })
        );
        assert_eq!(
            parse("Paused\t\tSelected Ambient Works\n"),
            Some(NowPlaying {
                artist: String::new(),
                title: "Selected Ambient Works".into()
            })
        );
        assert_eq!(parse("Stopped\tAphex Twin\tXtal\n"), None);
        assert_eq!(parse("Playing\t\t\n"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn display_avoids_dangling_separators() {
        let both = NowPlaying {
            artist: "A".into(),
            title: "B".into(),
        };
        assert_eq!(both.display(), "A - B");
        assert_eq!(
            NowPlaying {
                artist: String::new(),
                title: "B".into()
            }
            .display(),
            "B"
        );
    }
}
