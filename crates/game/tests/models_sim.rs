//! Model library headless: no renderer and no glTF loader, so scenes never finish loading, but
//! the library builds from the shipped definitions and `spawn_model` lays out the root.

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use rr_core::defs::WeaponId;
use rr_core::map::{ActorKind, ItemKind};
use rr_core::models::Placement;
use rr_game::models::{ClipRole, ModelLibrary, ModelSlot, ModelsPlugin, spawn_model};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<Gltf>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>()
        .add_plugins(ModelsPlugin);
    app.update();
    app
}

#[test]
fn library_builds_from_the_shipped_defs() {
    let mut app = app();
    app.update();
    let lib = app.world().resource::<ModelLibrary>();
    for kind in ActorKind::ALL {
        let e = lib.enemy(kind);
        // Without a glTF loader the graph is never built, so no clip resolves.
        assert!(e.graph.is_none());
        assert_eq!(lib.clip_node(kind, ClipRole::Idle), None);
    }
    assert!(lib.weapon(WeaponId::Boot).is_none());
    assert!(lib.weapon(WeaponId::Pistol).is_some());
    assert_eq!(
        lib.item(ItemKind::Medkit).scene.path().unwrap().label(),
        Some("Scene0")
    );
}

#[test]
fn spawn_model_creates_a_placed_scene_root() {
    let mut app = app();
    let scene = app
        .world()
        .resource::<ModelLibrary>()
        .enemy(ActorKind::Grunt)
        .model
        .scene
        .clone();
    let parent = app.world_mut().spawn(Transform::default()).id();
    let place = Placement {
        scale: 2.0,
        offset: (0.0, 1.0, 0.0),
        ..Placement::default()
    };
    let root = {
        let world = app.world_mut();
        let mut commands = world.commands();
        let root = spawn_model(&mut commands, parent, &scene, &place);
        world.flush();
        root
    };
    app.update();
    let world = app.world();
    let e = world.entity(root);
    assert_eq!(e.get::<WorldAssetRoot>().unwrap().0, scene);
    assert_eq!(e.get::<ChildOf>().unwrap().parent(), parent);
    let t = e.get::<Transform>().unwrap();
    assert_eq!(t.scale, Vec3::splat(2.0));
    assert_eq!(t.translation, Vec3::Y);
    let slot = e.get::<ModelSlot>().unwrap();
    assert!(slot.enemy.is_none() && !slot.no_shadows);
}
