//! Sound effects: the [`SoundBank`] from `assets/sounds/bank.ron`, positional one-shots for
//! every gameplay cue, loops for moving doors and lifts and the burning jetpack, and the
//! player's footsteps.
//!
//! Every cue goes through [`play_cues`]: core events from `FxQueue` are mapped by the
//! pure `rr_core::audio` functions, and the cues the game raises itself (footsteps, landing,
//! powers switching on and off) wait in [`GameCues`]. Distance falloff is the bank's own
//! [`gain`]; Bevy's spatial audio only pans (see [`spatial_scale`]).
//!
//! Every system is gated on [`SoundBank`], which only [`AudioFxPlugin`] inserts, so headless
//! sim tests run without audio.

mod sfx;

use crate::combat::{FxReaders, PlayerInventory, eye_of};
use crate::flow::{PlayState, SpawnLevel};
use crate::paths::assets_dir;
use crate::player::{Player, PlayerBody, PlayerCamera, PlayerSimSet, spawn_player};
use bevy::audio::{AudioSinkPlayback, SpatialAudioSink, SpatialScale, Volume};
use bevy::prelude::*;
use rr_core::audio::{Cue, CuePos, Footsteps, PowerWatch, SoundBankDef, SoundDef, gain};
use rr_core::map::SectorId;
use rr_core::rng::Rng;
use std::collections::HashMap;

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

/// Loads the sound bank and plays every gameplay cue. Needs `AssetPlugin` and the sim plugins.
pub struct AudioFxPlugin;

impl Plugin for AudioFxPlugin {
    fn build(&self, app: &mut App) {
        let has_bank = resource_exists::<SoundBank>;
        app.init_resource::<GameCues>()
            .insert_resource(SfxRng(Rng::new(SFX_SEED)))
            .add_systems(Startup, load_bank)
            .add_systems(SpawnLevel, attach_player_audio.after(spawn_player))
            .add_systems(
                FixedUpdate,
                player_cues.after(PlayerSimSet).run_if(has_bank),
            )
            .add_systems(
                Update,
                (
                    attach_listener,
                    (play_cues, loop_gain).chain().in_set(FxReaders),
                )
                    .run_if(has_bank),
            );
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
    mut cues: Option<ResMut<GameCues>>,
    players: Query<Entity, With<Player>>,
) {
    if let Some(cues) = cues.as_mut() {
        cues.0.clear();
    }
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
/// audible source within one unit, so rodio only pans and the bank's [`gain`] sets the level.
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
    global: Option<Res<GlobalVolume>>,
    player: Query<&PlayerBody, With<Player>>,
    mut loops: Query<(&SfxVoice, &Transform, &mut SpatialAudioSink), With<MoverLoop>>,
) {
    let Ok(body) = player.single() else { return };
    let ear = crate::coords::to_bevy(eye_of(&body.0));
    // Absent without Bevy's `AudioPlugin` (headless tests), where there are no sinks either.
    let global = global.map_or(Volume::Linear(1.0), |g| g.volume);
    for (voice, tf, mut sink) in &mut loops {
        let v = voice_volume(bank.def(voice.0), tf.translation.distance(ear));
        sink.set_volume(v * global);
    }
}
