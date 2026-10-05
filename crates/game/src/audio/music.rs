//! Level music: the map's track loops from the level spawn on and starts over on a restart.

use super::{AudioOptions, AudioVolumes};
use crate::level::CurrentMap;
use bevy::audio::Volume;
use bevy::prelude::*;

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
    old: Query<Entity, With<MusicTrack>>,
) {
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
