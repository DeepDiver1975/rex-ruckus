//! The one playback path: this frame's events and game cues become cues, the voice caps
//! decide, and each admitted cue spawns an `AudioPlayer` entity.

use super::{
    GameCues, JetpackHum, MAX_LIVE_VOICES, MoverLoop, SfxRng, SfxVoice, SoundBank, spatial_scale,
    voice_volume,
};
use crate::combat::{FxQueue, LevelCombat, eye_of};
use crate::coords::to_bevy;
use crate::flow::LevelEntity;
use crate::level::CurrentMap;
use crate::player::{Player, PlayerBody};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rr_core::audio::{Cue, CuePos, combat_cues, gain, mech_cues, weapon_cues};
use rr_core::map::{MoverKind, SectorId};
use rr_core::mechanics::MechEvent;
use std::collections::HashMap;

/// Where this frame's cues come from.
#[derive(SystemParam)]
pub struct CueSources<'w> {
    fx: Res<'w, FxQueue>,
    game: ResMut<'w, GameCues>,
    combat: Option<Res<'w, LevelCombat>>,
    map: Res<'w, CurrentMap>,
}

/// The sounds already playing.
#[derive(SystemParam)]
pub struct LiveSounds<'w, 's> {
    voices: Query<'w, 's, &'static SfxVoice>,
    movers: Query<'w, 's, (Entity, &'static MoverLoop)>,
    hums: Query<'w, 's, Entity, With<JetpackHum>>,
}

/// Spawns voices within the caps. Counts include this frame's spawns.
struct Mixer<'a, 'w, 's> {
    commands: Commands<'w, 's>,
    bank: &'a SoundBank,
    rng: &'a mut SfxRng,
    /// The listener (player eye), core coordinates.
    ear: Vec3,
    per_cue: HashMap<Cue, usize>,
    total: usize,
}

impl Mixer<'_, '_, '_> {
    /// Plays `cue` at `pos` (`None`: at the listener) if the caps allow and, for a one-shot,
    /// if it is audible at all. Loops start regardless of distance (the player may walk up to
    /// them; `loop_gain` follows) and belong to the level run.
    fn play(&mut self, cue: Cue, pos: CuePos) -> Option<Entity> {
        let def = self.bank.def(cue);
        let at = pos.unwrap_or(self.ear);
        let dist = at.distance(self.ear);
        if !def.looped && gain(def, dist) <= 0.0 {
            return None;
        }
        let n = self.per_cue.entry(cue).or_default();
        if *n >= def.max_voices as usize || self.total >= MAX_LIVE_VOICES {
            return None;
        }
        *n += 1;
        self.total += 1;

        let rng = &mut self.rng.0;
        let file = &def.files[(rng.next_u64() % def.files.len() as u64) as usize];
        let speed = 1.0 + def.pitch_jitter * rng.signed();
        let mode = if def.looped {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        };
        let settings = mode
            .with_volume(voice_volume(def, dist))
            .with_speed(speed)
            .with_spatial(def.spatial)
            .with_spatial_scale(spatial_scale(def));
        let mut e = self.commands.spawn((
            AudioPlayer(self.bank.handle(file)),
            settings,
            Transform::from_translation(to_bevy(at)),
            SfxVoice(cue),
        ));
        if def.looped {
            e.insert(LevelEntity);
        }
        Some(e.id())
    }

    fn stop(&mut self, e: Entity) {
        self.commands.entity(e).try_despawn();
    }
}

/// Plays this frame's cues: combat, weapon and mechanics events through the core mapping, then
/// the game's own [`GameCues`] (drained). Moving doors and lifts get a [`MoverLoop`] per
/// sector and the jetpack a [`JetpackHum`], each stopped by its matching stop cue.
pub fn play_cues(
    commands: Commands,
    bank: Res<SoundBank>,
    mut rng: ResMut<SfxRng>,
    mut src: CueSources,
    live: LiveSounds,
    player: Query<&PlayerBody, With<Player>>,
) {
    let game = std::mem::take(&mut src.game.0);
    let Ok(body) = player.single() else { return };
    let map = &src.map.0;

    let mut per_cue: HashMap<Cue, usize> = HashMap::new();
    for v in &live.voices {
        *per_cue.entry(v.0).or_default() += 1;
    }
    let mut mix = Mixer {
        commands,
        bank: &bank,
        rng: &mut rng,
        ear: eye_of(&body.0),
        total: live.voices.iter().len(),
        per_cue,
    };

    // Mover loops, keyed by sector, straight from the events (the cues only carry a position).
    let mut movers: HashMap<SectorId, Vec<Entity>> = HashMap::new();
    for (e, l) in &live.movers {
        movers.entry(l.0).or_default().push(e);
    }
    for ev in &src.fx.mech {
        match *ev {
            MechEvent::MoverStarted { sector, kind } => {
                let cue = match kind {
                    MoverKind::Door => Cue::DoorStart,
                    MoverKind::Lift { .. } => Cue::LiftStart,
                    MoverKind::Crack => continue,
                };
                if sector >= map.sectors.len() || movers.contains_key(&sector) {
                    continue;
                }
                if let Some(e) = mix.play(cue, Some(map.sector_centre(sector))) {
                    mix.commands.entity(e).insert(MoverLoop(sector));
                    movers.insert(sector, vec![e]);
                }
            }
            MechEvent::MoverStopped { sector, .. } => {
                for e in movers.remove(&sector).unwrap_or_default() {
                    mix.stop(e);
                }
            }
            _ => {}
        }
    }

    let mut cues = Vec::new();
    if let Some(combat) = &src.combat {
        for ev in &src.fx.combat {
            combat_cues(ev, &combat.0, map, &mut cues);
        }
    }
    for ev in &src.fx.weapon {
        weapon_cues(ev, &mut cues);
    }
    for ev in &src.fx.mech {
        mech_cues(ev, map, &mut cues);
    }
    cues.extend(game);

    let mut hums: Vec<Entity> = live.hums.iter().collect();
    for (cue, pos) in cues {
        // The looped start cues were handled above.
        if bank.def(cue).looped {
            continue;
        }
        match cue {
            Cue::JetpackStart if hums.is_empty() => {
                if let Some(e) = mix.play(Cue::JetpackLoop, None) {
                    mix.commands.entity(e).insert(JetpackHum);
                    hums.push(e);
                }
            }
            Cue::JetpackStop => {
                for e in hums.drain(..) {
                    mix.stop(e);
                }
            }
            _ => {}
        }
        mix.play(cue, pos);
    }
}
