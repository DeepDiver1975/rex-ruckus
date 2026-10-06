//! The M5b engine features end to end in the headless sim: triggers, quakes, hazards, death
//! actions (and, from later tasks, props and the automap).

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::collide::Body;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, engine_room};
use rr_core::map::{ActorKind, ActorSpawn, Map, SwitchAction};
use rr_core::mechanics::{Motion, UseTarget};
use rr_core::projectile::Shooter;
use rr_game::audio::{AudioFxPlugin, QuipVoice};
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, PlayerVitals, insert_defs};
use rr_game::episode::{EpisodePlugin, Stats};
use rr_game::flow::{FlowPlugin, PlayState};
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
    // Let the level-start line finish its cooldown so a kill quip would be allowed.
    let mut q = app.world_mut().query::<(Entity, &QuipVoice)>();
    let opening: Vec<_> = q.iter(app.world()).map(|(e, _)| e).collect();
    let subtitle = |app: &App| app.world().resource::<HudSubtitle>().text.clone();
    let before = subtitle(&app);
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::ActorKilled {
            actor: 0,
            by: Shooter::Actor(1),
        });
    app.update();
    assert_eq!(
        app.world().resource::<Stats>().0.kills,
        1,
        "stats count every death"
    );
    let mut q = app.world_mut().query::<(Entity, &QuipVoice)>();
    let after: Vec<_> = q.iter(app.world()).map(|(e, _)| e).collect();
    assert_eq!(after, opening, "no kill quip");
    assert_eq!(subtitle(&app), before);
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
