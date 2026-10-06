//! Item props headless: every map item gets an `ItemProp` with a glTF model child.

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use rr_core::defs::Defs;
use rr_core::fixtures::combat_room;
use rr_core::map::{Item, ItemKind, Key};
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::flow::FlowPlugin;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::models::{ModelSlot, ModelsPlugin};
use rr_game::player::PlayerSimPlugin;
use rr_game::props::{ItemProp, PropsPlugin};

#[test]
fn every_item_has_a_model_child() {
    let mut map = combat_room();
    let kinds = [
        ItemKind::PistolAmmo,
        ItemKind::HealthSmall,
        ItemKind::Jetpack,
        ItemKind::Key(Key::Red),
        ItemKind::Key(Key::Blue),
        ItemKind::Shotgun,
    ];
    for (i, kind) in kinds.iter().enumerate() {
        map.items.push(Item {
            kind: *kind,
            pos: Vec2::new(2.0 + i as f32 * 0.5, 1.5),
        });
    }
    let n = map.items.len();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<Gltf>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>();
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        ModelsPlugin,
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        PropsPlugin,
    ));
    app.update();
    let mut q = app.world_mut().query::<(&ItemProp, &Children)>();
    let mut with_model = Vec::new();
    for (p, children) in q.iter(app.world()) {
        let models = children
            .iter()
            .filter(|c| app.world().get::<ModelSlot>(*c).is_some())
            .count();
        assert_eq!(models, 1, "item {} has one model child", p.index);
        with_model.push(p.index);
    }
    with_model.sort();
    assert_eq!(with_model, (0..n).collect::<Vec<_>>());
}

#[test]
fn keycard_is_tinted_once_its_material_has_loaded() {
    use rr_game::models::ModelReady;
    let mut map = combat_room();
    map.items.push(Item {
        kind: ItemKind::Key(Key::Red),
        pos: Vec2::new(2.0, 1.5),
    });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<WorldAsset>()
        .init_asset::<Gltf>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>();
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        ModelsPlugin,
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        PropsPlugin,
    ));
    app.update();
    let mut q = app.world_mut().query::<(Entity, &ModelSlot)>();
    let root = q.iter(app.world()).next().expect("keycard model").0;
    // A mesh whose material is reserved but not yet loaded.
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .reserve_handle();
    let mesh = app
        .world_mut()
        .spawn(MeshMaterial3d::<StandardMaterial>(handle.clone()))
        .id();
    app.world_mut().entity_mut(root).insert(ModelReady {
        meshes: vec![mesh],
        ..default()
    });
    app.update();
    let current = |app: &App| {
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .unwrap()
            .0
            .id()
    };
    assert_eq!(
        current(&app),
        handle.id(),
        "not tinted before the material loads"
    );
    let _ = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .insert(handle.id(), StandardMaterial::default());
    app.update();
    assert_ne!(
        current(&app),
        handle.id(),
        "tinted once the material is there"
    );
}
