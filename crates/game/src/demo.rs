//! Scripted demo playback: a RON script of timed input segments drives the player instead of
//! the keyboard and mouse, one fixed tick at a time, so a run is deterministic. With a record
//! directory every rendered frame is saved as a PNG at a fixed 30 fps (see
//! `scripts/record-demo.sh`).

use crate::combat::PlayerVitals;
use crate::player::{Look, PendingInput, Player, PlayerBody};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use rr_core::defs::WeaponId;
use rr_core::health::PLAYER_MAX_HEALTH;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Fixed ticks per second; matches `PlayerSimPlugin`'s `Time::<Fixed>::from_hz(60.0)`.
const TICK_HZ: f32 = 60.0;
/// Recorded frames per second.
pub const RECORD_FPS: u32 = 30;
/// Frames rendered after the script ends before exiting, so pending screenshots are saved.
const EXIT_GRACE_FRAMES: u32 = 10;

/// One span of held input. Movement and fire are held for the whole segment; the one-shot
/// presses (`use_key`, `select`, `reload`, …) fire once on its first tick.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Segment {
    /// Duration in seconds.
    pub secs: f32,
    pub forward: f32,
    pub strafe: f32,
    /// Total heading change over the segment, degrees, positive = left (CCW).
    pub turn_deg: f32,
    /// Pitch the view eases to (linearly) by the end of the segment, degrees, up-positive.
    pub pitch_deg: Option<f32>,
    pub fire: bool,
    pub jump: bool,
    pub crouch: bool,
    pub use_key: bool,
    pub select: Option<WeaponId>,
    pub reload: bool,
    pub kick: bool,
    pub use_medkit: bool,
    pub toggle_jetpack: bool,
    pub toggle_nv: bool,
}

impl Segment {
    fn ticks(&self) -> u32 {
        (self.secs * TICK_HZ).round().max(1.0) as u32
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct DemoScript {
    /// Keep the player at full health so a showcase run cannot die. A single hit above full
    /// health (a point-blank explosion) still kills.
    #[serde(default)]
    pub invulnerable: bool,
    pub segments: Vec<Segment>,
}

impl DemoScript {
    pub fn from_ron(src: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(src)
    }
}

/// Playback position within a [`DemoScript`].
#[derive(Resource, Debug)]
pub struct DemoPlayback {
    script: DemoScript,
    segment: usize,
    tick: u32,
    /// Pitch at the start of the current segment, for the linear ease.
    pitch_from: f32,
}

impl DemoPlayback {
    pub fn new(script: DemoScript) -> Self {
        Self {
            script,
            segment: 0,
            tick: 0,
            pitch_from: 0.0,
        }
    }

    pub fn finished(&self) -> bool {
        self.segment >= self.script.segments.len()
    }

    /// Applies one fixed tick of the script to the player's input and view, then advances.
    /// Returns false once the script has ended (and leaves the input idle).
    pub fn step(&mut self, input: &mut PendingInput, look: &mut Look) -> bool {
        let Some(seg) = self.script.segments.get(self.segment) else {
            *input = PendingInput::default();
            return false;
        };
        let ticks = seg.ticks();
        if self.tick == 0 {
            self.pitch_from = look.pitch;
            input.use_pressed |= seg.use_key;
            input.fire_pressed |= seg.fire;
            input.reload |= seg.reload;
            input.kick |= seg.kick;
            input.use_medkit |= seg.use_medkit;
            input.toggle_jetpack |= seg.toggle_jetpack;
            input.toggle_nv |= seg.toggle_nv;
            if seg.select.is_some() {
                input.select = seg.select;
            }
        }
        input.forward = seg.forward;
        input.strafe = seg.strafe;
        input.jump = seg.jump;
        input.crouch = seg.crouch;
        input.fire = seg.fire;
        look.angle += seg.turn_deg.to_radians() / ticks as f32;
        self.tick += 1;
        if let Some(target) = seg.pitch_deg {
            let t = self.tick as f32 / ticks as f32;
            look.pitch = self.pitch_from + (target.to_radians() - self.pitch_from) * t;
        }
        if self.tick >= ticks {
            self.segment += 1;
            self.tick = 0;
        }
        true
    }
}

/// Loads a demo script, panicking with the path on failure.
pub fn load_script(path: &std::path::Path) -> DemoScript {
    let src = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read demo script {}: {e}", path.display()));
    DemoScript::from_ron(&src)
        .unwrap_or_else(|e| panic!("invalid demo script {}: {e}", path.display()))
}

/// Drives the player from a script. Add it instead of the keyboard/mouse input of
/// `PlayerControlPlugin { scripted: true }`.
pub struct DemoPlugin {
    pub script: DemoScript,
    /// Save every rendered frame as `frame_NNNNN.png` here (needs a window).
    pub record: Option<PathBuf>,
}

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DemoPlayback::new(self.script.clone()))
            // Before `FixedUpdate`, where movement, use and combat consume the input.
            .add_systems(FixedPreUpdate, drive_player)
            .add_systems(Last, exit_when_done);
        if let Some(dir) = &self.record {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
            app.insert_resource(TimeUpdateStrategy::ManualDuration(
                Duration::from_secs(1) / RECORD_FPS,
            ))
            .insert_resource(RecordDir(dir.clone()))
            .add_systems(Last, capture_frame);
        }
    }
}

#[derive(Resource)]
struct RecordDir(PathBuf);

type DemoPlayer = (
    &'static mut PendingInput,
    &'static mut Look,
    &'static PlayerBody,
    Option<&'static mut PlayerVitals>,
);

/// Logs where each segment ends, which is what script authoring needs.
fn drive_player(mut playback: ResMut<DemoPlayback>, mut player: Query<DemoPlayer, With<Player>>) {
    if let Ok((mut input, mut look, body, vitals)) = player.single_mut() {
        if playback.script.invulnerable
            && let Some(mut vitals) = vitals
        {
            vitals.0.heal_to(PLAYER_MAX_HEALTH, PLAYER_MAX_HEALTH);
        }
        let segment = playback.segment;
        if playback.step(&mut input, &mut look) && playback.segment != segment {
            let p = body.0.pos;
            info!(
                "demo segment {segment} ends at ({:.1}, {:.1}, {:.1}) heading {:.0}°",
                p.x,
                p.y,
                p.z,
                look.angle.to_degrees()
            );
        }
    }
}

fn exit_when_done(
    playback: Res<DemoPlayback>,
    mut grace: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    if playback.finished() {
        *grace += 1;
        if *grace >= EXIT_GRACE_FRAMES {
            exit.write(AppExit::Success);
        }
    }
}

fn capture_frame(
    mut commands: Commands,
    dir: Res<RecordDir>,
    playback: Res<DemoPlayback>,
    mut frame: Local<u32>,
) {
    if playback.finished() {
        return;
    }
    let path = dir.0.join(format!("frame_{:05}.png", *frame));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    *frame += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        Look {
            angle: 0.0,
            pitch: 0.0,
        }
    }

    #[test]
    fn parses_segments_with_defaults() {
        let s = DemoScript::from_ron(
            "(segments: [(secs: 1.0, forward: 1.0), (secs: 0.5, select: Some(Shotgun), fire: true)])",
        )
        .unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0].forward, 1.0);
        assert!(!s.segments[0].fire);
        assert_eq!(s.segments[1].select, Some(WeaponId::Shotgun));
        assert_eq!(s.segments[1].ticks(), 30);
    }

    #[test]
    fn shipped_demo_script_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/demo/arsenal_depot.ron");
        assert!(!load_script(&path).segments.is_empty());
    }

    #[test]
    fn holds_movement_and_turns_evenly_then_finishes() {
        let script = DemoScript {
            invulnerable: false,
            segments: vec![Segment {
                secs: 0.5,
                forward: 1.0,
                turn_deg: 90.0,
                ..default()
            }],
        };
        let mut pb = DemoPlayback::new(script);
        let (mut input, mut look) = (PendingInput::default(), look());
        for _ in 0..30 {
            assert!(pb.step(&mut input, &mut look));
            assert_eq!(input.forward, 1.0);
        }
        assert!((look.angle - 90f32.to_radians()).abs() < 1e-4);
        assert!(pb.finished());
        assert!(!pb.step(&mut input, &mut look));
        assert_eq!(input.forward, 0.0);
    }

    #[test]
    fn one_shot_presses_latch_only_on_the_first_tick() {
        let script = DemoScript {
            invulnerable: false,
            segments: vec![Segment {
                secs: 0.1,
                use_key: true,
                select: Some(WeaponId::Rockets),
                ..default()
            }],
        };
        let mut pb = DemoPlayback::new(script);
        let (mut input, mut look) = (PendingInput::default(), look());
        pb.step(&mut input, &mut look);
        assert!(input.use_pressed);
        assert_eq!(input.select, Some(WeaponId::Rockets));
        // The sim consumes latches; later ticks of the segment must not set them again.
        input.use_pressed = false;
        input.select = None;
        pb.step(&mut input, &mut look);
        assert!(!input.use_pressed);
        assert_eq!(input.select, None);
    }

    #[test]
    fn pitch_eases_linearly_to_the_target() {
        let script = DemoScript {
            invulnerable: false,
            segments: vec![Segment {
                secs: 1.0,
                pitch_deg: Some(-20.0),
                ..default()
            }],
        };
        let mut pb = DemoPlayback::new(script);
        let (mut input, mut look) = (PendingInput::default(), look());
        for _ in 0..30 {
            pb.step(&mut input, &mut look);
        }
        assert!((look.pitch - (-10f32).to_radians()).abs() < 1e-4);
        for _ in 0..30 {
            pb.step(&mut input, &mut look);
        }
        assert!((look.pitch - (-20f32).to_radians()).abs() < 1e-4);
    }
}
