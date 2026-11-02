/// Auto-advance policy. `Flow` is deliberately independent of the menu and
/// terminal; the app supplies candidates and a small audio snapshot each tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Phrases,
    Timer,
}

impl Mode {
    pub fn from_config(value: &str) -> Self {
        match value {
            "phrases" => Self::Phrases,
            "timer" => Self::Timer,
            _ => Self::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Favourites,
    Playlist,
}

impl Scope {
    pub fn from_config(value: &str) -> Self {
        match value {
            "favourites" => Self::Favourites,
            "playlist" => Self::Playlist,
            _ => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub transition_secs: f32,
    pub scope: Scope,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            transition_secs: 20.0,
            scope: Scope::All,
        }
    }
}

/// The subset of audio state that matters to auto-advance.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Signal {
    pub energy: f32,
    pub novelty: f32,
    pub onset: f32,
    pub connected: bool,
    pub silent_for: f32,
}

pub struct Flow {
    settings: Settings,
    last_scene: Option<usize>,
    last_switch: f32,
    last_energy: f32,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            last_scene: None,
            last_switch: 0.0,
            last_energy: 0.0,
        }
    }
}

impl Flow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pause the dwell timer while a menu or overlay is open. The next
    /// interval starts when the overlay closes.
    pub fn hold(&mut self, now: f32, signal: Signal) {
        self.last_switch = now;
        self.last_energy = signal.energy;
    }

    /// Select the next candidate, or `None` until the policy fires.
    ///
    /// `now` is monotonic seconds since session start. Manual scene changes
    /// and settings changes reset the dwell timer, so the new choice gets a
    /// full interval before it can be replaced again.
    pub fn tick(
        &mut self,
        settings: Settings,
        now: f32,
        current: usize,
        candidates: &[usize],
        signal: Signal,
    ) -> Option<usize> {
        if settings != self.settings || self.last_scene != Some(current) {
            self.settings = settings;
            self.last_scene = Some(current);
            self.last_switch = now;
            self.last_energy = signal.energy;
            return None;
        }
        let transition = settings.transition_secs.clamp(5.0, 300.0);
        let elapsed = (now - self.last_switch).max(0.0);
        let next = next_candidate(current, candidates);
        let advance = match settings.mode {
            Mode::Off => false,
            Mode::Timer => elapsed >= transition,
            Mode::Phrases => {
                if !signal.connected || signal.silent_for >= 1.5 {
                    // With no usable signal, a phrase timer is just a timer.
                    elapsed >= transition
                } else {
                    let min_dwell = (transition * 0.5).clamp(4.0, 20.0);
                    let fallback = (transition * 2.0).max(15.0);
                    let rise = (signal.energy - self.last_energy).max(0.0);
                    let phrase = signal.novelty >= 0.55
                        || rise >= 0.12
                        || (signal.onset >= 0.6 && rise >= 0.06);
                    elapsed >= fallback || (elapsed >= min_dwell && phrase)
                }
            }
        };
        self.last_energy = signal.energy;
        if advance {
            if let Some(next) = next {
                self.last_switch = now;
                self.last_energy = signal.energy;
                self.last_scene = Some(next);
            }
            return next;
        }
        None
    }
}

fn next_candidate(current: usize, candidates: &[usize]) -> Option<usize> {
    if candidates.len() < 2 {
        return None;
    }
    let position = candidates.iter().position(|&scene| scene == current);
    let index = match position {
        Some(position) => (position + 1) % candidates.len(),
        None => 0,
    };
    Some(candidates[index])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMER: Settings = Settings {
        mode: Mode::Timer,
        transition_secs: 20.0,
        scope: Scope::All,
    };

    fn signal(energy: f32, novelty: f32) -> Signal {
        Signal {
            energy,
            novelty,
            onset: 0.0,
            connected: true,
            silent_for: 0.0,
        }
    }

    #[test]
    fn timer_waits_for_the_interval_and_wraps() {
        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 0, &[0, 1, 2], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(TIMER, 19.9, 0, &[0, 1, 2], signal(0.2, 0.0)),
            None
        );
        assert_eq!(
            flow.tick(TIMER, 20.0, 0, &[0, 1, 2], signal(0.2, 0.0)),
            Some(1)
        );
        assert_eq!(
            flow.tick(TIMER, 39.9, 1, &[0, 1, 2], signal(0.2, 0.0)),
            None
        );
        assert_eq!(
            flow.tick(TIMER, 40.0, 1, &[0, 1, 2], signal(0.2, 0.0)),
            Some(2)
        );
    }

    #[test]
    fn manual_scene_change_restarts_the_interval() {
        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(flow.tick(TIMER, 15.0, 1, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(flow.tick(TIMER, 20.0, 1, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(TIMER, 35.0, 1, &[0, 1], signal(0.2, 0.0)),
            Some(0)
        );
    }

    #[test]
    fn phrases_need_a_dwell_and_a_strong_change() {
        let settings = Settings {
            mode: Mode::Phrases,
            transition_secs: 20.0,
            scope: Scope::All,
        };
        let mut flow = Flow::new();
        assert_eq!(flow.tick(settings, 0.0, 0, &[0, 1], signal(0.2, 0.9)), None);
        // A novelty spike inside the dwell is ignored.
        assert_eq!(flow.tick(settings, 3.0, 0, &[0, 1], signal(0.2, 0.9)), None);
        // After the minimum dwell, a strong change advances.
        assert_eq!(
            flow.tick(settings, 10.0, 0, &[0, 1], signal(0.7, 0.8)),
            Some(1)
        );
        // The timer resets again.
        assert_eq!(
            flow.tick(settings, 19.9, 1, &[0, 1], signal(0.7, 0.8)),
            None
        );
    }

    #[test]
    fn phrases_fall_back_to_a_timer_without_signal_or_after_too_long() {
        let settings = Settings {
            mode: Mode::Phrases,
            transition_secs: 20.0,
            scope: Scope::All,
        };
        let mut flow = Flow::new();
        assert_eq!(flow.tick(settings, 0.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        let quiet = Signal {
            connected: false,
            ..signal(0.0, 0.0)
        };
        assert_eq!(flow.tick(settings, 19.9, 0, &[0, 1], quiet), None);
        assert_eq!(flow.tick(settings, 20.0, 0, &[0, 1], quiet), Some(1));

        let mut flow = Flow::new();
        assert_eq!(flow.tick(settings, 0.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(settings, 39.9, 0, &[0, 1], signal(0.2, 0.0)),
            None
        );
        assert_eq!(
            flow.tick(settings, 40.0, 0, &[0, 1], signal(0.2, 0.0)),
            Some(1)
        );
    }

    #[test]
    fn off_and_single_candidate_scopes_do_nothing() {
        let mut flow = Flow::new();
        let off = Settings {
            mode: Mode::Off,
            ..TIMER
        };
        assert_eq!(flow.tick(off, 0.0, 0, &[0, 1], signal(0.2, 0.9)), None);
        assert_eq!(flow.tick(off, 100.0, 0, &[0, 1], signal(0.2, 0.9)), None);

        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 0, &[0], signal(0.2, 0.0)), None);
        assert_eq!(flow.tick(TIMER, 100.0, 0, &[0], signal(0.2, 0.0)), None);
    }

    #[test]
    fn settings_change_and_missing_current_choose_the_first_candidate() {
        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 5, &[2, 3], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(TIMER, 20.0, 5, &[2, 3], signal(0.2, 0.0)),
            Some(2)
        );

        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        let faster = Settings {
            transition_secs: 10.0,
            ..TIMER
        };
        assert_eq!(flow.tick(faster, 10.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(faster, 20.0, 0, &[0, 1], signal(0.2, 0.0)),
            Some(1)
        );
    }

    #[test]
    fn hold_restarts_the_dwell_without_changing_the_scene() {
        let mut flow = Flow::new();
        assert_eq!(flow.tick(TIMER, 0.0, 0, &[0, 1], signal(0.2, 0.0)), None);
        flow.hold(15.0, signal(0.2, 0.0));
        assert_eq!(flow.tick(TIMER, 34.9, 0, &[0, 1], signal(0.2, 0.0)), None);
        assert_eq!(
            flow.tick(TIMER, 35.0, 0, &[0, 1], signal(0.2, 0.0)),
            Some(1)
        );
    }
}
