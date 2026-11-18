//! Co-op over a bad network: a host app and a client app joined through a [`LossyLink`]
//! (100 ms each way, 5 % of datagrams lost), all clocks driven by the fixed timestep.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::combat::{CombatEvent, PendingBlast, Via};
use rr_core::defs::{Defs, SplashDef, WeaponId};
use rr_core::difficulty::Difficulty;
use rr_core::explosion::Blast;
use rr_core::fixtures::combat_room;
use rr_core::map::ActorKind;
use rr_core::players::PlayerId;
use rr_core::projectile::Shooter;
use rr_core::weapons::WeaponEvent;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerVitals, insert_defs,
};
use rr_game::flow::{FlowPlugin, LevelFile};
use rr_game::level::load_map;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::net::client::{NetClient, NetClientPlugin, start_joining};
use rr_game::net::host::{NetHost, NetHostPlugin, start_hosting};
use rr_game::player::{LocalPlayer, Look, PendingInput, Player, PlayerBody, PlayerSimPlugin};
use rr_net::testing::LossyLink;

const DELAY: Duration = Duration::from_millis(100);
const LOSS: f32 = 0.05;
const SEED: u64 = 7;
/// Extra delay of up to a tick, for the tests that time the link.
const JITTER: Duration = Duration::from_millis(16);

fn sim_app(extra: impl FnOnce(&mut App)) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    extra(&mut app);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        NetHostPlugin,
        NetClientPlugin,
    ));
    app.update();
    app
}

/// A host on `level` and a client that joined it through the lossy link.
struct Game {
    host: App,
    client: App,
    link: LossyLink,
    /// Link time: one fixed timestep per round.
    now: Duration,
}

impl Game {
    fn new(level: &str) -> Game {
        Game::with_jitter(level, Duration::ZERO)
    }

    /// [`Game::new`] with up to `jitter` extra delay per datagram (no reordering).
    fn with_jitter(level: &str, jitter: Duration) -> Game {
        let mut host = sim_app(|app| {
            insert_level(app, load_map(level), Difficulty::Normal);
            app.insert_resource(LevelFile(level.into()));
        });
        start_hosting(
            host.world_mut(),
            "127.0.0.1:0".parse().unwrap(),
            "Host".into(),
        )
        .expect("host");
        let server = host.world().resource::<NetHost>().host.local_addr();
        let link = LossyLink::new(server, DELAY, LOSS, SEED)
            .expect("link")
            .with_jitter(jitter);
        let mut client = sim_app(|app| {
            insert_level(app, combat_room(), Difficulty::Normal);
            app.insert_resource(LevelFile("combat_room".into()));
        });
        start_joining(client.world_mut(), link.addr(), "Tester".into()).expect("client");
        let mut game = Game {
            host,
            client,
            link,
            now: Duration::ZERO,
        };
        for _ in 0..600 {
            game.round();
            if game.joined() {
                return game;
            }
        }
        panic!("never joined through the link");
    }

    fn joined(&mut self) -> bool {
        let mut q = self.client.world_mut().query::<&Player>();
        let mut ids: Vec<u8> = q.iter(self.client.world()).map(|p| p.id.0).collect();
        ids.sort();
        ids == [0, 1]
            && self
                .client
                .world()
                .get_resource::<NetClient>()
                .is_some_and(|n| n.last_tick.is_some())
    }

    /// One fixed tick of each app, the link carrying what each sent.
    fn round(&mut self) {
        self.host.update();
        self.host.world_mut().run_schedule(FixedUpdate);
        self.link.pump(self.now);
        self.client.update();
        self.client.world_mut().run_schedule(FixedUpdate);
        self.link.pump(self.now);
        self.now += self.host.world().resource::<Time<Fixed>>().timestep();
        // Time for loopback delivery only: every clock above is the fixed timestep.
        std::thread::sleep(Duration::from_millis(1));
    }

    fn rounds(&mut self, n: usize) {
        for _ in 0..n {
            self.round();
        }
    }
}

fn pos(app: &mut App, id: PlayerId) -> Vec3 {
    let mut q = app.world_mut().query::<(&Player, &PlayerBody)>();
    q.iter(app.world())
        .find(|(p, _)| p.id == id)
        .map(|(_, b)| b.0.pos)
        .unwrap_or_else(|| panic!("no player {id:?}"))
}

fn local_pos(app: &mut App) -> Vec3 {
    let mut q = app
        .world_mut()
        .query_filtered::<&PlayerBody, With<LocalPlayer>>();
    q.single(app.world()).unwrap().0.pos
}

fn local_input(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    q.single_mut(app.world_mut()).unwrap()
}

#[test]
fn taps_survive_loss() {
    let mut game = Game::new("test_yard.ron");
    // A full magazine for all 20 taps: no reload pause swallows one.
    {
        let world = game.host.world_mut();
        let mut q = world.query::<(&Player, &mut PlayerArsenal)>();
        let (_, mut arsenal) = q
            .iter_mut(world)
            .find(|(p, _)| p.id == PlayerId(1))
            .unwrap();
        assert_eq!(arsenal.0.current, WeaponId::Pistol);
        arsenal.0.clip[WeaponId::Pistol.index()] = 100;
    }
    game.rounds(30);
    let fires = |host: &App| {
        host.world()
            .resource::<FxQueue>()
            .weapon
            .iter()
            .filter(|(p, e)| *p == PlayerId(1) && matches!(e, WeaponEvent::Fire { .. }))
            .count()
    };
    let mut shots = 0;
    for tap in 0..20 * 30 + 60 {
        if tap % 30 == 0 && tap < 20 * 30 {
            local_input(&mut game.client).fire_pressed = true;
        }
        game.round();
        shots += fires(&game.host);
    }
    assert_eq!(shots, 20);
}

#[test]
fn positions_converge() {
    let mut game = Game::new("test_yard.ron");
    let start = {
        let mut q = game
            .client
            .world_mut()
            .query_filtered::<&Look, With<LocalPlayer>>();
        q.single(game.client.world()).unwrap().angle
    };
    let from = pos(&mut game.host, PlayerId(1));
    let mut farthest: f32 = 0.0;
    for tick in 0..240 {
        let leg = (tick / 60) as f32;
        {
            let world = game.client.world_mut();
            let mut q = world.query_filtered::<&mut Look, With<LocalPlayer>>();
            q.single_mut(world).unwrap().angle = start + leg * std::f32::consts::FRAC_PI_2;
        }
        local_input(&mut game.client).forward = 1.0;
        game.round();
        farthest = farthest.max(pos(&mut game.host, PlayerId(1)).distance(from));
    }
    assert!(farthest > 2.0, "the host's player 1 got {farthest} m out");
    local_input(&mut game.client).forward = 0.0;
    game.rounds(60);
    let (h, c) = (
        pos(&mut game.host, PlayerId(1)),
        local_pos(&mut game.client),
    );
    assert!(h.distance(c) < 0.05, "own player: host {h} client {c}");
    let (h, c) = (
        pos(&mut game.host, PlayerId(0)),
        pos(&mut game.client, PlayerId(0)),
    );
    assert!(h.distance(c) < 0.05, "host's player: host {h} client {c}");
}

fn killed(app: &App) -> impl Iterator<Item = usize> + '_ {
    app.world()
        .resource::<FxQueue>()
        .combat
        .iter()
        .filter_map(|e| match e {
            CombatEvent::ActorKilled { actor, .. } => Some(*actor),
            _ => None,
        })
}

/// Ten enemies of `host`'s level, each well clear of the players, the barrels and the others
/// (so a small blast at one hurts no one else), in the order they are to die.
fn victims(host: &mut App) -> Vec<usize> {
    let players: Vec<Vec3> = {
        let mut q = host.world_mut().query::<&PlayerBody>();
        q.iter(host.world()).map(|b| b.0.pos).collect()
    };
    let defs = host.world().resource::<GameDefs>().0.clone();
    let combat = &host.world().resource::<LevelCombat>().0;
    let barrels: Vec<Vec3> = combat
        .actors
        .iter()
        .filter(|a| a.kind == ActorKind::Barrel)
        .map(|a| a.body.pos)
        .collect();
    let mut chosen: Vec<usize> = Vec::new();
    for (i, a) in combat.actors.iter().enumerate() {
        let clear = |p: &Vec3, d: f32| p.distance(a.body.pos) > d;
        if a.kind != ActorKind::Barrel
            && !defs.enemy(a.kind).boss
            && a.targetable()
            && players.iter().all(|p| clear(p, 8.0))
            && barrels.iter().all(|p| clear(p, 4.0))
            && chosen
                .iter()
                .all(|&j| clear(&combat.actors[j].body.pos, 4.0))
        {
            chosen.push(i);
        }
        if chosen.len() == 10 {
            break;
        }
    }
    assert_eq!(chosen.len(), 10, "the level has ten lone enemies");
    // Not index order, so the order checked is the kills'.
    chosen.reverse();
    chosen
}

#[test]
fn events_arrive_in_order_once() {
    let mut game = Game::new("hollywood_meltdown.ron");
    // The awake enemies may shoot the players meanwhile; nobody dies of it.
    {
        let world = game.host.world_mut();
        let mut q = world.query::<&mut PlayerVitals>();
        for mut v in q.iter_mut(world) {
            v.0.health.hp = 100_000;
        }
    }
    let order = victims(&mut game.host);
    let (mut on_host, mut on_client) = (Vec::new(), Vec::new());
    for (n, &i) in order.iter().enumerate() {
        let a = game.host.world().resource::<LevelCombat>().0.actors[i].body;
        let combat = &mut game.host.world_mut().resource_mut::<LevelCombat>().0;
        combat.pending_blasts.push(PendingBlast {
            fuse: 0.0,
            blast: Blast {
                center: a.pos + Vec3::Z * 0.5 * a.height,
                sector: a.sector,
                splash: SplashDef {
                    radius: 0.5,
                    damage: 100_000,
                    self_scale: 1.0,
                },
                owner: Shooter::World,
            },
            via: Via::Direct,
        });
        // One kill every third tick: each in its own tick's events.
        for _ in 0..3 {
            game.round();
            on_host.extend(killed(&game.host));
            on_client.extend(killed(&game.client));
        }
        assert_eq!(
            on_host.len(),
            n + 1,
            "the blast at actor {i} killed it alone"
        );
    }
    for _ in 0..180 {
        game.round();
        on_host.extend(killed(&game.host));
        on_client.extend(killed(&game.client));
    }
    assert_eq!(on_host, order);
    assert_eq!(on_client, order);
}

fn explosions(app: &App) -> usize {
    app.world()
        .resource::<FxQueue>()
        .combat
        .iter()
        .filter(|e| matches!(e, CombatEvent::Explosion { .. }))
        .count()
}

/// Most ticks a relayed event may be released after the host raised it: the link's 6 ticks and
/// the interpolation delay's 6 (13 in all, with the tick it is raised in), plus a reliable
/// resend (100 ms, 6 ticks) after a lost datagram, which the interpolation delay mostly hides.
/// A 300 ms resend took 25.
const MAX_RELEASE_LATENESS: usize = 20;
/// Blasts in [`lost_events_are_resent_promptly`], one every [`BLAST_EVERY`] ticks.
const BLASTS: usize = 120;
const BLAST_EVERY: usize = 6;

#[test]
fn lost_events_are_resent_promptly() {
    let mut game = Game::with_jitter("test_yard.ron", JITTER);
    game.rounds(30);
    let (at, sector) = {
        let mut q = game.host.world_mut().query::<(&Player, &PlayerBody)>();
        let (_, b) = q
            .iter(game.host.world())
            .find(|(p, _)| p.id == PlayerId(0))
            .unwrap();
        (b.0.pos + Vec3::Z, b.0.sector)
    };
    let (mut on_host, mut on_client) = (Vec::new(), Vec::new());
    for round in 0..BLASTS * BLAST_EVERY + 120 {
        if round % BLAST_EVERY == 0 && round < BLASTS * BLAST_EVERY {
            game.host
                .world_mut()
                .resource_mut::<LevelCombat>()
                .0
                .pending_blasts
                .push(PendingBlast {
                    fuse: 0.0,
                    blast: Blast {
                        center: at,
                        sector,
                        splash: SplashDef {
                            radius: 0.5,
                            damage: 0,
                            self_scale: 0.0,
                        },
                        owner: Shooter::World,
                    },
                    via: Via::Direct,
                });
        }
        game.round();
        on_host.extend(std::iter::repeat_n(round, explosions(&game.host)));
        on_client.extend(std::iter::repeat_n(round, explosions(&game.client)));
    }
    assert_eq!(on_host.len(), BLASTS);
    assert_eq!(
        on_client.len(),
        BLASTS,
        "every blast reaches the client once"
    );
    let late: Vec<usize> = on_host.iter().zip(&on_client).map(|(h, c)| c - h).collect();
    let worst = *late.iter().max().unwrap();
    assert!(
        worst <= MAX_RELEASE_LATENESS,
        "released up to {worst} ticks late: {late:?}"
    );
}
