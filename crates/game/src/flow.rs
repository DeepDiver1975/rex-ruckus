//! Game flow: playing, dead or level complete, and restarting the level from its authored map.
//!
//! Every level-scoped spawn (player, sector meshes, lights, props, combat) lives in the
//! [`SpawnLevel`] schedule; each plugin registers its own spawn systems there. `Startup` runs it
//! once, and [`restart_level`] despawns every [`LevelEntity`], resets the level resources and
//! runs it again. The camera and the HUD are spawned outside it and persist across restarts.

use crate::combat::{CombatSet, FxQueue, LevelCombat, PlayerVitals};
use crate::episode::Episode;
use crate::mechanics::{
    DirtySectors, HudMessage, HudSubtitle, UsePrompt, fresh_level, pickup_items,
};
use crate::net::{NetMode, not_client};
use crate::player::{LocalPlayer, PendingInput, Player, PlayerBody, PlayerSimSet, PrevFeet};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_core::map::Map;
use rr_core::players::PlayerId;

/// Seconds a death or completion screen stays up before a press may restart.
pub const RESTART_DELAY: f32 = 1.0;

/// Where play stands. Fixed-tick sim systems run on [`sim_ticks`]: `Playing`, or `Paused` from
/// `Playing` in a net game (the host's pause menu does not stop the world for its clients).
#[derive(Resource, PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum PlayState {
    /// Main menu over the frozen first level.
    Menu,
    #[default]
    Playing,
    /// Pause menu.
    Paused,
    Dead,
    /// Stats screen after a level.
    Complete,
    /// Episode totals after the last level.
    EpisodeEnd,
}

/// The state the pause menu was opened from; Esc (or Resume) returns there. Esc on the death
/// screen opens the pause menu too, and it must go back to `Dead`, not to `Playing`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PausedFrom(pub PlayState);

impl Default for PausedFrom {
    fn default() -> Self {
        Self(PlayState::Playing)
    }
}

/// Seconds since `PlayState` last changed; counts only while not `Playing`.
#[derive(Resource, Default, Debug)]
pub struct StateAge(pub f32);

/// Set by [`restart_on_press`]; [`restart_level`] runs while it is true.
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct RestartRequested(pub bool);

/// Set by [`restart_on_press`] for a completed level of an episode; [`advance_level`] runs while
/// it is true.
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct AdvanceRequested(pub bool);

/// The level file being played (a name under `assets/levels/`).
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct LevelFile(pub String);

/// How many times the level has been (re)started; a co-op host broadcasts a load when it changes.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelRuns(pub u32);

/// The seat the local player takes (`PlayerId(0)` unless a co-op host assigned another).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeat(pub PlayerId);

/// Whether the world is live: playing, or in a net game under the pause menu (a net game never
/// stops for a menu).
pub fn world_running(
    state: Res<PlayState>,
    from: Res<PausedFrom>,
    mode: Option<Res<NetMode>>,
) -> bool {
    world_live(*state, *from, mode.is_some_and(|m| m.is_net()))
}

/// [`world_running`] on plain values.
pub fn world_live(state: PlayState, from: PausedFrom, net: bool) -> bool {
    match state {
        PlayState::Playing => true,
        PlayState::Paused => net && from.0 == PlayState::Playing,
        _ => false,
    }
}

/// Whether this process steps the simulation: the world is running and it is not a co-op client.
pub fn sim_ticks(state: Res<PlayState>, from: Res<PausedFrom>, mode: Option<Res<NetMode>>) -> bool {
    let client = mode.as_deref() == Some(&NetMode::Client);
    !client && world_running(state, from, mode)
}

/// The level as authored (doors open), kept so a restart can rebuild it.
#[derive(Resource)]
pub struct LevelSource(pub Map);

/// The skill the level is played on; applied when the level is (re)built.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelDifficulty(pub Difficulty);

/// Marks every entity that belongs to the current level run; a restart despawns them all.
#[derive(Component, Default)]
pub struct LevelEntity;

/// Spawns everything level-scoped from `CurrentMap`. Run at startup and after every restart.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpawnLevel;

/// A restart needs the play to be over, the screen up for [`RESTART_DELAY`], and a fresh press.
pub fn restart_requested(
    state: PlayState,
    age: f32,
    use_pressed: bool,
    fire_pressed: bool,
) -> bool {
    matches!(state, PlayState::Dead | PlayState::Complete)
        && age >= RESTART_DELAY
        && (use_pressed || fire_pressed)
}

/// Owns the play state, the level-spawn schedule and restarts. Needs `insert_level`; the sim
/// plugins gate their fixed-tick systems on [`sim_ticks`] and the world's presentation on
/// [`world_running`] (both: [`PlayState::Playing`], or a net game under the pause menu).
pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayState>()
            .init_resource::<StateAge>()
            .init_resource::<PausedFrom>()
            .init_resource::<RestartRequested>()
            .init_resource::<AdvanceRequested>()
            .init_resource::<LevelDifficulty>()
            .init_resource::<LevelFile>()
            .init_resource::<LevelRuns>()
            .init_resource::<LocalSeat>()
            .init_schedule(SpawnLevel)
            .add_systems(Startup, run_spawn_level)
            .add_systems(
                FixedUpdate,
                freeze_interpolation
                    .run_if(not(world_running))
                    .before(PlayerSimSet)
                    .before(restart_on_press),
            )
            .add_systems(
                FixedUpdate,
                (
                    check_player_death
                        .run_if(sim_ticks)
                        .after(PlayerSimSet)
                        .after(CombatSet)
                        .after(pickup_items),
                    tick_state_age,
                    restart_on_press
                        .run_if(|s: Res<PlayState>| {
                            matches!(*s, PlayState::Dead | PlayState::Complete)
                        })
                        .run_if(not_client),
                    restart_level.run_if(resource_equals(RestartRequested(true))),
                    advance_level.run_if(resource_equals(AdvanceRequested(true))),
                )
                    .chain(),
            );
    }
}

/// Runs [`SpawnLevel`] once at startup; Startup systems that a spawn needs run before it.
pub fn run_spawn_level(world: &mut World) {
    world.run_schedule(SpawnLevel);
}

/// Play is over once every player is dead; a living co-op partner plays on.
pub(crate) fn check_player_death(
    mut state: ResMut<PlayState>,
    q: Query<&PlayerVitals, With<Player>>,
) {
    if !q.is_empty() && q.iter().all(|v| !v.0.health.alive()) {
        *state = PlayState::Dead;
    }
}

/// While play is frozen, collapses every render-interpolation endpoint onto the current
/// position. `Time<Fixed>` keeps accumulating overstep with the sim gated off, so a stale
/// previous position would make the frame loop redraw the last tick's step over and over
/// (visible shake on the death camera, chasing Grunts and bolts). Runs at the start of the
/// tick, so frames up to the first frozen tick still interpolate the final step normally.
/// A co-op client settles the same way on a tick with nothing to draw towards
/// (`net::interp`), all but its own predicted player.
pub(crate) fn freeze_interpolation(
    combat: Option<ResMut<LevelCombat>>,
    mut players: Query<(&PlayerBody, &mut PrevFeet)>,
) {
    for (body, mut prev) in &mut players {
        if prev.0 != body.0.pos {
            prev.0 = body.0.pos;
        }
    }
    if let Some(mut combat) = combat {
        settle_combat(&mut combat);
    }
}

/// Rests every actor's and projectile's previous pose on its current one, touching the
/// resource only when one differs.
pub(crate) fn settle_combat(combat: &mut ResMut<LevelCombat>) {
    let c = &combat.0;
    let stale = c.actors.iter().any(|a| a.prev_pos != a.body.pos)
        || c.projectiles.iter().any(|p| p.prev != p.pos);
    if !stale {
        return;
    }
    for a in &mut combat.0.actors {
        a.prev_pos = a.body.pos;
    }
    for p in &mut combat.0.projectiles {
        p.prev = p.pos;
    }
}

fn tick_state_age(time: Res<Time<Fixed>>, state: Res<PlayState>, mut age: ResMut<StateAge>) {
    if state.is_changed() {
        age.0 = 0.0;
    } else if *state != PlayState::Playing {
        age.0 += time.timestep().as_secs_f32();
    }
}

/// While the play is over, the sim systems no longer read the use and fire latches, so this
/// consumes them: a press during the grace period is dropped, not kept for later.
fn restart_on_press(
    state: Res<PlayState>,
    age: Res<StateAge>,
    mut request: ResMut<RestartRequested>,
    mut advance: ResMut<AdvanceRequested>,
    episode: Option<Res<Episode>>,
    mut q: Query<(Has<LocalPlayer>, &mut PendingInput), With<Player>>,
) {
    // Every player's latches are consumed; only the local player's presses restart.
    for (local, mut input) in &mut q {
        let use_pressed = std::mem::take(&mut input.use_pressed);
        let fire_pressed = std::mem::take(&mut input.fire_pressed);
        if local && restart_requested(*state, age.0, use_pressed, fire_pressed) {
            // A completed level of an episode moves on; anything else restarts the level.
            if *state == PlayState::Complete && episode.is_some() {
                advance.0 = true;
            } else {
                request.0 = true;
            }
        }
    }
}

/// Swaps in another level and starts it (the stats and loadout systems see a fresh spawn).
pub fn load_level(world: &mut World, map: Map) {
    world.insert_resource(LevelSource(map));
    restart_level(world);
}

/// Moves the episode on after a completed level.
pub(crate) fn advance_level(world: &mut World) {
    world.resource_mut::<AdvanceRequested>().0 = false;
    crate::episode::advance(world);
}

/// The authored [`LevelSource`] as played on the current [`LevelDifficulty`].
pub fn play_map(world: &World) -> Map {
    let difficulty = world
        .get_resource::<LevelDifficulty>()
        .copied()
        .unwrap_or_default();
    world
        .resource::<LevelSource>()
        .0
        .for_difficulty(difficulty.0)
}

/// Rebuilds the level from [`LevelSource`]: despawns every [`LevelEntity`], resets the map,
/// mechanics and per-level queues, then runs [`SpawnLevel`].
pub fn restart_level(world: &mut World) {
    let stale: Vec<Entity> = world
        .query_filtered::<Entity, With<LevelEntity>>()
        .iter(world)
        .collect();
    for e in stale {
        // A level entity parented to another one is gone with its parent already.
        if world.get_entity(e).is_ok() {
            world.despawn(e);
        }
    }
    let (map, mech) = fresh_level(play_map(world));
    world.insert_resource(map);
    world.insert_resource(mech);
    world.insert_resource(DirtySectors::default());
    world.insert_resource(FxQueue::default());
    world.insert_resource(HudMessage::default());
    world.insert_resource(HudSubtitle::default());
    // The decal entities went with the level; forget them in the ring too.
    if let Some(mut ring) = world.get_resource_mut::<crate::decals::DecalRing>() {
        ring.reset();
    }
    // Reset in place: the flash exists only with the HUD, and resources are entities.
    if let Some(mut flash) = world.get_resource_mut::<crate::hud::DamageFlash>() {
        flash.0 = 0.0;
    }
    if let Some(mut color) = world.get_resource_mut::<crate::hud::FlashColor>() {
        *color = default();
    }
    // Likewise in place: the viewmodel and camera state carry over from the dead run otherwise
    // (the gun swings in from the death-time look, the view rolls back upright over ~0.2 s).
    if let Some(mut view) = world.get_resource_mut::<crate::viewmodel::ViewState>() {
        *view = default();
    }
    if let Some(mut roll) = world.get_resource_mut::<crate::player::ViewRoll>() {
        roll.0 = 0.0;
    }
    // Camera shake and night vision belong to the dead run too. (Vitals, armour and the carried
    // inventory live on the player entity, so the loadout in `SpawnLevel` renews them; the
    // player's input latches are reborn empty with it, so a key pressed while dead cannot fire.)
    if let Some(mut shake) = world.get_resource_mut::<crate::fx::ScreenShake>() {
        shake.reset();
    }
    crate::inventory::reset_night_vision(world);
    world.insert_resource(UsePrompt::default());
    *world.resource_mut::<PlayState>() = PlayState::Playing;
    world.insert_resource(PausedFrom::default());
    world.resource_mut::<StateAge>().0 = 0.0;
    world.resource_mut::<RestartRequested>().0 = false;
    world.resource_mut::<LevelRuns>().0 += 1;
    world.run_schedule(SpawnLevel);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dead_or_complete_restart() {
        for s in [
            PlayState::Menu,
            PlayState::Playing,
            PlayState::Paused,
            PlayState::EpisodeEnd,
        ] {
            assert!(!restart_requested(s, 5.0, true, true), "{s:?}");
        }
        assert!(restart_requested(PlayState::Dead, 5.0, false, true));
        assert!(restart_requested(PlayState::Complete, 5.0, true, false));
    }
}
