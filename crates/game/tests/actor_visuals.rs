//! Actor visuals headless: assets without a GPU, the sim frozen so only the frame loop runs.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::actors::AiState;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::fixtures::combat_room;
use rr_core::map::{ActorKind, ActorSpawn};
use rr_core::projectile::{Projectile, Shooter, Targets};
use rr_game::actors::{
    ActorVisual, ActorVisualsPlugin, BarrelVisual, BoltVisual, BombVisual, DroneVisual,
    EnforcerVisual, GruntVisual, RocketVisual, SlasherVisual, Spark,
};
use rr_game::combat::{CombatSimPlugin, FxQueue, LevelCombat, insert_defs};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::fx::{ExplosionFx, ExplosionLight, FxPlugin, ScreenShake};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::PlayerCamera;
use rr_game::player::PlayerSimPlugin;
use std::time::Duration;

fn app() -> App {
    app_with(&[ActorKind::Grunt, ActorKind::Grunt])
}

fn app_with(kinds: &[ActorKind]) -> App {
    let mut map = combat_room();
    for (i, kind) in kinds.iter().enumerate() {
        map.actors.push(ActorSpawn {
            kind: *kind,
            pos: Vec2::new(5.0 + i as f32, 1.5),
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
        FxPlugin,
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
        gravity: 0.0,
        bounce: None,
        remote: false,
        splash: None,
        resting: false,
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
            sector: 0,
            wall: None,
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

/// Every material under Grunt `idx`'s visual (skin, visor, gun, tip), sorted.
fn grunt_materials(app: &mut App, idx: usize) -> Vec<AssetId<StandardMaterial>> {
    let root = {
        let mut q = app.world_mut().query::<(Entity, &GruntVisual)>();
        q.iter(app.world()).find(|(_, g)| g.0 == idx).unwrap().0
    };
    let mut q = app
        .world_mut()
        .query::<(Entity, &MeshMaterial3d<StandardMaterial>)>();
    let parts: Vec<_> = q.iter(app.world()).map(|(e, m)| (e, m.0.id())).collect();
    let mut out: Vec<_> = parts
        .into_iter()
        .filter(|(e, _)| {
            let mut cur = *e;
            while let Some(p) = app.world().get::<ChildOf>(cur) {
                cur = p.parent();
                if cur == root {
                    return true;
                }
            }
            false
        })
        .map(|(_, id)| id)
        .collect();
    out.sort();
    out
}

fn hurt(app: &mut App, actor: usize) {
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::ActorHurt { actor, amount: 5 });
}

#[test]
fn a_hit_flashes_the_grunt_briefly() {
    let mut app = app();
    app.update();
    assert_eq!(grunt_materials(&mut app, 0), grunt_materials(&mut app, 1));
    hurt(&mut app, 0);
    app.update();
    assert_ne!(
        grunt_materials(&mut app, 0),
        grunt_materials(&mut app, 1),
        "the hit grunt flashes"
    );
    // 50 ms frames: over within about 0.1 s.
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(grunt_materials(&mut app, 0), grunt_materials(&mut app, 1));
}

#[test]
fn pain_material_wins_over_the_hit_flash() {
    let mut app = app();
    for a in &mut app.world_mut().resource_mut::<LevelCombat>().0.actors {
        a.state = AiState::Pain { t: 10.0 };
    }
    hurt(&mut app, 0);
    app.update();
    assert_eq!(
        grunt_materials(&mut app, 0),
        grunt_materials(&mut app, 1),
        "a grunt in pain keeps its pain material"
    );
}

fn bomb(id: u32) -> Projectile {
    Projectile {
        owner: Shooter::Player,
        targets: Targets::All,
        gravity: 20.0,
        bounce: Some(0.5),
        remote: true,
        vel: Vec3::new(6.0, 0.0, 2.0),
        ..bolt(id)
    }
}

fn rocket(id: u32) -> Projectile {
    Projectile {
        owner: Shooter::Player,
        targets: Targets::All,
        vel: Vec3::new(20.0, 0.0, 0.0),
        splash: Some(rr_core::defs::SplashDef {
            radius: 3.0,
            damage: 100,
            self_scale: 0.5,
        }),
        ..bolt(id)
    }
}

fn rot_of<C: Component>(app: &mut App) -> (Vec3, Quat) {
    let mut q = app.world_mut().query_filtered::<&Transform, With<C>>();
    let t = q.single(app.world()).unwrap();
    (t.translation, t.rotation)
}

/// Bombs and rockets get their own visuals (never a bolt), follow their projectile, bombs
/// spin while moving and stop when resting, and a gone projectile loses its visual.
#[test]
fn bomb_visual_follows_projectile() {
    let mut app = app();
    let set = |app: &mut App, ps: Vec<Projectile>| {
        app.world_mut().resource_mut::<LevelCombat>().0.projectiles = ps;
    };
    set(&mut app, vec![bomb(1), rocket(2), bolt(3)]);
    app.update();
    assert_eq!(count::<BombVisual>(&mut app), 1);
    assert_eq!(count::<RocketVisual>(&mut app), 1);
    assert_eq!(bolt_ids(&mut app), vec![3], "only the actor shot is a bolt");
    let (pos, r0) = rot_of::<BombVisual>(&mut app);
    // Between prev (x 2.9) and pos (x 3.0) in core x, which is Bevy x.
    assert!(pos.x >= 2.89 && pos.x <= 3.01, "{pos:?}");
    app.update();
    app.update();
    let (_, r1) = rot_of::<BombVisual>(&mut app);
    assert!(r0.angle_between(r1) > 0.05, "spins while moving");
    // Rests: the spin stops.
    let mut resting = bomb(1);
    resting.resting = true;
    resting.vel = Vec3::ZERO;
    set(&mut app, vec![resting]);
    app.update();
    let (_, a) = rot_of::<BombVisual>(&mut app);
    app.update();
    app.update();
    let (_, b) = rot_of::<BombVisual>(&mut app);
    assert!(a.abs_diff_eq(b, 1e-6), "resting bomb is still");
    assert_eq!(count::<RocketVisual>(&mut app), 0);
    set(&mut app, vec![]);
    app.update();
    assert_eq!(count::<BombVisual>(&mut app), 0);
}

#[test]
fn each_kind_spawns_rig() {
    let mut app = app_with(&[
        ActorKind::Grunt,
        ActorKind::Enforcer,
        ActorKind::Slasher,
        ActorKind::Drone,
        ActorKind::Barrel,
    ]);
    assert_eq!(count::<GruntVisual>(&mut app), 1);
    assert_eq!(count::<EnforcerVisual>(&mut app), 1);
    assert_eq!(count::<SlasherVisual>(&mut app), 1);
    assert_eq!(count::<DroneVisual>(&mut app), 1);
    assert_eq!(count::<BarrelVisual>(&mut app), 1);
    assert_eq!(count::<ActorVisual>(&mut app), 5, "one root per actor");
}

#[test]
fn explosion_spawns_fx_and_shake() {
    let mut app = app();
    app.world_mut().spawn((Camera3d::default(), PlayerCamera));
    assert_eq!(app.world().resource::<ScreenShake>().trauma, 0.0);
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::Explosion {
            point: Vec3::new(3.0, 1.5, 1.0),
            radius: 4.0,
        });
    app.update();
    assert_eq!(count::<ExplosionFx>(&mut app), 1);
    assert_eq!(count::<ExplosionLight>(&mut app), 1);
    let trauma = app.world().resource::<ScreenShake>().trauma;
    assert!(trauma > 0.5 && trauma <= 1.0, "player is close: {trauma}");
    // The camera offset lands in the next frame's RunFixedMainLoop, before Update.
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Transform, With<PlayerCamera>>();
    let t = *q.single(app.world()).unwrap();
    assert_ne!(t.translation, Vec3::ZERO, "the camera is nudged");
    // The light dies after 0.2 s, the sphere after 0.35 s; trauma decays.
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(count::<ExplosionLight>(&mut app), 0);
    assert_eq!(count::<ExplosionFx>(&mut app), 1);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(count::<ExplosionFx>(&mut app), 0);
    assert!(app.world().resource::<ScreenShake>().trauma < trauma);
}

#[test]
fn far_explosion_adds_no_shake() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::Explosion {
            point: Vec3::new(15.0, 1.5, 1.0),
            radius: 2.0,
        });
    app.update();
    assert_eq!(app.world().resource::<ScreenShake>().trauma, 0.0);
    assert_eq!(count::<ExplosionFx>(&mut app), 1);
}

/// Core drops dead flyers itself; the visual must follow that z, not add a drop of its own.
#[test]
fn dying_drone_visual_follows_core_z() {
    let mut app = app_with(&[ActorKind::Drone]);
    {
        let mut c = app.world_mut().resource_mut::<LevelCombat>();
        let a = &mut c.0.actors[0];
        a.state = AiState::Dead;
        a.prev_pos.z = 0.4;
        a.body.pos.z = 0.4;
    }
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&Transform, With<DroneVisual>>();
    let y = q.single(app.world()).unwrap().translation.y;
    assert!((y - 0.4).abs() < 1e-5, "visual y {y} equals core z");
}
