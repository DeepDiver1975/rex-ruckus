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
use rr_core::extrude::extrude_sector;
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
use rr_game::level::{CurrentMap, LevelMaterials, SectorMesh, load_map, rebuild_dirty_sectors};
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

/// Engine lab's platform (sector 21) and its track, the second slime pit (sector 20).
const PLATFORM: usize = 21;
const PIT: usize = 20;
const FAR_LEDGE: usize = 22;

fn mesh_count(app: &mut App, s: usize) -> usize {
    let mut q = app.world_mut().query::<&SectorMesh>();
    q.iter(app.world()).filter(|m| m.0 == s).count()
}

/// Standing on the docked platform, the player presses the landing's switch, rides across the
/// slime pit and steps off onto the far ledge, which nothing else reaches. The renderer
/// rebuilds the moving sectors on the way.
#[test]
fn the_platform_carries_the_player_across_the_pit() {
    let mut app = app(load_map("engine_lab.ron"));
    app.insert_resource(LevelMaterials(vec![Handle::default(); 32], Vec::new()))
        .add_systems(Update, rebuild_dirty_sectors);
    // On the platform's north-east corner, facing the switch by the corridor mouth.
    teleport(&mut app, 10.3, -5.5, 80.0);
    assert_eq!(body(&mut app).sector, PLATFORM);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    let mech = &app.world().resource::<LevelMechanics>().0;
    assert!(mech.switch_on[2], "the landing switch");
    let m = mech.mover_in(PLATFORM).unwrap();
    assert_eq!(mech.movers[m].motion, Motion::ToEnd);

    for _ in 0..60 {
        app.update();
        if matches!(
            app.world().resource::<LevelMechanics>().0.movers[m].motion,
            Motion::AtEnd { .. }
        ) {
            break;
        }
    }
    let b = body(&mut app);
    assert_eq!(b.sector, PLATFORM);
    assert!(
        b.pos.truncate().distance(Vec2::new(10.3, -9.3)) < 0.01,
        "rode 3.8 m south to {}",
        b.pos
    );
    app.update();
    let live = app.world().resource::<CurrentMap>().0.clone();
    for s in [PLATFORM, PIT] {
        assert_eq!(
            mesh_count(&mut app, s),
            extrude_sector(&live, s).unwrap().len(),
            "sector {s} rebuilt at the end pose"
        );
    }

    teleport_look(&mut app, 270.0);
    let b = walk_to(&mut app, FAR_LEDGE, 60);
    assert_eq!(b.sector, FAR_LEDGE, "stepped off at {}", b.pos);
    assert_eq!(b.pos.z, 0.0);
}

/// Walks the player along its heading for up to `n` ticks, until it is in sector `goal`.
fn walk_to(app: &mut App, goal: usize, n: usize) -> Body {
    input(app).forward = 1.0;
    for _ in 0..n {
        ticks(app, 1);
        if body(app).sector == goal {
            break;
        }
    }
    input(app).forward = 0.0;
    body(app)
}

/// The sluice gate (sector 17) hangs across the corridor south of the hall until the switch
/// beside the doorway slides it into its pocket.
#[test]
fn the_sluice_gate_slides_aside_to_open_the_corridor() {
    const LANDING: usize = 18;
    let mut app = app(load_map("engine_lab.ron"));
    teleport(&mut app, 9.0, 1.0, 270.0);
    let b = walk_to(&mut app, LANDING, 120);
    assert_eq!(b.sector, 16, "held in the corridor");
    assert!(
        b.pos.y >= -1.6 + b.radius - 0.01,
        "stopped by the gate at {}",
        b.pos
    );
    // The switch on the hall's south wall, east of the doorway.
    teleport(&mut app, 11.0, 0.8, 270.0);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 120);
    let mech = &app.world().resource::<LevelMechanics>().0;
    let m = mech.mover_in(17).unwrap();
    assert!(matches!(mech.movers[m].motion, Motion::AtEnd { .. }));
    teleport(&mut app, 9.0, 1.0, 270.0);
    assert_eq!(walk_to(&mut app, LANDING, 120).sector, LANDING);
}

/// Turns the player to `angle_deg` where it stands.
fn teleport_look(app: &mut App, angle_deg: f32) {
    let mut q = app.world_mut().query::<&mut Look>();
    q.single_mut(app.world_mut()).unwrap().angle = angle_deg.to_radians();
}

/// The breach in engine_lab: the switch on the gag room's west wall fires channel 10, and half a
/// second later the scripted blast in the sealed lounge goes off and blows the crack wall
/// (sector 23) open. The closed crack shields the player at the switch.
#[test]
fn the_breach_switch_blows_the_crack_wall_open() {
    const BREACH: usize = 23;
    const LOUNGE: usize = 24;
    let mut app = app(load_map("engine_lab.ron"));
    teleport(&mut app, 1.0, 10.0, 180.0);
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<UsePrompt>().0,
        Some(UseTarget::Switch(4))
    );
    input(&mut app).use_pressed = true;
    let mut boom = None;
    let mut opened = false;
    let mut n = 0;
    while n < 120 && boom.is_none() {
        ticks(&mut app, 1);
        n += 1;
        let fx = app.world().resource::<FxQueue>();
        boom = boom.or(fx.combat.iter().find_map(|e| match e {
            CombatEvent::Explosion { point, radius } => Some((*point, *radius)),
            _ => None,
        }));
        opened |= fx.combat.contains(&CombatEvent::CrackOpened(BREACH));
        app.world_mut().resource_mut::<FxQueue>().combat.clear();
    }
    let (point, radius) = boom.expect("the blast went off");
    assert_eq!((point, radius), (Vec3::new(-2.0, 12.0, 1.0), 3.0));
    assert!(opened, "the blast opened the breach");
    let secs = n as f32 / 64.0;
    assert!(
        (0.4..0.7).contains(&secs),
        "after the 0.5 s delay: {secs} s"
    );
    assert_eq!(
        vitals(&mut app).0.health.hp,
        100,
        "the closed crack shielded the player"
    );

    // The crack opens through the mechanics, then the lounge is a walk away.
    ticks(&mut app, 60);
    let map = &app.world().resource::<CurrentMap>().0;
    assert!(
        map.sectors[BREACH].ceil_z >= 2.9,
        "{}",
        map.sectors[BREACH].ceil_z
    );
    teleport(&mut app, 0.8, 12.0, 180.0);
    assert_eq!(walk_to(&mut app, LOUNGE, 120).sector, LOUNGE);
}

/// A payphone tells how many secrets are left on the first call only.
#[test]
fn a_payphone_call_counts_the_secrets_left_once() {
    let mut app = app(load_map("engine_lab.ron"));
    // Facing the payphone on the hall's west wall.
    teleport(&mut app, 1.0, 6.0, 180.0);
    ticks(&mut app, 1);
    let phone = app
        .world()
        .resource::<CurrentMap>()
        .0
        .props
        .iter()
        .position(|p| p.kind == rr_core::props::PropKind::Payphone)
        .unwrap();
    assert_eq!(
        app.world().resource::<UsePrompt>().0,
        Some(UseTarget::Prop(phone))
    );
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    let text = app.world().resource::<HudMessage>().text.clone();
    assert!(text.contains("1 secret still out there"), "{text}");
    // After the cooldown: only a dial tone.
    ticks(&mut app, 120);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<HudMessage>().text,
        "Just a dial tone."
    );
}
