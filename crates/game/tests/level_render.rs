use bevy::prelude::*;
use rr_core::extrude::extrude_sector;
use rr_core::fixtures::door_rooms;
use rr_game::level::{LevelMaterials, SectorMesh, rebuild_dirty_sectors};
use rr_game::mechanics::{DirtySectors, insert_level};

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
        .insert_resource(LevelMaterials(vec![Handle::default(); 4]))
        .add_systems(Update, rebuild_dirty_sectors);
    insert_level(&mut app, door_rooms("(kind: Door)", ""));
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
