//! Sound effects: the [`SoundBank`] from `assets/sounds/bank.ron`, positional one-shots for
//! every gameplay cue, loops for moving doors and lifts and the burning jetpack, and the
//! player's footsteps.
//!
//! Every cue goes through [`play_cues`]: core events from `FxQueue` are mapped by the
//! pure `rr_core::audio` functions, and the cues the game raises itself (footsteps, landing,
//! powers switching on and off) wait in [`GameCues`]. Distance falloff is the bank's own
//! [`gain`]; Bevy's spatial audio only pans (see [`spatial_scale`]).
//!
//! The hero's quips (with a HUD subtitle) are in [`quips`], the level music in [`music`].
//! [`AudioVolumes`] holds the mix; `M` mutes and `[` / `]` step the master volume.
//!
//! Every system is gated on [`SoundBank`], which only [`AudioFxPlugin`] inserts, so headless
//! sim tests run without audio.

mod music;
mod quips;
mod sfx;

use crate::combat::{FxReaders, PlayerInventory, eye_of};
use crate::flow::{PlayState, SpawnLevel, run_spawn_level};
use crate::mechanics::HudMessage;
use crate::paths::assets_dir;
use crate::player::{Player, PlayerBody, PlayerCamera, PlayerSimSet, spawn_player};
use bevy::audio::{AudioSinkPlayback, SpatialAudioSink, SpatialScale, Volume};
use bevy::prelude::*;
use rr_core::audio::{Cue, CuePos, Footsteps, PowerWatch, SoundBankDef, SoundDef, gain};
use rr_core::map::SectorId;
use rr_core::rng::Rng;
use std::collections::HashMap;

pub use music::MusicTrack;
pub use quips::{QuipLines, QuipState, QuipVoice};
pub use sfx::play_cues;

/// At most this many sounds play at once, over all cues.
pub const MAX_LIVE_VOICES: usize = 32;

/// Seed of the RNG that picks variants and pitch jitter (not the gameplay RNG, so sound never
/// changes the simulation).
const SFX_SEED: u64 = 0x5f3a_17c0_d00d_cafe;

/// The parsed bank and one loaded handle per referenced file.
#[derive(Resource)]
pub struct SoundBank {
    pub defs: SoundBankDef,
    handles: HashMap<String, Handle<AudioSource>>,
}

impl SoundBank {
    /// The def for `cue`. The bank is checked at load to cover every cue.
    pub fn def(&self, cue: Cue) -> &SoundDef {
        self.defs
            .get(cue)
            .unwrap_or_else(|| panic!("sound bank has no entry for {cue:?}"))
    }

    pub fn handle(&self, file: &str) -> Handle<AudioSource> {
        self.handles[file].clone()
    }
}

/// Cues the game raises itself (no core event), drained by [`play_cues`] each frame.
#[derive(Resource, Default, Debug)]
pub struct GameCues(pub Vec<(Cue, CuePos)>);

/// Picks variants and pitch jitter; deterministic.
#[derive(Resource)]
pub struct SfxRng(Rng);

/// A playing sound of `cue` (one-shots and loops alike); counted for the voice caps.
#[derive(Component, Debug)]
pub struct SfxVoice(pub Cue);

/// The hum of a door or lift sector while it moves.
#[derive(Component, Debug)]
pub struct MoverLoop(pub SectorId);

/// The jetpack's burn loop while it is on.
#[derive(Component, Debug)]
pub struct JetpackHum;

/// Per-player detectors for the cues the game raises itself.
#[derive(Component, Default)]
pub struct PlayerAudio {
    steps: Footsteps,
    powers: PowerWatch,
}

/// Master volume step of the `[` and `]` keys.
const VOLUME_STEP: f32 = 0.1;

/// The mix. `master` and `muted` drive Bevy's [`GlobalVolume`]; `sfx`, `voice` (quips) and
/// `music` scale each kind of sound as it starts. All linear, 0..1.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct AudioVolumes {
    pub master: f32,
    pub sfx: f32,
    pub voice: f32,
    pub music: f32,
    pub muted: bool,
}

impl Default for AudioVolumes {
    fn default() -> Self {
        Self {
            master: 1.0,
            sfx: 0.8,
            voice: 1.0,
            music: 0.5,
            muted: false,
        }
    }
}

impl AudioVolumes {
    /// The global volume: the master, or silence while muted.
    pub fn global(&self) -> f32 {
        if self.muted { 0.0 } else { self.master }
    }

    /// Steps the master by `delta`, kept on whole tenths within 0..1.
    pub fn step_master(&mut self, delta: f32) {
        self.master = ((self.master + delta) * 10.0).round().clamp(0.0, 10.0) / 10.0;
    }
}

/// Audio settings from the command line.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioOptions {
    /// Start muted (`--mute`, and always while recording).
    pub muted: bool,
    /// Play level music (skipped while recording: manual time would let it drift).
    pub music: bool,
}

impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            muted: false,
            music: true,
        }
    }
}

/// Loads the sound bank and the quip table and plays every gameplay cue, the hero's quips and
/// the level music. Needs `AssetPlugin` and the sim plugins.
#[derive(Default)]
pub struct AudioFxPlugin {
    pub options: AudioOptions,
    /// Input comes from a demo script: the volume keys are not read.
    pub scripted: bool,
}

impl Plugin for AudioFxPlugin {
    fn build(&self, app: &mut App) {
        let has_bank = resource_exists::<SoundBank>;
        app.init_resource::<GameCues>()
            .insert_resource(SfxRng(Rng::new(SFX_SEED)))
            .insert_resource(self.options)
            .insert_resource(AudioVolumes {
                muted: self.options.muted,
                ..default()
            })
            // Loaded before the first level spawn, whose audio systems are gated on the bank.
            .add_systems(
                Startup,
                (load_bank, quips::load_quips).before(run_spawn_level),
            )
            .add_systems(
                SpawnLevel,
                (
                    attach_player_audio.after(spawn_player),
                    quips::reset_quips,
                    music::start_music,
                )
                    .run_if(has_bank),
            )
            .add_systems(
                FixedUpdate,
                player_cues.after(PlayerSimSet).run_if(has_bank),
            )
            .add_systems(
                Update,
                (
                    apply_volumes
                        .run_if(resource_changed::<AudioVolumes>)
                        .before(FxReaders),
                    attach_listener,
                    (play_cues, loop_gain).chain().in_set(FxReaders),
                    quips::play_quips
                        .in_set(FxReaders)
                        .run_if(resource_exists::<QuipState>),
                )
                    .run_if(has_bank),
            );
        if !self.scripted {
            app.add_systems(
                Update,
                volume_keys
                    .before(apply_volumes)
                    .run_if(has_bank)
                    .run_if(resource_exists::<ButtonInput<KeyCode>>),
            );
        }
    }
}

/// Reads `bank.ron` (panicking with the path on any problem) and starts loading every file.
fn load_bank(mut commands: Commands, server: Res<AssetServer>) {
    let path = assets_dir().join("sounds/bank.ron");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read sound bank {}: {e}", path.display()));
    let defs = SoundBankDef::from_ron(&src)
        .unwrap_or_else(|e| panic!("invalid sound bank {}: {e}", path.display()));
    let problems = defs.problems();
    assert!(
        problems.is_empty(),
        "invalid sound bank {}: {problems:?}",
        path.display()
    );
    let handles = defs
        .files()
        .into_iter()
        .map(|f| (f.to_owned(), server.load(f.to_owned())))
        .collect();
    commands.insert_resource(SoundBank { defs, handles });
}

/// The ears sit on the player camera. Bevy's default ear gap (4 m) is kept: panning depends
/// only on the direction for sources more than a few metres away, and a wide gap keeps close
/// sounds from snapping hard to one side.
fn attach_listener(
    mut commands: Commands,
    cams: Query<Entity, (With<PlayerCamera>, Without<SpatialListener>)>,
) {
    for e in &cams {
        commands.entity(e).insert(SpatialListener::default());
    }
}

/// Fresh detectors for the newly spawned player; old cues from the previous run are dropped.
fn attach_player_audio(
    mut commands: Commands,
    mut cues: ResMut<GameCues>,
    players: Query<Entity, With<Player>>,
) {
    cues.0.clear();
    for e in &players {
        commands.entity(e).insert(PlayerAudio::default());
    }
}

/// Footsteps and landings while playing, and the powers switching on or off. The jetpack
/// counts as off once play stops, so its loop does not burn on through the death screen.
fn player_cues(
    state: Res<PlayState>,
    mut cues: ResMut<GameCues>,
    mut q: Query<(&PlayerBody, Option<&PlayerInventory>, &mut PlayerAudio)>,
) {
    let playing = *state == PlayState::Playing;
    let mut raised = Vec::new();
    for (body, inv, mut audio) in &mut q {
        if playing && let Some(c) = audio.steps.tick_body(&body.0) {
            raised.push(c);
        }
        let (jet, nv) = inv.map_or((false, false), |i| (i.0.jetpack_on, i.0.nv_on));
        audio.powers.tick(jet && playing, nv, &mut raised);
    }
    cues.0.extend(raised.into_iter().map(|c| (c, None)));
}

/// Scale for Bevy's spatial audio. Rodio attenuates by `1 / d²` in scaled units (capped at 1),
/// which at our metre scale would mute a door 10 m away; scaling by `1 / max_dist` keeps every
/// audible source within about one unit, so the bank's [`gain`] sets the level and rodio
/// mostly pans (its residual per-ear falloff is negligible, at most ~9% at `max_dist`).
pub fn spatial_scale(def: &SoundDef) -> SpatialScale {
    SpatialScale::new(1.0 / def.max_dist)
}

/// Linear volume for `def` heard from `dist` metres.
pub fn voice_volume(def: &SoundDef, dist: f32) -> Volume {
    Volume::Linear(def.volume * gain(def, dist))
}

/// Mover loops keep playing while the player walks about; follow their distance gain.
fn loop_gain(
    bank: Res<SoundBank>,
    volumes: Res<AudioVolumes>,
    global: Option<Res<GlobalVolume>>,
    player: Query<&PlayerBody, With<Player>>,
    mut loops: Query<(&SfxVoice, &Transform, &mut SpatialAudioSink), With<MoverLoop>>,
) {
    let Ok(body) = player.single() else { return };
    let ear = crate::coords::to_bevy(eye_of(&body.0));
    // Absent without Bevy's `AudioPlugin` (headless tests), where there are no sinks either.
    let global = global.map_or(Volume::Linear(1.0), |g| g.volume) * Volume::Linear(volumes.sfx);
    for (voice, tf, mut sink) in &mut loops {
        let v = voice_volume(bank.def(voice.0), tf.translation.distance(ear));
        sink.set_volume(v * global);
    }
}

/// `M` mutes or unmutes, `[` and `]` step the master volume; each shows on the HUD.
fn volume_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut volumes: ResMut<AudioVolumes>,
    mut msg: ResMut<HudMessage>,
) {
    if keys.just_pressed(KeyCode::KeyM) {
        volumes.muted = !volumes.muted;
        msg.show(if volumes.muted {
            "Sound off"
        } else {
            "Sound on"
        });
    }
    for (key, delta) in [
        (KeyCode::BracketLeft, -VOLUME_STEP),
        (KeyCode::BracketRight, VOLUME_STEP),
    ] {
        if keys.just_pressed(key) {
            volumes.step_master(delta);
            let pct = (volumes.master * 100.0).round();
            msg.show(if volumes.muted {
                format!("Volume {pct}% (muted)")
            } else {
                format!("Volume {pct}%")
            });
        }
    }
}

/// Sets Bevy's [`GlobalVolume`] from the mix. It only applies to sounds as they start, so the
/// playing ones are set again from their own spawn volume (mover loops follow in `loop_gain`).
fn apply_volumes(
    mut commands: Commands,
    volumes: Res<AudioVolumes>,
    mut sinks: Query<(
        &PlaybackSettings,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let global = Volume::Linear(volumes.global());
    commands.insert_resource(GlobalVolume::new(global));
    for (settings, sink, spatial) in &mut sinks {
        let v = settings.volume * global;
        if let Some(mut sink) = sink {
            sink.set_volume(v);
        }
        if let Some(mut sink) = spatial {
            sink.set_volume(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_steps_in_tenths_within_range() {
        let mut v = AudioVolumes::default();
        v.step_master(VOLUME_STEP);
        assert_eq!(v.master, 1.0);
        for _ in 0..3 {
            v.step_master(-VOLUME_STEP);
        }
        assert_eq!(v.master, 0.7);
        for _ in 0..12 {
            v.step_master(-VOLUME_STEP);
        }
        assert_eq!(v.master, 0.0);
        v.muted = true;
        v.step_master(VOLUME_STEP);
        assert_eq!((v.master, v.global()), (0.1, 0.0));
    }
}
