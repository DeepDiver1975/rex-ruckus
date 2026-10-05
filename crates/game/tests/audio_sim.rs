//! Sound effects without an audio device: the voices are plain `AudioPlayer` entities, so the
//! tests assert on what `play_cues` spawns and despawns. Bevy's `AudioPlugin` is not added.

use bevy::prelude::*;
use rr_core::audio::Cue;
use rr_core::combat::CombatEvent;
use rr_core::defs::{Defs, WeaponId};
use rr_core::fixtures::door_rooms;
use rr_core::map::MoverKind;
use rr_core::mechanics::MechEvent;
use rr_core::weapons::WeaponEvent;
use rr_game::audio::{AudioFxPlugin, GameCues, JetpackHum, MoverLoop, SfxVoice, SoundBank};
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, insert_defs};
use rr_game::flow::FlowPlugin;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::paths::assets_dir;
use rr_game::player::PlayerSimPlugin;

/// The door level with one Grunt in room B.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        },
    ))
    .init_asset::<AudioSource>();
    insert_level(
        &mut app,
        door_rooms(
            "(kind: Door)",
            "actors: [(kind: Grunt, pos: (7.0, 2.0), angle_deg: 180.0)],",
        ),
    );
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        AudioFxPlugin,
    ));
    app.update();
    assert!(app.world().contains_resource::<SoundBank>());
    app
}

fn fx(app: &mut App) -> Mut<'_, FxQueue> {
    app.world_mut().resource_mut::<FxQueue>()
}

fn voices(app: &mut App, cue: Cue) -> usize {
    let mut q = app
        .world_mut()
        .query_filtered::<&SfxVoice, With<AudioPlayer>>();
    q.iter(app.world()).filter(|v| v.0 == cue).count()
}

fn count<C: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<C>>();
    q.iter(app.world()).count()
}

fn fire(weapon: WeaponId) -> WeaponEvent {
    WeaponEvent::Fire {
        weapon,
        dirs: vec![Vec3::X],
    }
}

#[test]
fn a_shot_plays_one_voice() {
    let mut app = app();
    assert_eq!(count::<SfxVoice>(&mut app), 0);
    fx(&mut app).weapon.push(fire(WeaponId::Pistol));
    app.update();
    assert_eq!(voices(&mut app, Cue::Fire(WeaponId::Pistol)), 1);
    assert_eq!(count::<SfxVoice>(&mut app), 1);
    // The queue was cleared; nothing more plays.
    app.update();
    assert_eq!(count::<SfxVoice>(&mut app), 1);
}

#[test]
fn a_burst_is_capped_at_max_voices() {
    let mut app = app();
    for _ in 0..10 {
        fx(&mut app).weapon.push(fire(WeaponId::Chaingun));
    }
    app.update();
    let cap = app
        .world()
        .resource::<SoundBank>()
        .def(Cue::Fire(WeaponId::Chaingun))
        .max_voices;
    let n = voices(&mut app, Cue::Fire(WeaponId::Chaingun));
    assert!(n >= 1 && n <= cap as usize, "{n} voices, cap {cap}");
    // Still playing next frame (no audio device to end them): the cap holds across frames.
    fx(&mut app).weapon.push(fire(WeaponId::Chaingun));
    app.update();
    assert_eq!(voices(&mut app, Cue::Fire(WeaponId::Chaingun)), n);
}

#[test]
fn a_door_hums_while_it_moves() {
    let mut app = app();
    let door = MechEvent::MoverStarted {
        sector: 1,
        kind: MoverKind::Door,
    };
    fx(&mut app).mech.push(door);
    app.update();
    assert_eq!(count::<MoverLoop>(&mut app), 1);
    assert_eq!(voices(&mut app, Cue::DoorStart), 1);
    // A second start for the same sector does not stack.
    fx(&mut app).mech.push(door);
    app.update();
    assert_eq!(count::<MoverLoop>(&mut app), 1);

    fx(&mut app).mech.push(MechEvent::MoverStopped {
        sector: 1,
        kind: MoverKind::Door,
    });
    app.update();
    assert_eq!(count::<MoverLoop>(&mut app), 0);
    assert_eq!(voices(&mut app, Cue::DoorStop), 1);
}

#[test]
fn a_far_actor_is_silent() {
    let mut app = app();
    app.world_mut().resource_mut::<LevelCombat>().0.actors[0]
        .body
        .pos = Vec3::new(500.0, 2.0, 0.0);
    fx(&mut app).combat.push(CombatEvent::ActorWoke(0));
    app.update();
    assert_eq!(count::<SfxVoice>(&mut app), 0);

    // The same Grunt in room B is heard.
    app.world_mut().resource_mut::<LevelCombat>().0.actors[0]
        .body
        .pos = Vec3::new(7.0, 2.0, 0.0);
    fx(&mut app).combat.push(CombatEvent::ActorWoke(0));
    app.update();
    assert_eq!(
        voices(&mut app, Cue::ActorWake(rr_core::map::ActorKind::Grunt)),
        1
    );
}

#[test]
fn the_jetpack_burns_between_start_and_stop() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<GameCues>()
        .0
        .push((Cue::JetpackStart, None));
    app.update();
    assert_eq!(count::<JetpackHum>(&mut app), 1);
    assert_eq!(voices(&mut app, Cue::JetpackStart), 1);
    app.world_mut()
        .resource_mut::<GameCues>()
        .0
        .push((Cue::JetpackStop, None));
    app.update();
    assert_eq!(count::<JetpackHum>(&mut app), 0);
    assert_eq!(voices(&mut app, Cue::JetpackStop), 1);
}

#[test]
fn walking_raises_footsteps() {
    let mut app = app();
    {
        let mut q = app
            .world_mut()
            .query::<&mut rr_game::player::PendingInput>();
        q.single_mut(app.world_mut()).unwrap().forward = 1.0;
    }
    // About a second of walking west to east across room A.
    for _ in 0..60 {
        app.world_mut().run_schedule(FixedUpdate);
    }
    let steps = app
        .world()
        .resource::<GameCues>()
        .0
        .iter()
        .filter(|(c, _)| *c == Cue::Footstep)
        .count();
    assert!(steps >= 1, "no footsteps after walking");
    app.update();
    assert!(app.world().resource::<GameCues>().0.is_empty());
    assert!(voices(&mut app, Cue::Footstep) >= 1);
}

#[test]
fn a_restart_silences_the_loops() {
    let mut app = app();
    fx(&mut app).mech.push(MechEvent::MoverStarted {
        sector: 1,
        kind: MoverKind::Door,
    });
    app.world_mut()
        .resource_mut::<GameCues>()
        .0
        .push((Cue::JetpackStart, None));
    app.update();
    assert_eq!(count::<MoverLoop>(&mut app), 1);
    assert_eq!(count::<JetpackHum>(&mut app), 1);
    rr_game::flow::restart_level(app.world_mut());
    assert_eq!(count::<MoverLoop>(&mut app), 0);
    assert_eq!(count::<JetpackHum>(&mut app), 0);
}
