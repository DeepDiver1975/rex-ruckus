//! Sound effects without an audio device: the voices are plain `AudioPlayer` entities, so the
//! tests assert on what `play_cues` spawns and despawns. Bevy's `AudioPlugin` is not added.
//! Quips, music and the volume keys likewise: their entities and resources are checked directly.

use bevy::audio::GlobalVolume;
use bevy::prelude::*;
use rr_core::audio::{COOLDOWN, Cue, QuipOn, QuipTable};
use rr_core::combat::CombatEvent;
use rr_core::defs::{Defs, WeaponId};
use rr_core::fixtures::door_rooms;
use rr_core::map::{ItemKind, Key, Map, MoverKind};
use rr_core::mechanics::MechEvent;
use rr_core::weapons::WeaponEvent;
use rr_game::audio::{
    AudioFxPlugin, AudioOptions, AudioVolumes, GameCues, JetpackHum, MoverLoop, MusicTrack,
    QuipState, QuipVoice, SfxVoice, SoundBank,
};
use rr_game::combat::PlayerArsenal;
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, insert_defs};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::mechanics::{HudMessage, HudSubtitle, MechanicsSimPlugin, insert_level};
use rr_game::paths::assets_dir;
use rr_game::player::PendingInput;
use rr_game::player::PlayerSimPlugin;

/// The door level with one Grunt in room B.
fn app() -> App {
    app_with(door_level(), AudioFxPlugin::default())
}

fn door_level() -> Map {
    door_rooms(
        "(kind: Door)",
        "actors: [(kind: Grunt, pos: (7.0, 2.0), angle_deg: 180.0)],",
    )
}

fn app_with(map: Map, audio: AudioFxPlugin) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        },
    ))
    .init_asset::<AudioSource>();
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        audio,
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
    assert_eq!(n, cap as usize);
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

#[test]
fn mover_loops_stop_when_play_ends() {
    for end in [PlayState::Dead, PlayState::Complete] {
        let mut app = app();
        fx(&mut app).mech.push(MechEvent::MoverStarted {
            sector: 1,
            kind: MoverKind::Door,
        });
        app.update();
        assert_eq!(count::<MoverLoop>(&mut app), 1);
        *app.world_mut().resource_mut::<PlayState>() = end;
        app.update();
        assert_eq!(count::<MoverLoop>(&mut app), 0, "{end:?}");
    }
}

#[test]
fn a_distant_mover_still_hums_with_its_cue_slots_used_up() {
    let mut app = app();
    let cap = app
        .world()
        .resource::<SoundBank>()
        .def(Cue::DoorStart)
        .max_voices;
    // Hums of other doors far away hold every DoorStart slot.
    for _ in 0..cap {
        app.world_mut().spawn(SfxVoice(Cue::DoorStart));
    }
    fx(&mut app).mech.push(MechEvent::MoverStarted {
        sector: 1,
        kind: MoverKind::Door,
    });
    app.update();
    assert_eq!(count::<MoverLoop>(&mut app), 1);
}

/// The quip lines filed under `on`, from the shipped table.
fn lines(on: QuipOn) -> Vec<String> {
    let src = std::fs::read_to_string(assets_dir().join("quips/quips.ron")).unwrap();
    QuipTable::parse(&src)
        .unwrap()
        .quips
        .into_iter()
        .filter(|q| q.on == on)
        .map(|q| q.text)
        .collect()
}

fn subtitle(app: &App) -> String {
    app.world().resource::<HudSubtitle>().text.clone()
}

fn message(app: &App) -> String {
    app.world().resource::<HudMessage>().text.clone()
}

fn quip_voice(app: &mut App) -> Vec<Entity> {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, (With<QuipVoice>, With<AudioPlayer>)>();
    q.iter(app.world()).collect()
}

/// Lets the quip cooldown run out without simulating eight seconds.
fn skip_cooldown(app: &mut App) {
    app.world_mut().resource_mut::<QuipState>().clock += COOLDOWN + 1.0;
}

fn kill_burst(app: &mut App) {
    for _ in 0..3 {
        fx(app).combat.push(CombatEvent::ActorKilled(0));
    }
}

#[test]
fn the_level_opens_with_a_start_quip() {
    let mut app = app();
    assert_eq!(quip_voice(&mut app).len(), 1);
    assert!(lines(QuipOn::LevelStart).contains(&subtitle(&app)));
}

#[test]
fn three_kills_at_once_make_a_multi_kill_quip() {
    let mut app = app();
    skip_cooldown(&mut app);
    kill_burst(&mut app);
    app.update();
    let voice = quip_voice(&mut app);
    // The new quip cut the opening line.
    assert_eq!(voice.len(), 1);
    let text = subtitle(&app);
    assert!(lines(QuipOn::MultiKill).contains(&text), "{text:?}");

    // A second burst inside the cooldown says nothing.
    kill_burst(&mut app);
    app.update();
    assert_eq!(quip_voice(&mut app), voice);
    assert_eq!(subtitle(&app), text);
}

#[test]
fn a_restart_respawns_with_a_quip_right_away() {
    let mut app = app();
    skip_cooldown(&mut app);
    kill_burst(&mut app);
    app.update();
    assert!(lines(QuipOn::MultiKill).contains(&subtitle(&app)));
    // Well inside the cooldown of the last quip: the restart still gets its line.
    rr_game::flow::restart_level(app.world_mut());
    app.update();
    assert_eq!(quip_voice(&mut app).len(), 1);
    let text = subtitle(&app);
    assert!(lines(QuipOn::Respawn).contains(&text), "{text:?}");
}

#[test]
fn level_music_plays_once_and_restarts_with_the_level() {
    let mut map = door_level();
    map.music = Some("x".into());
    let mut app = app_with(map, AudioFxPlugin::default());
    assert_eq!(count::<MusicTrack>(&mut app), 1);
    rr_game::flow::restart_level(app.world_mut());
    app.update();
    assert_eq!(count::<MusicTrack>(&mut app), 1);
}

#[test]
fn no_music_when_the_options_skip_it() {
    let mut map = door_level();
    map.music = Some("x".into());
    let audio = AudioFxPlugin {
        options: AudioOptions {
            muted: true,
            music: false,
        },
        ..default()
    };
    let mut app = app_with(map, audio);
    assert_eq!(count::<MusicTrack>(&mut app), 0);
}

#[test]
fn a_muted_start_is_silent_until_m_is_pressed() {
    let audio = AudioFxPlugin {
        options: AudioOptions {
            muted: true,
            music: true,
        },
        ..default()
    };
    let mut app = app_with(door_level(), audio);
    assert_eq!(
        app.world().resource::<GlobalVolume>().volume.to_linear(),
        0.0
    );

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::KeyM);
    app.insert_resource(keys);
    app.update();
    assert!(!app.world().resource::<AudioVolumes>().muted);
    assert_eq!(message(&app), "Sound on");
    assert_eq!(
        app.world().resource::<GlobalVolume>().volume.to_linear(),
        1.0
    );

    // `[` turns the master down a notch.
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.clear();
    keys.press(KeyCode::BracketLeft);
    app.update();
    assert_eq!(message(&app), "Volume 90%");
    let v = app.world().resource::<GlobalVolume>().volume.to_linear();
    assert!((v - 0.9).abs() < 1e-6, "{v}");
}

#[test]
fn volume_steps_while_muted_say_so() {
    let audio = AudioFxPlugin {
        options: AudioOptions {
            muted: true,
            music: true,
        },
        ..default()
    };
    let mut app = app_with(door_level(), audio);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::BracketLeft);
    app.insert_resource(keys);
    app.update();
    assert_eq!(message(&app), "Volume 90% (muted)");
    assert_eq!(
        app.world().resource::<GlobalVolume>().volume.to_linear(),
        0.0
    );
}

fn own(app: &mut App, w: WeaponId) {
    let mut q = app.world_mut().query::<&mut PlayerArsenal>();
    q.single_mut(app.world_mut()).unwrap().0.owned[w.index()] = true;
}

fn shotgun_taken() -> MechEvent {
    MechEvent::ItemTaken {
        item: 0,
        kind: ItemKind::Shotgun,
    }
}

#[test]
fn a_new_weapon_is_quipped_but_not_its_ammo() {
    let mut app = app();
    skip_cooldown(&mut app);
    // The pickup that gives the shotgun: its owned flag turns on.
    own(&mut app, WeaponId::Shotgun);
    fx(&mut app).mech.push(shotgun_taken());
    app.update();
    let voice = quip_voice(&mut app);
    let text = subtitle(&app);
    assert!(lines(QuipOn::NewWeapon).contains(&text), "{text:?}");

    // Another shotgun once owned only gives shells: no quip, even past the cooldown.
    skip_cooldown(&mut app);
    fx(&mut app).mech.push(shotgun_taken());
    app.update();
    assert_eq!(quip_voice(&mut app), voice);
    assert_eq!(subtitle(&app), text);
}

fn pending(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app.world_mut().query::<&mut PendingInput>();
    q.single_mut(app.world_mut()).unwrap()
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

#[test]
fn a_need_key_quip_leaves_the_key_message_readable() {
    let map = door_rooms("(kind: Door, lock: Some(Red))", "");
    let mut app = app_with(map, AudioFxPlugin::default());
    // Walk east up to the locked door (start (2,2) faces east) and press use.
    pending(&mut app).forward = 1.0;
    ticks(&mut app, 40);
    pending(&mut app).forward = 0.0;
    ticks(&mut app, 30);
    pending(&mut app).use_pressed = true;
    app.world_mut().run_schedule(FixedUpdate);
    assert!(
        fx(&mut app)
            .mech
            .iter()
            .any(|e| matches!(e, MechEvent::NeedKey(Key::Red)))
    );
    skip_cooldown(&mut app);
    app.update();
    assert_eq!(message(&app), "You need the red keycard");
    let text = subtitle(&app);
    assert!(lines(QuipOn::NeedKey).contains(&text), "{text:?}");
}
