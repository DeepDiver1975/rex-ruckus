use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_core::extrude::extrude_sector;
use rr_core::fixtures::door_rooms;
use rr_game::level::{LevelMaterials, SectorMesh, rebuild_dirty_sectors};
use rr_game::mechanics::{DirtySectors, insert_level};
use std::collections::BTreeSet;

fn count(app: &mut App, s: usize) -> usize {
    let mut q = app.world_mut().query::<&SectorMesh>();
    q.iter(app.world()).filter(|m| m.0 == s).count()
}

#[test]
fn rebuild_replaces_meshes_of_changed_sector_and_neighbours() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_resource::<DirtySectors>()
        .insert_resource(LevelMaterials(vec![Handle::default(); 4], Vec::new()))
        .add_systems(Update, rebuild_dirty_sectors);
    insert_level(&mut app, door_rooms("(kind: Door)", ""), Difficulty::Normal);
    app.world_mut().resource_mut::<DirtySectors>().0.insert(1);
    app.update();
    // Expected counts come from the live map (door closed), one entity per material sub-mesh.
    let live = app
        .world()
        .resource::<rr_game::level::CurrentMap>()
        .0
        .clone();
    for s in 0..3 {
        assert_eq!(
            count(&mut app, s),
            extrude_sector(&live, s).unwrap().len(),
            "sector {s}"
        );
    }
    assert!(app.world().resource::<DirtySectors>().0.is_empty());
    app.update(); // nothing dirty: nothing duplicated
    assert_eq!(count(&mut app, 0), extrude_sector(&live, 0).unwrap().len());
}

fn ids(app: &mut App, s: usize) -> BTreeSet<Entity> {
    let mut q = app.world_mut().query::<(Entity, &SectorMesh)>();
    q.iter(app.world())
        .filter(|(_, m)| m.0 == s)
        .map(|(e, _)| e)
        .collect()
}

#[test]
fn rebuild_despawns_old_meshes_and_spares_unaffected_sectors() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_resource::<DirtySectors>()
        .insert_resource(LevelMaterials(vec![Handle::default(); 4], Vec::new()))
        .add_systems(Update, rebuild_dirty_sectors);
    insert_level(&mut app, door_rooms("(kind: Door)", ""), Difficulty::Normal);
    app.world_mut()
        .resource_mut::<DirtySectors>()
        .0
        .extend([0, 1, 2]);
    app.update();
    let first: Vec<_> = (0..3).map(|s| ids(&mut app, s)).collect();

    // Sector 0 dirty: 0 and its neighbour 1 are replaced, sector 2 is untouched.
    app.world_mut().resource_mut::<DirtySectors>().0.insert(0);
    app.update();
    let live = app
        .world()
        .resource::<rr_game::level::CurrentMap>()
        .0
        .clone();
    for (s, before) in first.iter().enumerate() {
        let now = ids(&mut app, s);
        assert_eq!(
            now.len(),
            extrude_sector(&live, s).unwrap().len(),
            "sector {s}"
        );
        if s == 2 {
            assert_eq!(&now, before, "sector 2 must keep its entities");
        } else {
            assert!(now.is_disjoint(before), "sector {s} must be replaced");
        }
    }
}

mod world {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use rr_core::combat::CombatEvent;
    use rr_core::defs::Defs;
    use rr_core::fixtures::glass_rooms;
    use rr_core::map::{Map, RawLight};
    use rr_game::breakables::{BreakablesPlugin, FixtureMaterials, LightFixture, Shard};
    use rr_game::combat::{CombatSimPlugin, FxQueue, insert_defs};
    use rr_game::flow::FlowPlugin;
    use rr_game::level::{LevelLight, LevelRenderPlugin};
    use rr_game::mechanics::{HudMessage, MechanicsSimPlugin};
    use rr_game::player::{PendingInput, PlayerSimPlugin};
    use std::time::Duration;

    fn world_app(map: Map) -> App {
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
            LevelRenderPlugin,
            BreakablesPlugin,
        ));
        app.update();
        app
    }

    /// Entities drawing the glass pane (any sector).
    fn panes(app: &mut App) -> usize {
        let glass = app
            .world()
            .resource::<rr_game::level::CurrentMap>()
            .0
            .glass_material()
            .unwrap();
        let handle = app.world().resource::<LevelMaterials>().0[glass].clone();
        let mut q = app
            .world_mut()
            .query::<(&SectorMesh, &MeshMaterial3d<StandardMaterial>)>();
        q.iter(app.world()).filter(|(_, m)| m.0 == handle).count()
    }

    fn count<C: Component>(app: &mut App) -> usize {
        let mut q = app.world_mut().query_filtered::<(), With<C>>();
        q.iter(app.world()).count()
    }

    fn fire(app: &mut App) {
        let mut q = app.world_mut().query::<&mut PendingInput>();
        q.single_mut(app.world_mut()).unwrap().fire_pressed = true;
    }

    #[test]
    fn glass_break_remeshes_both_sectors() {
        let mut app = world_app(glass_rooms());
        assert!(panes(&mut app) >= 1, "intact pane is drawn");
        let before: Vec<_> = (0..2).map(|s| ids(&mut app, s)).collect();
        fire(&mut app);
        app.update();
        for (s, old) in before.iter().enumerate() {
            let now = ids(&mut app, s);
            assert!(!now.is_empty());
            assert!(now.is_disjoint(old), "sector {s} re-meshed");
        }
        assert_eq!(count::<Shard>(&mut app), rr_game::breakables::SHARD_COUNT);
    }

    #[test]
    fn broken_pane_disappears_in_the_frame_it_breaks() {
        let mut app = world_app(glass_rooms());
        assert!(panes(&mut app) >= 1);
        fire(&mut app);
        app.update(); // one frame: the fixed tick breaks the glass, Update re-meshes
        assert_eq!(panes(&mut app), 0);
    }

    #[test]
    fn shards_fade_and_despawn() {
        let mut app = world_app(glass_rooms());
        fire(&mut app);
        app.update();
        assert!(count::<Shard>(&mut app) > 0);
        for _ in 0..12 {
            app.update();
        }
        assert_eq!(count::<Shard>(&mut app), 0);
    }

    fn lit_map() -> Map {
        let mut map = glass_rooms();
        map.lights.push(RawLight {
            pos: (2.0, 2.0, 3.5),
            color: (1.0, 1.0, 1.0),
            intensity: 100_000.0,
            range: 10.0,
            breakable: true,
            shadows: true,
        });
        map
    }

    #[test]
    fn light_shadows_flag_reaches_the_point_light() {
        let mut map = lit_map();
        map.lights.push(RawLight {
            shadows: false,
            ..map.lights[0]
        });
        let mut app = world_app(map);
        let mut q = app.world_mut().query::<(&PointLight, &LevelLight)>();
        let mut flags: Vec<(usize, bool)> = q
            .iter(app.world())
            .map(|(p, l)| (l.0, p.shadow_maps_enabled))
            .collect();
        flags.sort();
        assert_eq!(flags, vec![(0, true), (1, false)]);
    }

    #[test]
    fn light_broken_despawns_point_light() {
        let mut app = world_app(lit_map());
        assert_eq!(count::<LevelLight>(&mut app), 1);
        assert_eq!(count::<LightFixture>(&mut app), 1, "fixture is drawn");
        let dark = app.world().resource::<FixtureMaterials>().dark.clone();
        app.world_mut()
            .resource_mut::<FxQueue>()
            .combat
            .push(CombatEvent::LightBroken(0));
        app.update();
        assert_eq!(count::<LevelLight>(&mut app), 0);
        let mut q = app
            .world_mut()
            .query::<(&LightFixture, &MeshMaterial3d<StandardMaterial>)>();
        let (_, m) = q.single(app.world()).unwrap();
        assert_eq!(m.0, dark, "fixture goes dark");
        assert!(count::<rr_game::actors::Spark>(&mut app) > 0, "sparks");
    }

    #[test]
    fn secret_found_shows_hud_message() {
        let mut app = world_app(glass_rooms());
        app.world_mut()
            .resource_mut::<FxQueue>()
            .combat
            .push(CombatEvent::SecretFound);
        app.update();
        let msg = app.world().resource::<HudMessage>();
        assert_eq!(msg.text, "Secret found!");
        assert!(msg.remaining > 0.0);
    }
}

#[test]
fn crack_sectors_use_the_cracked_material() {
    let mut app = App::new();
    insert_level(
        &mut app,
        door_rooms("(kind: Crack)", ""),
        Difficulty::Normal,
    );
    let map = &app.world().resource::<rr_game::level::CurrentMap>().0;
    let id = map.materials.iter().position(|m| m == "cracked").unwrap();
    assert_eq!(map.sectors[1].face_mat, Some(id));
    assert_eq!(map.sectors[1].wall_mat, id);
    assert_eq!(map.sectors[0].face_mat, None, "other sectors untouched");
}

#[test]
fn hazard_sectors_use_their_material() {
    let (map, _) = rr_game::mechanics::fresh_level(rr_core::fixtures::engine_room(""));
    assert_eq!(map.0.materials[map.0.sectors[3].floor_mat], "slime");
    assert_eq!(map.0.materials[map.0.sectors[0].floor_mat], "floor");
}
