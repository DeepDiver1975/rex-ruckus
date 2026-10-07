//! sfxr-style procedural sound-effect synthesiser (`rr-tools synth`).
//!
//! A recipe file (RON) lists named sounds; each sound mixes one or more layers. A layer is an
//! oscillator (square, saw, sine, white or brown noise) with a pitch slide, vibrato, an
//! attack/sustain/decay envelope with punch, one-pole low- and high-pass filters and an optional
//! bit crusher. The mix is peak-normalised to -1 dBFS and written as mono 22050 Hz 16-bit WAV.
//!
//! Determinism: noise comes from rr-core's seeded SplitMix64 and all DSP uses plain f64
//! `+ - * /`, `floor` and `round` (sine is a polynomial, not `f64::sin`), which IEEE 754 defines
//! exactly. Rust does not fuse multiply-adds on its own, so the same recipe should give
//! byte-identical files on every platform; CI only checks this on Linux, though.

use rr_core::rng::Rng;
use serde::Deserialize;
use std::io::Cursor;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 22050;
const SR: f64 = SAMPLE_RATE as f64;
/// Longest sound a recipe may describe, in seconds.
const MAX_SECONDS: f64 = 10.0;
/// Fade-out at the end of one-shot sounds, against clicks.
const FADE_SECONDS: f64 = 0.005;
/// Loops render this much extra and cross-fade it into their start, so the wrap is seamless.
const LOOP_XFADE_SECONDS: f64 = 0.05;
/// -1 dBFS as linear amplitude: 10^(-1/20).
const PEAK: f64 = 0.891_250_938_133_745_6;
/// Noise draws a new value 32 times per period of `freq` (as in sfxr), so `freq` sets its colour.
const NOISE_STEPS: f64 = 32.0;
/// Leak of the brown-noise integrator (pole at about 35 Hz) and its make-up gain.
const BROWN_LEAK: f64 = 0.99;
const BROWN_GAIN: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Wave {
    /// Pulse wave; `duty` is the high fraction of each period.
    Square,
    Saw,
    Sine,
    /// White noise, sample-and-held at 32 x `freq` (`freq` 0 = a new value every sample).
    Noise,
    /// Integrated (brown) noise from the same source, for rumble.
    Brown,
}

/// One oscillator voice. Every field but `wave` may be omitted.
#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    pub wave: Wave,
    /// Square duty cycle, in (0, 1).
    #[serde(default = "half")]
    pub duty: f64,
    /// Start pitch, Hz.
    #[serde(default)]
    pub freq: f64,
    /// Pitch change, Hz/s.
    #[serde(default)]
    pub slide: f64,
    /// Change of `slide`, Hz/s².
    #[serde(default)]
    pub delta_slide: f64,
    /// Envelope stages, seconds: linear rise, hold, linear fall.
    #[serde(default)]
    pub attack: f64,
    #[serde(default)]
    pub sustain: f64,
    #[serde(default)]
    pub decay: f64,
    /// Extra level at the start of the sustain, falling linearly to none over the sustain.
    #[serde(default)]
    pub punch: f64,
    /// Vibrato depth in Hz at `vibrato_hz`.
    #[serde(default)]
    pub vibrato_depth: f64,
    #[serde(default)]
    pub vibrato_hz: f64,
    /// One-pole filter cutoffs, Hz; 0 = off.
    #[serde(default)]
    pub lpf: f64,
    #[serde(default)]
    pub hpf: f64,
    /// Quantise to this many bits (after the envelope, before `gain`); 0 = off.
    #[serde(default)]
    pub crush_bits: u32,
    #[serde(default = "one")]
    pub gain: f64,
    /// Start offset within the sound, seconds.
    #[serde(default)]
    pub delay: f64,
}

fn half() -> f64 {
    0.5
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recipe {
    /// Becomes the file name, `<name>.wav`.
    pub name: String,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub seed: u64,
    /// Seamless loop: no fade-out, the tail is cross-faded into the start instead.
    #[serde(default, rename = "loop")]
    pub looped: bool,
}

impl Layer {
    fn length(&self) -> f64 {
        self.attack + self.sustain + self.decay
    }

    fn end(&self) -> f64 {
        self.delay + self.length()
    }

    fn check(&self, looped: bool) -> Result<(), String> {
        let fields = [
            ("duty", self.duty),
            ("freq", self.freq),
            ("slide", self.slide),
            ("delta_slide", self.delta_slide),
            ("attack", self.attack),
            ("sustain", self.sustain),
            ("decay", self.decay),
            ("punch", self.punch),
            ("vibrato_depth", self.vibrato_depth),
            ("vibrato_hz", self.vibrato_hz),
            ("lpf", self.lpf),
            ("hpf", self.hpf),
            ("gain", self.gain),
            ("delay", self.delay),
        ];
        for (name, v) in fields {
            if !v.is_finite() {
                return Err(format!("{name} is not finite"));
            }
        }
        for (name, v) in [
            ("freq", self.freq),
            ("attack", self.attack),
            ("sustain", self.sustain),
            ("decay", self.decay),
            ("vibrato_hz", self.vibrato_hz),
            ("lpf", self.lpf),
            ("hpf", self.hpf),
            ("delay", self.delay),
        ] {
            if v < 0.0 {
                return Err(format!("{name} is negative"));
            }
        }
        if !(self.duty > 0.0 && self.duty < 1.0) {
            return Err("duty must be in (0, 1)".into());
        }
        if self.crush_bits > 16 {
            return Err("crush_bits must be at most 16".into());
        }
        if looped
            && (self.attack != 0.0 || self.decay != 0.0 || self.punch != 0.0 || self.delay != 0.0)
        {
            return Err("loop layers need attack, decay, punch and delay of 0".into());
        }
        Ok(())
    }
}

impl Recipe {
    /// Length in seconds: the end of the last layer.
    pub fn seconds(&self) -> f64 {
        self.layers.iter().map(Layer::end).fold(0.0, f64::max)
    }

    fn check(&self) -> Result<(), String> {
        let name_ok = !self.name.is_empty()
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !name_ok {
            return Err(format!("bad name {:?} (use a-z, 0-9 and _)", self.name));
        }
        let fail = |e: String| format!("{}: {e}", self.name);
        if self.layers.is_empty() {
            return Err(fail("no layers".into()));
        }
        for (i, layer) in self.layers.iter().enumerate() {
            layer
                .check(self.looped)
                .map_err(|e| fail(format!("layer {i}: {e}")))?;
        }
        let secs = self.seconds();
        if samples(secs) == 0 {
            return Err(fail("zero length".into()));
        }
        if secs > MAX_SECONDS {
            return Err(fail(format!("too long ({secs} s, max {MAX_SECONDS} s)")));
        }
        Ok(())
    }
}

/// Parses and checks a recipe file.
pub fn parse_recipes(src: &str) -> Result<Vec<Recipe>, String> {
    let recipes: Vec<Recipe> = ron::from_str(src).map_err(|e| format!("parse error: {e}"))?;
    for (i, r) in recipes.iter().enumerate() {
        r.check()?;
        if recipes[..i].iter().any(|o| o.name == r.name) {
            return Err(format!("duplicate name {:?}", r.name));
        }
    }
    Ok(recipes)
}

fn samples(secs: f64) -> usize {
    (secs * SR).round() as usize
}

/// sin(2π·p) for `p` in turns, from a polynomial so it is identical on every platform.
fn sin_turns(p: f64) -> f64 {
    let p = p - p.floor();
    let (sign, q) = if p < 0.5 { (1.0, p) } else { (-1.0, p - 0.5) };
    let q = if q > 0.25 { 0.5 - q } else { q };
    let x = std::f64::consts::TAU * q;
    let x2 = x * x;
    // Taylor series to x^13; error below 1e-9 on [0, π/2].
    let s = x
        * (1.0
            - x2 / 6.0
                * (1.0
                    - x2 / 20.0
                        * (1.0
                            - x2 / 42.0
                                * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0 * (1.0 - x2 / 156.0))))));
    sign * s
}

/// One-pole low-pass smoothing factor for a cutoff in Hz (the RC-filter form, no `exp`).
fn one_pole(cutoff: f64) -> f64 {
    let w = std::f64::consts::TAU * cutoff / SR;
    w / (1.0 + w)
}

/// Renders `layer` into `out`, starting at its delay. In a loop (`looped`) the layer runs to the
/// end of `out`, holding its sustain level, instead of stopping at its own length.
fn render_layer(layer: &Layer, rng: &mut Rng, out: &mut [f64], looped: bool) {
    let start = samples(layer.delay);
    let (a, s, d) = (layer.attack, layer.sustain, layer.decay);
    let n = if looped {
        out.len().saturating_sub(start)
    } else {
        samples(layer.length()).min(out.len().saturating_sub(start))
    };
    let (lp_a, hp_a) = (one_pole(layer.lpf), one_pole(layer.hpf));
    let mut freq = layer.freq;
    let mut slide = layer.slide;
    let (mut phase, mut vib_phase, mut noise_phase) = (0.0f64, 0.0f64, 1.0f64);
    let (mut noise, mut brown, mut lp, mut hp_lp) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let crush = (layer.crush_bits > 0).then(|| (1u32 << (layer.crush_bits - 1)) as f64);
    for (i, slot) in out[start..start + n].iter_mut().enumerate() {
        let t = i as f64 / SR;
        let f = (freq + layer.vibrato_depth * sin_turns(vib_phase)).clamp(0.0, SR / 2.0);
        vib_phase += layer.vibrato_hz / SR;
        vib_phase -= vib_phase.floor();

        let x = match layer.wave {
            Wave::Square => {
                if phase < layer.duty {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => 1.0 - 2.0 * phase,
            Wave::Sine => sin_turns(phase),
            Wave::Noise | Wave::Brown => {
                let step = if f > 0.0 { f * NOISE_STEPS / SR } else { 1.0 };
                noise_phase += step;
                if noise_phase >= 1.0 {
                    noise_phase -= noise_phase.floor();
                    noise = rng.signed() as f64;
                }
                if layer.wave == Wave::Noise {
                    noise
                } else {
                    brown = brown * BROWN_LEAK + noise * (1.0 - BROWN_LEAK) * BROWN_GAIN;
                    brown
                }
            }
        };
        phase += f / SR;
        phase -= phase.floor();
        freq += slide / SR;
        slide += layer.delta_slide / SR;

        let mut y = x;
        if layer.lpf > 0.0 {
            lp += lp_a * (y - lp);
            y = lp;
        }
        if layer.hpf > 0.0 {
            hp_lp += hp_a * (y - hp_lp);
            y -= hp_lp;
        }

        let env = if looped {
            1.0
        } else if t < a {
            t / a
        } else if t < a + s {
            1.0 + layer.punch * (1.0 - (t - a) / s)
        } else if t < a + s + d {
            1.0 - (t - a - s) / d
        } else {
            0.0
        };
        y *= env;
        if let Some(q) = crush {
            y = (y * q).round() / q;
        }
        *slot += y * layer.gain;
    }
}

/// Renders a recipe to 16-bit samples at [`SAMPLE_RATE`].
pub fn render(recipe: &Recipe) -> Vec<i16> {
    let len = samples(recipe.seconds());
    let xfade = if recipe.looped {
        samples(LOOP_XFADE_SECONDS).min(len / 2)
    } else {
        0
    };
    let mut mix = vec![0.0f64; len + xfade];
    for (i, layer) in recipe.layers.iter().enumerate() {
        // Each layer gets its own stream, so editing one layer leaves the others' noise alone.
        let seed = recipe.seed ^ (i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        render_layer(layer, &mut Rng::new(seed), &mut mix, recipe.looped);
    }
    if recipe.looped {
        // The rendered tail continues the end seamlessly; blend it over the start.
        for i in 0..xfade {
            let w = (i as f64 + 0.5) / xfade as f64;
            mix[i] = mix[i] * w + mix[len + i] * (1.0 - w);
        }
        mix.truncate(len);
    } else {
        let fade = samples(FADE_SECONDS).min(len);
        for k in 0..fade {
            mix[len - 1 - k] *= k as f64 / fade as f64;
        }
    }
    let peak = mix.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let scale = if peak > 0.0 { PEAK / peak } else { 0.0 };
    mix.iter()
        .map(|v| (v * scale * i16::MAX as f64).round() as i16)
        .collect()
}

/// Encodes samples as a mono 16-bit WAV file.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Vec::new();
    let mut w = hound::WavWriter::new(Cursor::new(&mut buf), spec).expect("in-memory WAV");
    for &s in samples {
        w.write_sample(s).expect("in-memory WAV");
    }
    w.finalize().expect("in-memory WAV");
    buf
}

/// Renders every recipe in `recipes` to `<out_dir>/<name>.wav`; returns one line per file.
pub fn synth_file(recipes: &Path, out_dir: &Path) -> Result<Vec<String>, String> {
    let label = recipes.display();
    let src = std::fs::read_to_string(recipes).map_err(|e| format!("{label}: cannot read: {e}"))?;
    let list = parse_recipes(&src).map_err(|e| format!("{label}: {e}"))?;
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("{}: cannot create: {e}", out_dir.display()))?;
    let mut lines = Vec::new();
    for r in &list {
        let pcm = render(r);
        let path = out_dir.join(format!("{}.wav", r.name));
        std::fs::write(&path, wav_bytes(&pcm))
            .map_err(|e| format!("{}: cannot write: {e}", path.display()))?;
        lines.push(format!(
            "{}: {:.3} s{}",
            path.display(),
            pcm.len() as f64 / SR,
            if r.looped { ", loop" } else { "" }
        ));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const ONE_SHOT: &str = r#"[
        (name: "zap", layers: [
            (wave: Square, duty: 0.5, freq: 520.0, slide: -900.0,
             sustain: 0.04, decay: 0.12, punch: 0.4, lpf: 6000.0, hpf: 80.0, gain: 0.8),
            (wave: Noise, freq: 400.0, decay: 0.1, gain: 0.5, delay: 0.05, crush_bits: 6),
        ], seed: 7),
    ]"#;

    const LOOPED: &str = r#"[
        (name: "hum", loop: true, layers: [
            (wave: Saw, freq: 55.0, sustain: 1.0, lpf: 800.0, gain: 0.7, vibrato_depth: 2.0, vibrato_hz: 4.0),
            (wave: Brown, freq: 2000.0, sustain: 1.0, gain: 0.3),
        ]),
    ]"#;

    fn one(src: &str) -> Recipe {
        parse_recipes(src).unwrap().remove(0)
    }

    fn peak_db(s: &[i16]) -> f64 {
        let peak = s.iter().map(|v| (*v as f64).abs()).fold(0.0, f64::max);
        20.0 * (peak / 32767.0).log10()
    }

    #[test]
    fn same_recipe_gives_identical_bytes() {
        let r = one(ONE_SHOT);
        assert_eq!(wav_bytes(&render(&r)), wav_bytes(&render(&r)));
        // A different seed changes the noise layer.
        let mut r2 = r.clone();
        r2.seed = 8;
        assert_ne!(render(&r), render(&r2));
    }

    #[test]
    fn wav_header_and_length() {
        let r = one(ONE_SHOT);
        let bytes = wav_bytes(&render(&r));
        let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 22050);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        // Longest layer: 0.04 + 0.12 = 0.16 s vs 0.05 + 0.1 = 0.15 s.
        assert_eq!(reader.len(), (0.16f64 * 22050.0).round() as u32);
    }

    #[test]
    fn peak_is_minus_one_dbfs() {
        for src in [ONE_SHOT, LOOPED] {
            let db = peak_db(&render(&one(src)));
            assert!((db + 1.0).abs() < 0.05, "peak {db} dB");
        }
    }

    #[test]
    fn silent_recipe_stays_silent() {
        let r = one(
            r#"[(name: "quiet", layers: [(wave: Sine, freq: 440.0, sustain: 0.1, gain: 0.0)])]"#,
        );
        assert!(render(&r).iter().all(|&s| s == 0));
    }

    #[test]
    fn one_shots_end_at_zero_loops_do_not_fade() {
        let s = render(&one(ONE_SHOT));
        assert_eq!(*s.last().unwrap(), 0);
        // One-shots fade to exactly zero; loops start and end mid-wave and wrap seamlessly.
        let s = render(&one(LOOPED));
        assert_eq!(s.len(), 22050);
        assert_ne!(s[0], 0);
        assert_ne!(*s.last().unwrap(), 0);
        // Seamless: the wrap from last to first is no bigger a step than typical neighbours.
        let max_step = s
            .windows(2)
            .map(|w| (w[1] as i32 - w[0] as i32).abs())
            .max()
            .unwrap();
        let wrap = (s[0] as i32 - *s.last().unwrap() as i32).abs();
        assert!(wrap <= max_step, "wrap {wrap} > max step {max_step}");
    }

    #[test]
    fn sine_is_accurate() {
        for i in 0..1000 {
            let p = i as f64 / 1000.0;
            let want = (p * std::f64::consts::TAU).sin();
            assert!((sin_turns(p) - want).abs() < 1e-6, "p {p}");
        }
    }

    #[test]
    fn malformed_recipes_are_clean_errors() {
        for (src, needle) in [
            ("[(name: \"x\", layers: [(wave: Triangle)])]", "parse"),
            ("[(name: \"x\"", "parse"),
            ("[(name: \"x\", layers: [])]", "no layers"),
            ("[(name: \"x\", layers: [(wave: Sine)])]", "zero length"),
            (
                "[(name: \"../x\", layers: [(wave: Sine, sustain: 0.1)])]",
                "name",
            ),
            (
                "[(name: \"x\", layers: [(wave: Sine, sustain: 0.1)]), (name: \"x\", layers: [(wave: Sine, sustain: 0.1)])]",
                "duplicate",
            ),
            (
                "[(name: \"x\", layers: [(wave: Sine, sustain: -1.0)])]",
                "negative",
            ),
            (
                "[(name: \"x\", layers: [(wave: Sine, sustain: 60.0)])]",
                "too long",
            ),
        ] {
            let err = parse_recipes(src).unwrap_err();
            assert!(err.contains(needle), "{src}: {err}");
        }
    }

    fn real_recipes() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds/synth.ron")
    }

    const NAMES: &[&str] = &[
        "pistol",
        "shotgun",
        "chaingun",
        "rocket_launch",
        "bomb_throw",
        "dry_fire",
        "reload",
        "weapon_switch",
        "kick_swoosh",
        "ricochet",
        "impact",
        "explosion",
        "pickup_item",
        "pickup_weapon",
        "pickup_ammo",
        "pickup_health",
        "pickup_key",
        "pickup_power",
        "secret",
        "switch_click",
        "denied",
        "nightvision_on",
        "nightvision_off",
        "enemy_bolt",
        "drone_hum",
        "glass_break",
        "light_break",
        "door_servo",
        "door_stop",
        "lift_hum",
        "lift_stop",
        "jetpack_loop",
        "jetpack_start",
        "jetpack_stop",
        "footstep",
        "land",
        "player_hurt",
        "player_death",
        "enemy_wake",
        "enemy_pain",
        "enemy_death",
        "level_complete",
        "hazard_sizzle",
        "hazard_zap",
        "quake_rumble",
        "toilet_flush",
        "vending_dispense",
        "vending_empty",
        "pool_break",
        "boss_roar",
        "boss_rocket",
        "boss_minigun",
        "boss_death",
    ];

    #[test]
    fn shipped_recipes_parse_and_cover_every_name() {
        let src = std::fs::read_to_string(real_recipes()).unwrap();
        let recipes = parse_recipes(&src).unwrap();
        for name in NAMES {
            assert!(recipes.iter().any(|r| r.name == *name), "missing {name}");
        }
        let mut bytes = 0;
        for r in &recipes {
            let pcm = render(r);
            bytes += wav_bytes(&pcm).len();
            let secs = pcm.len() as f64 / SAMPLE_RATE as f64;
            let max = if r.looped { 2.0 } else { 2.5 };
            assert!(secs <= max + 1e-9, "{} is {secs} s", r.name);
            let looped = ["drone_hum", "lift_hum", "jetpack_loop", "quake_rumble"]
                .contains(&r.name.as_str());
            assert_eq!(r.looped, looped, "{} loop flag", r.name);
            if looped {
                assert!(secs >= 1.0, "{} loop is {secs} s", r.name);
            }
        }
        assert!(bytes <= 4 << 20, "{bytes} bytes of WAV");
    }

    #[test]
    fn committed_wavs_match_their_recipes() {
        let src = std::fs::read_to_string(real_recipes()).unwrap();
        let dir = real_recipes().with_file_name("synth");
        for r in parse_recipes(&src).unwrap() {
            let path = dir.join(format!("{}.wav", r.name));
            let committed =
                std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                committed == wav_bytes(&render(&r)),
                "{}.wav is stale: regenerate with `cargo run -p rr-tools -- synth assets/sounds/synth.ron -o assets/sounds/synth`",
                r.name
            );
        }
    }

    #[test]
    fn synth_file_writes_one_wav_per_recipe() {
        let dir = std::env::temp_dir().join(format!("rr-synth-test-{}", std::process::id()));
        let recipes = dir.join("r.ron");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&recipes, LOOPED).unwrap();
        let out = dir.join("out");
        let lines = synth_file(&recipes, &out).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("hum.wav"), "{lines:?}");
        assert!(out.join("hum.wav").is_file());
        assert!(synth_file(&dir.join("missing.ron"), &out).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
