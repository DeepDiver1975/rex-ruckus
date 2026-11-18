//! Co-op rules on the game side: what each player enters a level with, getting back up after
//! being downed, and the team's shared keycards.

use crate::combat::CombatSet;
use crate::combat::{PlayerArsenal, PlayerInventory, PlayerVitals, spawn_combat};
use crate::episode::Episode;
use crate::flow::{SpawnLevel, check_player_death, sim_ticks, world_running};
use crate::level::CurrentMap;
use crate::mechanics::pickup_items;
use crate::net::host::{host_send, spawn_seated_players};
use crate::player::{
    Inventory, Look, PLAYER_RADIUS, PendingInput, Player, PlayerBody, PlayerSimSet, PlayerTuning,
    PrevFeet, spawn_dev_partners, spawn_spot,
};
use bevy::prelude::*;
use rr_core::carry::CarryOver;
use rr_core::collide::Body;
use rr_core::map::KeySet;

/// The loadout this player entered the level with (or joined with): what a respawn and a level
/// restart give back, and what a downed player carries on.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct EntryLoadout(pub CarryOver);

/// Co-op rules; added with the other sim plugins (headless test apps add it themselves).
pub struct CoopPlugin;

impl Plugin for CoopPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SpawnLevel,
            apply_entry_loadouts
                .after(spawn_combat)
                .after(spawn_seated_players)
                .after(spawn_dev_partners),
        )
        .add_systems(
            FixedUpdate,
            (
                tick_down_time
                    .run_if(world_running)
                    .after(PlayerSimSet)
                    .after(CombatSet),
                respawn_players
                    .run_if(sim_ticks)
                    .after(tick_down_time)
                    .before(check_player_death),
                share_keys
                    .run_if(sim_ticks)
                    .after(pickup_items)
                    .before(host_send),
            ),
        );
    }
}

/// Gives every player the loadout the episode carried for them (when there is one) and records
/// whatever loadout they now have as their [`EntryLoadout`].
pub fn apply_entry_loadouts(
    mut commands: Commands,
    episode: Option<Res<Episode>>,
    players: Query<(
        Entity,
        &Player,
        &PlayerVitals,
        &PlayerArsenal,
        &PlayerInventory,
    )>,
) {
    for (e, p, v, a, i) in &players {
        let mut entity = commands.entity(e);
        let carried = episode
            .as_ref()
            .and_then(|ep| ep.entries.get(p.id.0 as usize))
            .and_then(Option::as_ref);
        let entry = match carried {
            Some(c) => {
                entity.insert((
                    PlayerVitals(c.vitals),
                    PlayerArsenal(c.arsenal.clone()),
                    PlayerInventory(c.inventory),
                ));
                c.clone()
            }
            None => CarryOver::take(&v.0, &a.0, &i.0),
        };
        entity.insert(EntryLoadout(entry));
    }
}

/// Seconds a downed player waits before getting back up.
pub const RESPAWN_SECS: f32 = 3.0;

/// Seconds this player has been dead (0 while alive).
#[derive(Component, Default, Debug)]
pub struct DownTime(pub f32);

/// Counts the dead up and resets the living; runs on clients too (on the vitals the snapshots
/// bring) so their HUD can count down.
pub fn tick_down_time(time: Res<Time<Fixed>>, mut q: Query<(&PlayerVitals, &mut DownTime)>) {
    let dt = time.timestep().as_secs_f32();
    for (v, mut down) in &mut q {
        if v.0.health.alive() {
            if down.0 != 0.0 {
                down.0 = 0.0;
            }
        } else {
            down.0 += dt;
        }
    }
}

/// What a respawn reads and resets.
type RespawnQuery = (
    &'static Player,
    &'static mut DownTime,
    &'static EntryLoadout,
    &'static mut PlayerBody,
    &'static mut PrevFeet,
    &'static mut Look,
    &'static mut PendingInput,
    &'static mut PlayerVitals,
    &'static mut PlayerArsenal,
    &'static mut PlayerInventory,
);

/// Gets every player who has been down for [`RESPAWN_SECS`] back up while a partner still lives.
pub fn respawn_players(
    map: Res<CurrentMap>,
    tuning: Res<PlayerTuning>,
    mut q: Query<RespawnQuery>,
) {
    if !q.iter().any(|t| t.7.0.health.alive()) {
        return;
    }
    for (p, mut down, entry, mut body, mut prev, mut look, mut input, mut v, mut a, mut i) in &mut q
    {
        if down.0 < RESPAWN_SECS {
            continue;
        }
        let xy = spawn_spot(&map.0, &tuning.0, p.id);
        let Some(spawned) = Body::spawn(&map.0, xy, PLAYER_RADIUS, tuning.0.stand_height) else {
            continue;
        };
        body.0 = spawned;
        prev.0 = spawned.pos;
        look.angle = map.0.player_start.angle_deg.to_radians();
        look.pitch = 0.0;
        *input = PendingInput::default();
        v.0 = entry.0.vitals;
        a.0 = entry.0.arsenal.clone();
        i.0 = entry.0.inventory;
        down.0 = 0.0;
    }
}

/// The team shares its keycards: everyone holds every key anyone found.
pub fn share_keys(mut q: Query<&mut Inventory, With<Player>>) {
    let all = q
        .iter()
        .fold(KeySet::default(), |acc, inv| acc.union(inv.keys));
    for mut inv in &mut q {
        if inv.keys != all {
            inv.keys = all;
        }
    }
}
