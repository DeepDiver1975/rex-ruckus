//! Actor visuals headless: assets without a GPU, the sim frozen so only the frame loop runs.
//! There is no glTF loader, so enemy models never load by themselves; tests that need a ready
//! model install a fake one ([`fake_ready`]).

use bevy::gltf::Gltf;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::world_serialization::WorldAsset;
use rr_core::actors::AiState;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
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
use rr_game::models::{ClipRole, EnemyGraph, ModelLibrary, ModelReady, ModelSlot, ModelsPlugin};
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
            skill: Difficulty::Easy,
            on_death: None,
        });
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        // The asset types ModelsPlugin loads (no loaders: loads just fail).
        .init_asset::<WorldAsset>()
        .init_asset::<Gltf>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            50,
        )));
    insert_level(&mut app, map, Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        ModelsPlugin,
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

/// Every material under Grunt `idx`'s visual (model meshes and tip glow), sorted.
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
    fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
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
    fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    app.update();
    let resting = grunt_materials(&mut app, 0);
    // The Hit variant, seen on a grunt that is not in pain.
    hurt(&mut app, 0);
    app.update();
    let hit = grunt_materials(&mut app, 0);
    assert_ne!(hit, resting, "the flash differs from the resting look");
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(grunt_materials(&mut app, 0), resting);
    for a in &mut app.world_mut().resource_mut::<LevelCombat>().0.actors {
        a.state = AiState::Pain { t: 10.0 };
    }
    app.update();
    let pain = grunt_materials(&mut app, 1);
    assert_ne!(pain, resting, "pain looks different from resting");
    hurt(&mut app, 0);
    app.update();
    assert_eq!(
        grunt_materials(&mut app, 0),
        pain,
        "a grunt in pain keeps its pain material when hit"
    );
    assert_ne!(grunt_materials(&mut app, 0), hit, "not the hit variant");
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

/// Length of every clip in the fake graph (s).
const FAKE_CLIP_SECS: f32 = 1.2;

fn actor_root(app: &mut App, idx: usize) -> Entity {
    let mut q = app.world_mut().query::<(Entity, &ActorVisual)>();
    q.iter(app.world()).find(|(_, g)| g.0 == idx).unwrap().0
}

/// The glTF model root ([`ModelSlot`]) directly under actor `idx`'s visual.
fn model_root(app: &mut App, idx: usize) -> Entity {
    let root = actor_root(app, idx);
    let mut q = app
        .world_mut()
        .query_filtered::<(Entity, &ChildOf), With<ModelSlot>>();
    q.iter(app.world())
        .find(|(_, c)| c.parent() == root)
        .expect("every actor has a model")
        .0
}

/// Stands in for a loaded glTF: gives `kind` an animation graph with `roles` and makes the
/// model of every actor of that kind "ready", with a player bound to the graph, two meshes
/// sharing one material (as glTF meshes do) and the `Hand.R` and `Muzzle` nodes.
fn fake_ready(
    app: &mut App,
    kind: ActorKind,
    roles: &[ClipRole],
) -> HashMap<ClipRole, AnimationNodeIndex> {
    let clips: Vec<Handle<AnimationClip>> = roles
        .iter()
        .map(|_| {
            let mut clip = AnimationClip::default();
            clip.set_duration(FAKE_CLIP_SECS);
            app.world_mut()
                .resource_mut::<Assets<AnimationClip>>()
                .add(clip)
        })
        .collect();
    let (graph, nodes) = AnimationGraph::from_clips(clips.iter().cloned());
    let graph = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    let nodes: HashMap<ClipRole, AnimationNodeIndex> = roles.iter().copied().zip(nodes).collect();
    app.world_mut()
        .resource_mut::<ModelLibrary>()
        .enemy_mut(kind)
        .graph = Some(EnemyGraph {
        graph: graph.clone(),
        nodes: nodes.clone(),
        clips: roles.iter().copied().zip(clips).collect(),
    });
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.5, 0.3),
            ..default()
        });
    let indices: Vec<usize> = {
        let combat = app.world().resource::<LevelCombat>();
        (0..combat.0.actors.len())
            .filter(|&i| combat.0.actors[i].kind == kind)
            .collect()
    };
    for i in indices {
        let m = model_root(app, i);
        let w = app.world_mut();
        let player = w
            .spawn((
                AnimationPlayer::default(),
                AnimationTransitions::new(),
                AnimationGraphHandle(graph.clone()),
                ChildOf(m),
            ))
            .id();
        let meshes = (0..2)
            .map(|_| w.spawn((MeshMaterial3d(material.clone()), ChildOf(m))).id())
            .collect();
        let mut named = HashMap::new();
        for name in ["Hand.R", "Muzzle"] {
            let e = w
                .spawn((Name::new(name), Transform::default(), ChildOf(player)))
                .id();
            named.insert(name.to_owned(), e);
        }
        w.entity_mut(m).insert(ModelReady {
            player: Some(player),
            meshes,
            nodes: named,
        });
    }
    nodes
}

fn player_of(app: &mut App, idx: usize) -> Entity {
    let m = model_root(app, idx);
    app.world().get::<ModelReady>(m).unwrap().player.unwrap()
}

/// How the main clip plays.
#[derive(Debug, Clone, Copy)]
struct Playback {
    speed: f32,
    looped: bool,
    paused: bool,
    seek: f32,
}

/// The main clip of actor `idx` and its playback.
fn playing(app: &mut App, idx: usize) -> (Option<AnimationNodeIndex>, Option<Playback>) {
    let p = player_of(app, idx);
    let main = app
        .world()
        .get::<AnimationTransitions>(p)
        .unwrap()
        .get_main_animation();
    let active = main.and_then(|n| {
        let a = app.world().get::<AnimationPlayer>(p)?.animation(n)?;
        Some(Playback {
            speed: a.speed(),
            looped: a.repeat_mode() == bevy::animation::RepeatAnimation::Forever,
            paused: a.is_paused(),
            seek: a.seek_time(),
        })
    });
    (main, active)
}

fn set_actor(app: &mut App, idx: usize, state: AiState, speed: f32) {
    let mut c = app.world_mut().resource_mut::<LevelCombat>();
    let a = &mut c.0.actors[idx];
    a.state = state;
    a.body.vel = Vec3::new(speed, 0.0, 0.0);
}

#[test]
fn ai_state_drives_the_clip_of_a_ready_model() {
    use ClipRole::*;
    let mut app = app_with(&[ActorKind::Grunt]);
    let nodes = fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    app.update();
    let (main, active) = playing(&mut app, 0);
    assert_eq!(main, Some(nodes[&Idle]), "asleep: idle");
    assert_eq!(active.unwrap().speed, 0.3, "drowsy idle");

    set_actor(&mut app, 0, AiState::Chase, 3.5);
    app.update();
    assert_eq!(playing(&mut app, 0).0, Some(nodes[&Run]), "full speed: run");
    set_actor(&mut app, 0, AiState::Chase, 0.5);
    app.update();
    assert_eq!(playing(&mut app, 0).0, Some(nodes[&Walk]), "slow: walk");

    set_actor(&mut app, 0, AiState::Attack { t: 0.3, left: 2 }, 0.0);
    app.update();
    assert_eq!(playing(&mut app, 0).0, Some(nodes[&Attack]));
    // Each shot restarts the attack clip.
    let p = player_of(&mut app, 0);
    app.world_mut()
        .get_mut::<AnimationPlayer>(p)
        .unwrap()
        .animation_mut(nodes[&Attack])
        .unwrap()
        .seek_to(0.6);
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::ActorFired { actor: 0 });
    app.update();
    assert_eq!(
        playing(&mut app, 0).1.unwrap().seek,
        0.0,
        "restarted by the shot"
    );

    set_actor(&mut app, 0, AiState::Pain { t: 0.3 }, 0.0);
    app.update();
    let (main, active) = playing(&mut app, 0);
    assert_eq!(main, Some(nodes[&Pain]));
    assert!(!active.unwrap().looped, "pain plays once");

    set_actor(&mut app, 0, AiState::Dying { t: 0.8 }, 0.0);
    app.update();
    let (main, active) = playing(&mut app, 0);
    assert_eq!(main, Some(nodes[&Death]));
    let a = active.unwrap();
    // Grunt death_time 0.8 s: a 1.2 s clip plays at 1.5x, once.
    assert!((a.speed - 1.5).abs() < 1e-5, "{a:?}");
    assert!(!a.looped && !a.paused);

    set_actor(&mut app, 0, AiState::Dead, 0.0);
    app.update();
    let (main, active) = playing(&mut app, 0);
    assert_eq!(main, Some(nodes[&Death]), "dead: the death clip stays");
    assert!(
        !active.unwrap().paused,
        "a clip that just played out is not jumped"
    );
}

#[test]
fn a_model_without_pain_keeps_its_clip() {
    use ClipRole::*;
    let mut app = app_with(&[ActorKind::Enforcer]);
    let nodes = fake_ready(
        &mut app,
        ActorKind::Enforcer,
        &[Idle, Walk, Run, Attack, Death],
    );
    set_actor(&mut app, 0, AiState::Chase, 0.5);
    app.update();
    assert_eq!(playing(&mut app, 0).0, Some(nodes[&Walk]));
    set_actor(&mut app, 0, AiState::Pain { t: 0.3 }, 0.0);
    app.update();
    assert_eq!(playing(&mut app, 0).0, Some(nodes[&Walk]), "no pain clip");
}

#[test]
fn a_corpse_seen_late_holds_the_last_death_frame() {
    let mut app = app_with(&[ActorKind::Grunt]);
    set_actor(&mut app, 0, AiState::Dead, 0.0);
    let nodes = fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    app.update();
    let (main, active) = playing(&mut app, 0);
    assert_eq!(main, Some(nodes[&ClipRole::Death]));
    let a = active.unwrap();
    assert!(!a.looped && a.paused);
    assert_eq!(a.seek, FAKE_CLIP_SECS, "at the end");
}

#[test]
fn the_grunt_holds_its_cannon_in_the_hand() {
    let mut app = app_with(&[ActorKind::Grunt]);
    fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    app.update();
    let m = model_root(&mut app, 0);
    let hand = app.world().get::<ModelReady>(m).unwrap().nodes["Hand.R"];
    let mut q = app
        .world_mut()
        .query_filtered::<&ChildOf, With<ModelSlot>>();
    let in_hand = q.iter(app.world()).filter(|c| c.parent() == hand).count();
    assert_eq!(in_hand, 1, "one cannon, parented to the bone");
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<&ChildOf, With<ModelSlot>>();
    let in_hand = q.iter(app.world()).filter(|c| c.parent() == hand).count();
    assert_eq!(in_hand, 1, "attached only once");
}

/// A source material that has not loaded yet must not be recorded as tinted: the look is
/// applied on a later frame, once the material is there.
#[test]
fn enemy_look_waits_for_its_material() {
    let mut app = app_with(&[ActorKind::Grunt]);
    fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .reserve_handle();
    let m = model_root(&mut app, 0);
    let meshes = app.world().get::<ModelReady>(m).unwrap().meshes.clone();
    for &e in &meshes {
        app.world_mut()
            .entity_mut(e)
            .insert(MeshMaterial3d(handle.clone()));
    }
    let current = |app: &App| {
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(meshes[0])
            .unwrap()
            .0
            .id()
    };
    app.update();
    assert_eq!(current(&app), handle.id(), "not swapped before it loads");
    let _ = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .insert(handle.id(), StandardMaterial::default());
    app.update();
    assert_ne!(
        current(&app),
        handle.id(),
        "looked once the material is there"
    );
}

/// A bone in a scaled armature (Quaternius rigs carry x100): the attachment must be
/// scale-corrected in the very frame it is attached, not drawn 100x too big for one frame.
#[test]
fn the_attachment_is_scale_corrected_the_frame_it_attaches() {
    let mut app = app_with(&[ActorKind::Grunt]);
    fake_ready(&mut app, ActorKind::Grunt, &ClipRole::ALL);
    let m = model_root(&mut app, 0);
    let hand = app.world().get::<ModelReady>(m).unwrap().nodes["Hand.R"];
    app.world_mut()
        .entity_mut(hand)
        .insert(GlobalTransform::from(Transform::from_scale(Vec3::splat(
            100.0,
        ))));
    app.update();
    let mut q = app
        .world_mut()
        .query_filtered::<(&ChildOf, &Transform), With<ModelSlot>>();
    let (_, t) = q
        .iter(app.world())
        .find(|(c, _)| c.parent() == hand)
        .expect("cannon attached");
    // The cannon's attach scale is 1.0 world units, so under a x100 bone it is 1/100.
    assert!(
        t.scale.abs_diff_eq(Vec3::splat(0.01), 1e-5),
        "{:?}",
        t.scale
    );
}

/// The Enforcer's tip glow sits on its `Muzzle` node and lights only after a shot.
#[test]
fn a_node_tip_glows_after_a_shot() {
    let mut app = app_with(&[ActorKind::Enforcer]);
    fake_ready(&mut app, ActorKind::Enforcer, &ClipRole::ALL);
    app.update();
    let m = model_root(&mut app, 0);
    let muzzle = app.world().get::<ModelReady>(m).unwrap().nodes["Muzzle"];
    let tip = |app: &mut App| -> Visibility {
        let mut q = app
            .world_mut()
            .query_filtered::<(&ChildOf, &Visibility), With<Mesh3d>>();
        let v: Vec<_> = q
            .iter(app.world())
            .filter(|(c, _)| c.parent() == muzzle)
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(v.len(), 1, "one tip sphere on Muzzle");
        v[0]
    };
    app.update();
    assert_eq!(tip(&mut app), Visibility::Hidden);
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::ActorFired { actor: 0 });
    app.update();
    assert_eq!(tip(&mut app), Visibility::Inherited, "glows after the shot");
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(tip(&mut app), Visibility::Hidden, "dark again");
}
