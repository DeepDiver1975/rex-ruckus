use bevy::prelude::*;
use rr_core::fixtures::pillar_room;
use rr_game::flow::FlowPlugin;
use rr_game::level::CurrentMap;
use rr_game::player::{PendingInput, PlayerBody, PlayerSimPlugin};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(CurrentMap(pillar_room()))
        .add_plugins((FlowPlugin, PlayerSimPlugin));
    app.update(); // runs Startup → spawns the player
    app
}

#[test]
fn player_spawns_at_level_start_on_the_floor() {
    let mut app = app();
    let mut q = app.world_mut().query::<&PlayerBody>();
    let body = q.single(app.world()).unwrap().0;
    assert_eq!((body.pos.x, body.pos.y, body.pos.z), (2.0, 1.0, 0.0));
}

#[test]
fn forward_input_moves_along_view_heading() {
    let mut app = app();
    {
        let mut q = app.world_mut().query::<&mut PendingInput>();
        q.single_mut(app.world_mut()).unwrap().forward = 1.0;
    }
    for _ in 0..60 {
        app.world_mut().run_schedule(FixedUpdate);
    }
    let mut q = app.world_mut().query::<&PlayerBody>();
    let body = q.single(app.world()).unwrap().0;
    // pillar_room starts facing north (90°): y grows, x stays put.
    assert!(body.pos.y > 8.0, "{}", body.pos);
    assert!((body.pos.x - 2.0).abs() < 0.01, "{}", body.pos);
}
