//! The M5b engine features end to end in the headless sim: triggers, quakes, hazards, death
//! actions, props and the automap.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::audio::COOLDOWN;
use rr_core::automap::Automap;
use rr_core::collide::Body;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, door_rooms, engine_room};
use rr_core::map::{ActorKind, ActorSpawn, Map, SwitchAction};
use rr_core::mechanics::{Motion, UseTarget};
use rr_core::projectile::Shooter;
use rr_game::audio::{AudioFxPlugin, QuipState, QuipVoice};
use rr_game::automap::{AutomapSimPlugin, AutomapView, LevelAutomap, REVEAL_EVERY};
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, PlayerVitals, insert_defs};
use rr_game::episode::{EpisodePlugin, Stats};
use rr_game::flow::{FlowPlugin, PlayState, load_level};
use rr_game::fx::{FxPlugin, ScreenShake};
use rr_game::level::CurrentMap;
use rr_game::mechanics::{
    HudMessage, HudSubtitle, LevelMechanics, MechanicsSimPlugin, UsePrompt, insert_level,
};
use rr_game::paths::assets_dir;
use rr_game::player::{EYE_BELOW_TOP, Look, PendingInput, PlayerBody, PlayerSimPlugin, PrevFeet};
use std::time::Duration;

fn app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    insert_level(&mut app, map, Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        FxPlugin,
        AutomapSimPlugin,
    ));
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn input(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app.world_mut().query::<&mut PendingInput>();
    q.single_mut(app.world_mut()).unwrap()
}

fn vitals(app: &mut App) -> Mut<'_, PlayerVitals> {
    let mut q = app.world_mut().query::<&mut PlayerVitals>();
    q.single_mut(app.world_mut()).unwrap()
}

fn body(app: &mut App) -> Body {
    let mut q = app.world_mut().query::<&PlayerBody>();
    q.single(app.world()).unwrap().0
}

fn state(app: &App) -> PlayState {
    *app.world().resource::<PlayState>()
}

/// Puts the player at `(x, y)` on the floor, facing `angle_deg`.
fn teleport(app: &mut App, x: f32, y: f32, angle_deg: f32) {
    let map = app.world().resource::<CurrentMap>().0.clone();
    let mut q = app
        .world_mut()
        .query::<(&mut PlayerBody, &mut Look, &mut PrevFeet)>();
    let (mut b, mut look, mut prev) = q.single_mut(app.world_mut()).unwrap();
    b.0 = Body::spawn(&map, Vec2::new(x, y), b.0.radius, b.0.height).unwrap();
    prev.0 = b.0.pos;
    look.angle = angle_deg.to_radians();
    look.pitch = 0.0;
}

fn tough(app: &mut App) {
    let mut v = vitals(app);
    v.0.health.max = 100_000;
    v.0.health.hp = 100_000;
}

/// Fires at actor `actor` one tick at a time until an event matching `want` appears.
fn shoot_until(app: &mut App, actor: usize, want: impl Fn(&CombatEvent) -> bool) {
    input(app).fire = true;
    for _ in 0..600 {
        let target = {
            let b = app.world().resource::<LevelCombat>().0.actors[actor].body;
            b.pos + Vec3::Z * 0.6 * b.height
        };
        let mut q = app.world_mut().query::<(&PlayerBody, &mut Look)>();
        let (b, mut look) = q.single_mut(app.world_mut()).unwrap();
        let d = target - (b.0.pos + Vec3::Z * (b.0.height - EYE_BELOW_TOP));
        look.angle = d.y.atan2(d.x);
        look.pitch = d.z.atan2(d.truncate().length());
        ticks(app, 1);
        let hit = app.world().resource::<FxQueue>().combat.iter().any(&want);
        app.world_mut().resource_mut::<FxQueue>().combat.clear();
        if hit {
            input(app).fire = false;
            return;
        }
    }
    panic!("never saw the event");
}

#[test]
fn walking_onto_the_trigger_drops_the_floor_and_shakes() {
    let mut app = app(engine_room(""));
    teleport(&mut app, 5.5, 2.0, 0.0);
    input(&mut app).forward = 1.0;
    for _ in 0..60 {
        ticks(&mut app, 1);
        if body(&mut app).sector == 1 {
            break;
        }
    }
    input(&mut app).forward = 0.0;
    assert_eq!(
        app.world().resource::<LevelMechanics>().0.movers[0].motion,
        Motion::ToEnd
    );
    app.update();
    assert!(app.world().resource::<ScreenShake>().trauma >= 0.7);
    ticks(&mut app, 200);
    assert_eq!(
        app.world().resource::<CurrentMap>().0.sectors[2].floor_z,
        -2.0
    );
}

#[test]
fn slime_lowers_health() {
    let mut app = app(engine_room(""));
    teleport(&mut app, 11.0, 2.0, 0.0);
    ticks(&mut app, 60);
    assert_eq!(vitals(&mut app).0.health.hp, 92);
}

#[test]
fn boss_enters_phase_two_and_its_death_ends_the_level() {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Boss,
        pos: Vec2::new(6.0, 6.0),
        angle: std::f32::consts::PI,
        asleep: false,
        skill: Difficulty::Easy,
        on_death: Some(SwitchAction::Exit),
    });
    let mut app = app(map);
    tough(&mut app);
    // North of the pillar, so the line of fire to the boss is clear.
    teleport(&mut app, 1.5, 6.5, 0.0);
    app.world_mut().resource_mut::<LevelCombat>().0.actors[0]
        .health
        .hp = 301;
    shoot_until(&mut app, 0, |e| {
        *e == CombatEvent::PhaseChanged { actor: 0, phase: 1 }
    });
    app.world_mut().resource_mut::<LevelCombat>().0.actors[0]
        .health
        .hp = 1;
    shoot_until(&mut app, 0, |e| {
        *e == CombatEvent::ActorKilled {
            actor: 0,
            by: Shooter::Player,
        }
    });
    assert_eq!(state(&app), PlayState::Playing, "not at the kill");
    ticks(&mut app, 180);
    assert_eq!(state(&app), PlayState::Complete);
    assert_eq!(app.world().resource::<HudMessage>().text, "Level complete!");
}

/// An enemy killed by another enemy still counts in the level stats, but earns no kill quip.
#[test]
fn an_infight_kill_counts_in_the_stats_but_is_not_the_players() {
    let mut map = combat_room();
    for x in [6.0, 7.0] {
        map.actors.push(ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(x, 6.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
        });
    }
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        },
    ))
    .init_asset::<AudioSource>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    insert_level(&mut app, map, Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        EpisodePlugin,
        AudioFxPlugin::default(),
    ));
    app.update();
    assert_eq!(app.world().resource::<Stats>().0.kills, 0);
    let voices = |app: &mut App| {
        let mut q = app.world_mut().query_filtered::<Entity, With<QuipVoice>>();
        q.iter(app.world()).collect::<Vec<_>>()
    };
    let subtitle = |app: &App| app.world().resource::<HudSubtitle>().text.clone();
    let burst = |app: &mut App, by: Shooter| {
        // Past the quip cooldown, so only the kill's owner decides whether a line plays.
        app.world_mut().resource_mut::<QuipState>().clock += COOLDOWN + 1.0;
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<FxQueue>()
                .combat
                .push(CombatEvent::ActorKilled { actor: 0, by });
        }
        app.update();
    };

    // Control: the player's own kills do earn a (multi-kill) line.
    let opening = (voices(&mut app), subtitle(&app));
    burst(&mut app, Shooter::Player);
    assert_ne!(
        (voices(&mut app), subtitle(&app)),
        opening,
        "player kills quip"
    );
    assert_eq!(app.world().resource::<Stats>().0.kills, 3);

    // Infight kills are counted but silent.
    let after_player = (voices(&mut app), subtitle(&app));
    burst(&mut app, Shooter::Actor(1));
    assert_eq!(
        app.world().resource::<Stats>().0.kills,
        6,
        "stats count every death"
    );
    assert_eq!(
        (voices(&mut app), subtitle(&app)),
        after_player,
        "no kill quip"
    );
}
#[test]
fn the_toilet_heals_and_says_so() {
    let mut app = app(engine_room(""));
    teleport(&mut app, 14.0, 2.5, 90.0);
    vitals(&mut app).0.damage(30);
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<UsePrompt>().0,
        Some(UseTarget::Prop(0))
    );
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(vitals(&mut app).0.health.hp, 80);
    assert_eq!(
        app.world().resource::<HudMessage>().text,
        "+10 health. Much better."
    );
}

/// Mechanics freeze while paused, so a running quake must not keep the screen shaking.
#[test]
fn a_paused_quake_does_not_shake() {
    let mut app = app(engine_room(""));
    app.world_mut()
        .resource_mut::<LevelMechanics>()
        .0
        .apply(SwitchAction::Channel(7));
    app.update();
    assert!(
        app.world().resource::<ScreenShake>().trauma > 0.0,
        "control: playing shakes"
    );
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.world_mut().resource_mut::<ScreenShake>().trauma = 0.0;
    app.update();
    assert_eq!(app.world().resource::<ScreenShake>().trauma, 0.0);
}

fn map_open(app: &App) -> bool {
    app.world().resource::<AutomapView>().open
}

/// Walls of room B (behind the door of `door_rooms`) the automap has revealed.
fn room_b_seen(app: &App) -> usize {
    let map = &app.world().resource::<CurrentMap>().0;
    let am = &app.world().resource::<LevelAutomap>().0;
    map.sectors[2].walls().filter(|&w| am.seen[w]).count()
}

fn kill_and_restart(app: &mut App) {
    vitals(app).0.damage(1_000_000);
    ticks(app, 1);
    assert_eq!(state(app), PlayState::Dead);
    ticks(app, 70);
    input(app).use_pressed = true;
    ticks(app, 1);
    assert_eq!(state(app), PlayState::Playing);
}

#[test]
fn the_automap_key_toggles_only_while_playing() {
    // A press while paused is dropped, not applied on resume.
    let mut app = app(door_rooms("(kind: Door)", ""));
    input(&mut app).toggle_map = true;
    ticks(&mut app, 1);
    assert!(map_open(&app));
    input(&mut app).toggle_map = true;
    ticks(&mut app, 1);
    assert!(!map_open(&app));
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    input(&mut app).toggle_map = true;
    ticks(&mut app, 1);
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Playing;
    ticks(&mut app, 1);
    assert!(!map_open(&app));
}

#[test]
fn restart_forgets_the_explored_map_and_closes_it() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    ticks(&mut app, 6);
    assert_eq!(room_b_seen(&app), 0, "behind the closed door");
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    ticks(&mut app, 60);
    assert!(room_b_seen(&app) > 0);
    input(&mut app).toggle_map = true;
    ticks(&mut app, 1);
    kill_and_restart(&mut app);
    assert!(!map_open(&app));
    assert_eq!(room_b_seen(&app), 0);
}

#[test]
fn a_level_switch_sizes_the_automap_for_the_new_map() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    load_level(app.world_mut(), engine_room(""));
    ticks(&mut app, 6);
    let walls = app.world().resource::<CurrentMap>().0.walls.len();
    assert_eq!(app.world().resource::<LevelAutomap>().0.seen.len(), walls);
}

#[test]
fn reveal_runs_every_six_ticks() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    ticks(&mut app, 60);
    let forget = |app: &mut App| {
        let am = Automap::new(&app.world().resource::<CurrentMap>().0);
        app.insert_resource(LevelAutomap(am));
    };
    let ticks_to_reveal = |app: &mut App| {
        (1..=100)
            .find(|_| {
                ticks(app, 1);
                room_b_seen(app) > 0
            })
            .unwrap()
    };
    forget(&mut app);
    assert!(ticks_to_reveal(&mut app) <= REVEAL_EVERY as usize);
    // Now in phase: a forgotten map comes back exactly one reveal period later.
    forget(&mut app);
    assert_eq!(ticks_to_reveal(&mut app), REVEAL_EVERY as usize);
}
