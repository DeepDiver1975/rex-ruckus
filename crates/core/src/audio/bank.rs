//! The sound bank as authored in `assets/sounds/bank.ron`: one [`SoundDef`] per [`Cue`], plus
//! the distance gain the game applies on top of each def's volume. Pure, so the level
//! validator can check the bank without Bevy.

use super::Cue;
use serde::Deserialize;
use std::collections::HashMap;

fn one() -> f32 {
    1.0
}
fn ref_dist() -> f32 {
    4.0
}
fn max_dist() -> f32 {
    40.0
}
fn max_voices() -> u32 {
    4
}
fn yes() -> bool {
    true
}

/// How one cue sounds.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundDef {
    /// Variants, relative to the assets root; one is picked per play.
    pub files: Vec<String>,
    /// Linear volume, 1.0 = as recorded.
    #[serde(default = "one")]
    pub volume: f32,
    /// Full volume up to this distance (metres), then falling off as `ref_dist / d`.
    #[serde(default = "ref_dist")]
    pub ref_dist: f32,
    /// Silent beyond this distance (metres).
    #[serde(default = "max_dist")]
    pub max_dist: f32,
    /// At most this many copies of the cue play at once.
    #[serde(default = "max_voices")]
    pub max_voices: u32,
    /// Playback speed varies by up to ± this fraction.
    #[serde(default)]
    pub pitch_jitter: f32,
    /// Positioned in the world (panned). False for sounds centred on the player.
    #[serde(default = "yes")]
    pub spatial: bool,
    /// A loop that runs until the game stops it (mover hum, jetpack burn).
    #[serde(default)]
    pub looped: bool,
}

/// Every cue's [`SoundDef`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct SoundBankDef(pub HashMap<Cue, SoundDef>);

impl SoundBankDef {
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }

    pub fn get(&self, cue: Cue) -> Option<&SoundDef> {
        self.0.get(&cue)
    }

    /// Everything wrong with the bank: cues from [`Cue::all`] without an entry, and entries
    /// with no files or nonsensical numbers. Empty when the bank is usable.
    pub fn problems(&self) -> Vec<String> {
        let mut out: Vec<String> = Cue::all()
            .into_iter()
            .filter(|c| !self.0.contains_key(c))
            .map(|c| format!("{c:?}: no entry"))
            .collect();
        let mut defs: Vec<_> = self.0.iter().collect();
        defs.sort_by_key(|(c, _)| format!("{c:?}"));
        for (c, d) in defs {
            if d.files.is_empty() {
                out.push(format!("{c:?}: no files"));
            }
            if d.volume.is_nan() || d.volume < 0.0 {
                out.push(format!("{c:?}: volume {} is negative", d.volume));
            }
            let dists_ok = d.ref_dist > 0.0 && d.max_dist >= d.ref_dist;
            if !dists_ok {
                out.push(format!(
                    "{c:?}: need 0 < ref_dist ({}) <= max_dist ({})",
                    d.ref_dist, d.max_dist
                ));
            }
            if d.max_voices == 0 {
                out.push(format!("{c:?}: max_voices is 0"));
            }
            if !(0.0..1.0).contains(&d.pitch_jitter) {
                out.push(format!(
                    "{c:?}: pitch_jitter {} not in [0, 1)",
                    d.pitch_jitter
                ));
            }
        }
        out
    }

    /// Every referenced file, each once, sorted.
    pub fn files(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .0
            .values()
            .flat_map(|d| d.files.iter().map(String::as_str))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Distance gain for `def` at `dist` metres: 1 inside `ref_dist`, then `ref_dist / dist`,
/// and 0 beyond `max_dist`.
pub fn gain(def: &SoundDef, dist: f32) -> f32 {
    if dist > def.max_dist {
        return 0.0;
    }
    (def.ref_dist / dist.max(def.ref_dist)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::WeaponId;
    use std::path::Path;

    const ONE: &str = r#"{ Fire(Pistol): (files: ["sounds/synth/pistol.wav"]) }"#;

    fn def() -> SoundDef {
        SoundBankDef::from_ron(ONE).unwrap().0[&Cue::Fire(WeaponId::Pistol)].clone()
    }

    #[test]
    fn parses_with_defaults() {
        let d = def();
        assert_eq!(d.files, ["sounds/synth/pistol.wav"]);
        assert_eq!(d.volume, 1.0);
        assert_eq!(d.ref_dist, 4.0);
        assert_eq!(d.max_dist, 40.0);
        assert_eq!(d.max_voices, 4);
        assert_eq!(d.pitch_jitter, 0.0);
        assert!(d.spatial);
        assert!(!d.looped);
    }

    #[test]
    fn parses_explicit_fields() {
        let bank = SoundBankDef::from_ron(
            r#"{ DoorStart: (files: ["a.wav", "b.wav"], volume: 0.4, ref_dist: 2.0,
                 max_dist: 20.0, max_voices: 2, pitch_jitter: 0.1, spatial: false, looped: true) }"#,
        )
        .unwrap();
        let d = bank.get(Cue::DoorStart).unwrap();
        assert_eq!(d.files.len(), 2);
        assert_eq!((d.volume, d.ref_dist, d.max_dist), (0.4, 2.0, 20.0));
        assert_eq!(d.max_voices, 2);
        assert_eq!(d.pitch_jitter, 0.1);
        assert!(!d.spatial && d.looped);
    }

    #[test]
    fn unknown_field_is_an_error() {
        assert!(SoundBankDef::from_ron(r#"{ Kick: (files: [], loud: true) }"#).is_err());
    }

    #[test]
    fn gain_is_full_inside_ref_dist() {
        let d = def();
        assert_eq!(gain(&d, 0.0), 1.0);
        assert_eq!(gain(&d, 3.9), 1.0);
        assert_eq!(gain(&d, 4.0), 1.0);
    }

    #[test]
    fn gain_halves_at_twice_ref_dist() {
        assert!((gain(&def(), 8.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn gain_is_zero_beyond_max_dist() {
        let d = def();
        assert!(gain(&d, 40.0) > 0.0);
        assert_eq!(gain(&d, 40.1), 0.0);
        assert_eq!(gain(&d, 1e9), 0.0);
    }

    #[test]
    fn problems_lists_missing_cues_and_bad_numbers() {
        let bank = SoundBankDef::from_ron(
            r#"{ Kick: (files: [], ref_dist: 5.0, max_dist: 2.0, max_voices: 0) }"#,
        )
        .unwrap();
        let p = bank.problems();
        assert!(p.iter().any(|s| s == "Reload: no entry"), "{p:?}");
        assert!(p.iter().any(|s| s == "Kick: no files"), "{p:?}");
        assert!(
            p.iter().any(|s| s.starts_with("Kick: need 0 < ref_dist")),
            "{p:?}"
        );
        assert!(p.iter().any(|s| s == "Kick: max_voices is 0"), "{p:?}");
    }

    #[test]
    fn shipped_bank_covers_every_cue_and_its_files_exist() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let src = std::fs::read_to_string(assets.join("sounds/bank.ron")).unwrap();
        let bank = SoundBankDef::from_ron(&src).unwrap();
        assert_eq!(bank.problems(), Vec::<String>::new());
        for f in bank.files() {
            assert!(assets.join(f).is_file(), "missing sound file {f}");
        }
        // Only the loops the game knows how to run are looped.
        for (c, d) in &bank.0 {
            let loop_cue = matches!(c, Cue::DoorStart | Cue::LiftStart | Cue::JetpackLoop);
            assert_eq!(d.looped, loop_cue, "{c:?}");
        }
    }
}
