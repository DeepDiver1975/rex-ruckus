//! On/off edges of the player's powers (jetpack, night vision) as cues. Core's inventory only
//! reports running dry, so the game compares the flags tick by tick instead.

use super::Cue;

/// Remembers last tick's jetpack and night-vision flags. Starts with both off.
#[derive(Debug, Clone, Default)]
pub struct PowerWatch {
    jetpack: bool,
    nv: bool,
}

impl PowerWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds this tick's flags and pushes a cue for every change: `JetpackStart`/`JetpackStop`
    /// and `NightVisionOn`/`NightVisionOff`. Switched off by hand or out of power sound alike.
    pub fn tick(&mut self, jetpack_on: bool, nv_on: bool, out: &mut Vec<Cue>) {
        if std::mem::replace(&mut self.jetpack, jetpack_on) != jetpack_on {
            out.push(if jetpack_on {
                Cue::JetpackStart
            } else {
                Cue::JetpackStop
            });
        }
        if std::mem::replace(&mut self.nv, nv_on) != nv_on {
            out.push(if nv_on {
                Cue::NightVisionOn
            } else {
                Cue::NightVisionOff
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(w: &mut PowerWatch, jet: bool, nv: bool) -> Vec<Cue> {
        let mut out = Vec::new();
        w.tick(jet, nv, &mut out);
        out
    }

    #[test]
    fn steady_flags_give_nothing() {
        let mut w = PowerWatch::new();
        assert!(tick(&mut w, false, false).is_empty());
        assert!(tick(&mut w, false, false).is_empty());
    }

    #[test]
    fn jetpack_edges() {
        let mut w = PowerWatch::new();
        assert_eq!(tick(&mut w, true, false), [Cue::JetpackStart]);
        assert!(tick(&mut w, true, false).is_empty());
        assert_eq!(tick(&mut w, false, false), [Cue::JetpackStop]);
        assert!(tick(&mut w, false, false).is_empty());
    }

    #[test]
    fn night_vision_edges() {
        let mut w = PowerWatch::new();
        assert_eq!(tick(&mut w, false, true), [Cue::NightVisionOn]);
        assert!(tick(&mut w, false, true).is_empty());
        assert_eq!(tick(&mut w, false, false), [Cue::NightVisionOff]);
    }

    #[test]
    fn both_at_once() {
        let mut w = PowerWatch::new();
        assert_eq!(
            tick(&mut w, true, true),
            [Cue::JetpackStart, Cue::NightVisionOn]
        );
        assert_eq!(
            tick(&mut w, false, false),
            [Cue::JetpackStop, Cue::NightVisionOff]
        );
    }
}
