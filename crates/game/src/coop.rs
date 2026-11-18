//! Co-op rules on the game side: what each player enters a level with.

use crate::combat::{PlayerArsenal, PlayerInventory, PlayerVitals, spawn_combat};
use crate::episode::Episode;
use crate::flow::SpawnLevel;
use crate::net::host::spawn_seated_players;
use crate::player::{Player, spawn_dev_partners};
use bevy::prelude::*;
use rr_core::carry::CarryOver;

/// The loadout this player entered the level with (or joined with): what a respawn and a level
/// restart give back, and what a downed player carries on.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct EntryLoadout(pub CarryOver);

/// Co-op rules; added by [`crate::episode::EpisodePlugin`].
pub struct CoopPlugin;

impl Plugin for CoopPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SpawnLevel,
            apply_entry_loadouts
                .after(spawn_combat)
                .after(spawn_seated_players)
                .after(spawn_dev_partners),
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
