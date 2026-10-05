//! The hero's quips: this frame's events become [`QuipTrigger`]s, the core [`QuipDirector`]
//! picks a line (or none), and the line plays as a non-spatial voice with a [`HudSubtitle`]
//! (its own HUD line, so a quip never hides a message such as the key a door needs).

use super::AudioVolumes;
use crate::combat::{FxQueue, LevelCombat, PlayerArsenal, PlayerVitals};
use crate::flow::LevelEntity;
use crate::level::CurrentMap;
use crate::mechanics::HudSubtitle;
use crate::paths::assets_dir;
use crate::player::Player;
use bevy::audio::Volume;
use bevy::prelude::*;
use rr_core::audio::{LowHealthWatch, QuipDirector, QuipTable, QuipTrigger};
use rr_core::combat::{CombatEvent, level_seed};
use rr_core::defs::WeaponId;
use rr_core::map::ActorKind;
use rr_core::mechanics::MechEvent;

/// Mixed into the level seed so the quip picks have a stream of their own.
const QUIP_RNG_SALT: u64 = 0x2545_f491_4f6c_dd1d;

/// The parsed `assets/quips/quips.ron`.
#[derive(Resource)]
pub struct QuipLines(pub QuipTable);

/// The playing quip; a new quip cuts it.
#[derive(Component, Debug)]
pub struct QuipVoice;

/// Quip state of the current level run. Rebuilt by every level spawn, so the director's clock
/// starts at zero and never runs backwards, and a restart can quip right away.
#[derive(Resource)]
pub struct QuipState {
    pub director: QuipDirector,
    pub low_health: LowHealthWatch,
    pub voice: Option<Entity>,
    /// Seconds since the run started: the director's clock.
    pub clock: f32,
    /// Runs before this one (0 on the first spawn).
    run: u32,
    /// `LevelStart` or `Respawn`, raised on the run's first frame.
    pending: Option<QuipTrigger>,
    /// Weapons the player owned last frame; `None` before the run's first frame.
    owned: Option<[bool; WeaponId::ALL.len()]>,
}

/// Reads the quip table, panicking with the path on any problem.
pub fn load_quips(mut commands: Commands) {
    let path = assets_dir().join("quips/quips.ron");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read quips {}: {e}", path.display()));
    let table =
        QuipTable::parse(&src).unwrap_or_else(|e| panic!("invalid quips {}: {e}", path.display()));
    commands.insert_resource(QuipLines(table));
}

/// A fresh director for the new run, seeded from the level like `PlayRng` (the run number is
/// mixed in so a retry does not replay the same lines).
pub fn reset_quips(
    mut commands: Commands,
    lines: Res<QuipLines>,
    map: Res<CurrentMap>,
    old: Option<Res<QuipState>>,
) {
    let run = old.map_or(0, |s| s.run + 1);
    let seed = (level_seed(&map.0.name) ^ QUIP_RNG_SALT)
        .wrapping_add(u64::from(run).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    commands.insert_resource(QuipState {
        director: QuipDirector::new(lines.0.clone(), seed),
        low_health: LowHealthWatch::default(),
        voice: None,
        clock: 0.0,
        run,
        pending: Some(if run == 0 {
            QuipTrigger::LevelStart
        } else {
            QuipTrigger::Respawn
        }),
        owned: None,
    });
}

/// Feeds this frame's triggers to the director and plays its pick. A new weapon is one whose
/// `owned` flag turned on since last frame (a weapon pickup for one already owned only gives
/// ammo, so `ItemTaken` alone would over-report). An [`FxReaders`](crate::combat::FxReaders)
/// system.
#[allow(clippy::too_many_arguments)]
pub fn play_quips(
    mut commands: Commands,
    server: Res<AssetServer>,
    time: Res<Time>,
    fx: Res<FxQueue>,
    combat: Option<Res<LevelCombat>>,
    volumes: Res<AudioVolumes>,
    mut state: ResMut<QuipState>,
    mut subtitle: ResMut<HudSubtitle>,
    player: Query<(&PlayerVitals, &PlayerArsenal), With<Player>>,
) {
    let state = &mut *state;
    state.clock += time.delta_secs();
    let mut triggers: Vec<QuipTrigger> = state.pending.take().into_iter().collect();

    // A blast this frame makes every kill of it (barrels aside) a big one.
    let blast = fx
        .combat
        .iter()
        .any(|e| matches!(e, CombatEvent::Explosion { .. }));
    for ev in &fx.combat {
        match *ev {
            CombatEvent::ActorKilled(i) => {
                let Some(kind) = combat
                    .as_ref()
                    .and_then(|c| c.0.actors.get(i))
                    .map(|a| a.kind)
                else {
                    continue;
                };
                triggers.push(QuipTrigger::Kill(kind));
                if blast && kind != ActorKind::Barrel {
                    triggers.push(QuipTrigger::BigKill);
                }
            }
            CombatEvent::SecretFound => triggers.push(QuipTrigger::Secret),
            _ => {}
        }
    }
    for ev in &fx.mech {
        match ev {
            MechEvent::NeedKey(_) => triggers.push(QuipTrigger::NeedKey),
            MechEvent::Exit => triggers.push(QuipTrigger::LevelComplete),
            _ => {}
        }
    }
    if let Ok((vitals, arsenal)) = player.single() {
        let owned = arsenal.0.owned;
        if let Some(before) = state.owned {
            for w in WeaponId::ALL {
                if owned[w.index()] && !before[w.index()] {
                    triggers.push(QuipTrigger::NewWeapon(w));
                }
            }
        }
        state.owned = Some(owned);
        // The dead are past commenting on their health.
        let hp = vitals.0.health.hp;
        if hp > 0 && state.low_health.update(hp) {
            triggers.push(QuipTrigger::LowHealth);
        }
    }

    let Some(quip) = state.director.update(state.clock, &triggers) else {
        return;
    };
    if let Some(old) = state.voice.take() {
        commands.entity(old).try_despawn();
    }
    let voice = commands.spawn((
        AudioPlayer::<AudioSource>(server.load(format!("quips/{}.ogg", quip.id))),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volumes.voice)),
        QuipVoice,
        LevelEntity,
    ));
    state.voice = Some(voice.id());
    subtitle.show(quip.text.clone());
}
