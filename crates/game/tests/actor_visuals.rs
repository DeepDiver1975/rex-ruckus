//! Actor visuals headless: assets without a GPU, the sim frozen so only the frame loop runs.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::fixtures::combat_room;
use rr_core::map::{ActorKind, ActorSpawn};
use rr_core::projectile::{Projectile, Shooter, Targets};
use rr_game::actors::{ActorVisualsPlugin, BoltVisual, GruntVisual, Spark};
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, insert_defs};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::PlayerSimPlugin;
use std::time::Duration;

fn app() -> App {
    let mut map = combat_room();
    for x in [6.0, 7.0] {
        map.actors.push(ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(x, 1.5),
            angle: std::f32::consts::PI,
            asleep: true,
        });
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            50,
        )));
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        ActorVisualsPlugin,
    ));
    app.update();
    // Freeze the sim: only the frame-loop visuals run from here on.
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Complete;
    app
}

fn count<C: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<C>>();
    q.iter(app.world()).count()
}

fn bolt(id: u32) -> Projectile {
    Projectile {
        id,
        pos: Vec3::new(3.0, 1.5, 1.0),
        prev: Vec3::new(2.9, 1.5, 1.0),
        vel: Vec3::new(-15.0, 0.0, 0.0),
        sector: 0,
        radius: 0.12,
        damage: 8,
        owner: Shooter::Actor(0),
        targets: Targets::Player,
        life: 2.0,
    }
}

fn bolt_ids(app: &mut App) -> Vec<u32> {
    let mut q = app.world_mut().query::<&BoltVisual>();
    let mut ids: Vec<u32> = q.iter(app.world()).map(|b| b.0).collect();
    ids.sort();
    ids
}

#[test]
fn one_grunt_visual_per_actor() {
    let mut app = app();
    let mut q = app.world_mut().query::<&GruntVisual>();
    let mut ids: Vec<usize> = q.iter(app.world()).map(|g| g.0).collect();
    ids.sort();
    assert_eq!(ids, vec![0, 1]);
}

#[test]
fn bolt_visuals_follow_projectile_ids() {
    let mut app = app();
    assert!(bolt_ids(&mut app).is_empty());
    let projectiles = |app: &mut App| -> Vec<Projectile> {
        app.world_mut()
            .resource_mut::<LevelCombat>()
            .0
            .projectiles
            .clone()
    };
    let set = |app: &mut App, ps: Vec<Projectile>| {
        app.world_mut().resource_mut::<LevelCombat>().0.projectiles = ps;
    };
    set(&mut app, vec![bolt(3), bolt(7)]);
    app.update();
    assert_eq!(bolt_ids(&mut app), vec![3, 7]);
    app.update();
    assert_eq!(bolt_ids(&mut app), vec![3, 7], "no duplicates");
    let mut ps = projectiles(&mut app);
    ps.retain(|p| p.id != 3);
    set(&mut app, ps);
    app.update();
    assert_eq!(
        bolt_ids(&mut app),
        vec![7],
        "a gone projectile loses its visual"
    );
    set(&mut app, vec![]);
    app.update();
    assert!(bolt_ids(&mut app).is_empty());
}

#[test]
fn impact_sparks_expire_and_fx_drains_after_readers() {
    let mut app = app();
    app.world_mut().resource_mut::<FxQueue>().combat.extend([
        CombatEvent::Impact {
            point: Vec3::new(0.0, 2.0, 1.0),
            normal: Vec3::X,
        },
        CombatEvent::ActorFired { actor: 0 },
    ]);
    app.update();
    assert_eq!(count::<Spark>(&mut app), 1, "the reader saw the impact");
    let fx = app.world().resource::<FxQueue>();
    assert!(
        fx.combat.is_empty() && fx.weapon.is_empty(),
        "cleared after readers"
    );
    app.update();
    assert_eq!(
        count::<Spark>(&mut app),
        1,
        "no second spark from a stale event"
    );
    // 50 ms frames: gone within 0.15 s.
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(count::<Spark>(&mut app), 0);
}
