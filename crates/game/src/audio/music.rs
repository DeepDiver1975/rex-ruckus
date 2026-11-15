//! Level music: the map's track loops from the level spawn on and starts over on a restart. A
//! jukebox stinger ducks it while it plays ([`MusicDuck`]).

use super::{AudioOptions, AudioVolumes};
use crate::combat::FxQueue;
use crate::level::CurrentMap;
use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use bevy::prelude::*;
use rr_core::mechanics::{JUKEBOX_SECS, MechEvent};
use rr_core::props::{PropKind, PropOutcome};

/// The level music's level, as a fraction of the music slider, while a jukebox stinger plays.
pub const MUSIC_DUCK: f32 = 0.25;

/// Seconds of jukebox stinger left: the level music is ducked to [`MUSIC_DUCK`] meanwhile.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct MusicDuck {
    pub left: f32,
}

impl MusicDuck {
    /// The multiplier on the music slider right now.
    pub fn gain(&self) -> f32 {
        if self.left > 0.0 { MUSIC_DUCK } else { 1.0 }
    }

    /// Starts a duck for each jukebox stinger among `events`, then runs it down by `dt` (the
    /// stinger is a one-shot that plays on through a pause, so this uses frame time).
    pub fn advance(&mut self, events: &[MechEvent], dt: f32) {
        self.left = (self.left - dt).max(0.0);
        let stinger = events.iter().any(|e| {
            matches!(
                e,
                MechEvent::PropUsed {
                    kind: PropKind::Jukebox,
                    outcome: PropOutcome::Stinger,
                    ..
                }
            )
        });
        if stinger {
            self.left = JUKEBOX_SECS;
        }
    }
}

/// Ducks the level music while a jukebox stinger plays and brings it back after: the sink is
/// set whenever the duck starts or ends.
pub fn duck_music(
    fx: Res<FxQueue>,
    time: Res<Time>,
    volumes: Res<AudioVolumes>,
    global: Option<Res<GlobalVolume>>,
    mut duck: ResMut<MusicDuck>,
    mut sinks: Query<&mut AudioSink, With<MusicTrack>>,
) {
    let before = duck.gain();
    duck.advance(&fx.mech, time.delta_secs());
    if duck.gain() == before {
        return;
    }
    let global = global.map_or(Volume::Linear(1.0), |g| g.volume);
    for mut sink in &mut sinks {
        sink.set_volume(Volume::Linear(volumes.music * duck.gain()) * global);
    }
}

/// The playing level music (`assets/music/<name>.ogg`). At most one exists.
#[derive(Component, Debug)]
pub struct MusicTrack;

/// Replaces any playing track with the map's own, if it names one and music is on. Not a
/// level entity: it is replaced here, so the track also stops for a level without music.
pub fn start_music(
    mut commands: Commands,
    server: Res<AssetServer>,
    options: Res<AudioOptions>,
    volumes: Res<AudioVolumes>,
    map: Res<CurrentMap>,
    mut duck: ResMut<MusicDuck>,
    old: Query<Entity, With<MusicTrack>>,
) {
    // A new level run starts with the music up.
    *duck = MusicDuck::default();
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(name) = map.0.music.as_ref().filter(|_| options.music) else {
        return;
    };
    commands.spawn((
        AudioPlayer::<AudioSource>(server.load(format!("music/{name}.ogg"))),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(volumes.music)),
        MusicTrack,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stinger() -> MechEvent {
        MechEvent::PropUsed {
            prop: 0,
            kind: PropKind::Jukebox,
            outcome: PropOutcome::Stinger,
        }
    }

    #[test]
    fn a_stinger_ducks_the_music_for_its_length() {
        let mut d = MusicDuck::default();
        assert_eq!(d.gain(), 1.0);
        d.advance(&[stinger()], 0.1);
        assert_eq!((d.left, d.gain()), (JUKEBOX_SECS, MUSIC_DUCK));
        d.advance(&[], JUKEBOX_SECS - 0.5);
        assert_eq!(d.gain(), MUSIC_DUCK);
        d.advance(&[], 0.6);
        assert_eq!(d.gain(), 1.0, "back up once the stinger is over");
        // Other props never duck it.
        d.advance(
            &[MechEvent::PropUsed {
                prop: 1,
                kind: PropKind::Arcade,
                outcome: PropOutcome::Played,
            }],
            0.1,
        );
        assert_eq!(d.gain(), 1.0);
    }
}
