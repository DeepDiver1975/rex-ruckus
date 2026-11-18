//! Real UDP on 127.0.0.1: host and client sessions, garbage, LAN beacons and the lossy link.

use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use rr_core::collide::Body;
use rr_core::combat::{Combat, level_seed};
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::glam::{Vec2, Vec3};
use rr_core::inventory::Inventory;
use rr_core::map::{KeySet, Map};
use rr_core::mechanics::Mechanics;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::projectile::{Projectile, Shooter, Targets};
use rr_core::snapshot::{PlayerSnap, Snapshot};
use rr_core::vitals::Vitals;
use rr_core::weapons::Arsenal;
use rr_net::testing::LossyLink;
use rr_net::*;

const STEP: Duration = Duration::from_millis(16);

fn host() -> Host {
    Host::bind("127.0.0.1:0".parse().unwrap()).expect("bind host")
}

/// Pump the host and the clients (300 rounds at most, 1 ms apart) until `done` returns true.
/// Returns the host events and each client's messages collected on the way, and whether it
/// finished.
fn pump(
    host: &mut Host,
    clients: &mut [&mut Client],
    step: Duration,
    done: impl Fn(&[HostEvent], &[Vec<ServerMsg>], &[ClientState]) -> bool,
) -> (Vec<HostEvent>, Vec<Vec<ServerMsg>>, bool) {
    let mut events = Vec::new();
    let mut msgs: Vec<Vec<ServerMsg>> = clients.iter().map(|_| Vec::new()).collect();
    for _ in 0..300 {
        events.extend(host.update(step));
        for (i, c) in clients.iter_mut().enumerate() {
            msgs[i].extend(c.update(step));
            c.flush();
        }
        host.flush();
        let states: Vec<ClientState> = clients.iter().map(|c| c.state()).collect();
        if done(&events, &msgs, &states) {
            return (events, msgs, true);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    (events, msgs, false)
}

fn connected(host: &mut Host, client: &mut Client) -> ClientKey {
    let (events, _, ok) = pump(host, &mut [client], STEP, |e, _, s| {
        s[0] == ClientState::Connected && e.iter().any(|e| matches!(e, HostEvent::Connected(_)))
    });
    assert!(ok, "never connected");
    match events.first() {
        Some(HostEvent::Connected(k)) => *k,
        other => panic!("first event {other:?}"),
    }
}

fn hello() -> ClientMsg {
    ClientMsg::Hello(Hello {
        protocol: PROTOCOL_VERSION,
        game_version: "0.2.0".into(),
        assets: 99,
        name: "Rex".into(),
    })
}

fn undertow_snapshot() -> ServerMsg {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/undertow.ron");
    let source = Map::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut map = source.clone();
    let mech = Mechanics::new(&mut map);
    let defs = Defs::builtin();
    let mut combat = Combat::spawn(&map, &defs, level_seed(&map.name));
    combat.skill = Difficulty::Normal;
    let body = Body::spawn(&map, Vec2::new(0.0, 0.0), 0.35, 1.8).expect("player spot");
    for i in 0..32 {
        let at = Vec3::new(0.0, 0.0, 1.0);
        combat.projectiles.push(Projectile {
            id: 100 + i,
            pos: at,
            prev: at,
            vel: Vec3::X * 20.0,
            sector: 0,
            radius: 0.1,
            damage: 10,
            owner: Shooter::Actor(0),
            targets: Targets::Players,
            life: 5.0,
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        });
    }
    let players = (0..MAX_PLAYERS as u8)
        .map(|i| PlayerSnap {
            id: PlayerId(i),
            body,
            angle: 0.5,
            pitch: -0.25,
            vitals: Vitals::new(),
            arsenal: Arsenal::new(&defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 1000 + u32::from(i),
        })
        .collect();
    ServerMsg::Snapshot {
        you: PlayerId(1),
        snap: Snapshot::capture(7, &combat, &mech, &map, &source, players),
    }
}

#[test]
fn hello_reaches_the_host_and_welcome_comes_back() {
    let mut host = host();
    let mut client = Client::connect(host.local_addr()).unwrap();
    client.send(&hello());
    let (events, _, ok) = pump(&mut host, &mut [&mut client], STEP, |e, _, _| {
        e.iter()
            .any(|e| matches!(e, HostEvent::Message(_, ClientMsg::Hello(_))))
    });
    assert!(ok, "hello never arrived");
    let Some(HostEvent::Connected(key)) = events.first() else {
        panic!("first event {:?}", events.first());
    };
    assert!(matches!(events[1], HostEvent::Message(k, ClientMsg::Hello(_)) if k == *key));
    let welcome = ServerMsg::Roster(vec!["Rex".into()]);
    host.send(*key, &welcome);
    host.flush();
    let (_, msgs, ok) = pump(&mut host, &mut [&mut client], STEP, |_, m, _| {
        !m[0].is_empty()
    });
    assert!(ok);
    assert_eq!(msgs[0][0], welcome);
    assert_eq!(client.state(), ClientState::Connected);
}

#[test]
fn snapshots_go_unreliable_and_arrive_whole() {
    let mut host = host();
    let mut client = Client::connect(host.local_addr()).unwrap();
    let key = connected(&mut host, &mut client);
    let snap = undertow_snapshot();
    assert!(encode(&snap).len() > 1200, "must be sliced");
    // Unreliable: loopback should not drop, but allow a few tries.
    let mut got = None;
    for _ in 0..10 {
        host.send(key, &snap);
        host.flush();
        let (_, mut msgs, ok) = pump(&mut host, &mut [&mut client], STEP, |_, m, _| {
            !m[0].is_empty()
        });
        if ok {
            got = msgs.remove(0).into_iter().next();
            break;
        }
    }
    assert_eq!(got, Some(snap));
}

#[test]
fn client_clock_behind_the_host_still_joins() {
    // Unsecure netcode stamps the connect token with the client's clock (+300 s); a host that
    // compared it with its own wall clock dropped every request from a client 5+ minutes behind.
    let mut host = host();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let mut clients = Vec::new();
    for skew in [
        Duration::from_secs(600),
        Duration::from_secs(400 * 24 * 3600),
    ] {
        let mut client = Client::connect_at(host.local_addr(), now - skew).unwrap();
        connected(&mut host, &mut client);
        clients.push(client);
    }
}

#[test]
fn junk_udp_is_ignored() {
    let mut host = host();
    let addr = host.local_addr();
    let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
    for junk in [&[][..], &[0xff; 3][..], &[7u8; 1200][..], b"hello host"] {
        raw.send_to(junk, addr).unwrap();
    }
    std::thread::sleep(Duration::from_millis(20));
    assert!(host.update(STEP).is_empty());
    let mut client = Client::connect(addr).unwrap();
    connected(&mut host, &mut client);
}

#[test]
fn undecodable_message_is_garbage_not_fatal() {
    let mut host = host();
    let mut client = Client::connect(host.local_addr()).unwrap();
    let key = connected(&mut host, &mut client);
    client.send_raw(vec![0xff; 40]);
    client.send(&hello());
    let (events, _, ok) = pump(&mut host, &mut [&mut client], STEP, |e, _, _| {
        e.iter()
            .any(|e| matches!(e, HostEvent::Message(_, ClientMsg::Hello(_))))
    });
    assert!(ok);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HostEvent::Garbage(k) if *k == key))
    );
}

#[test]
fn undecodable_control_message_is_counted_apart() {
    let mut host = host();
    let mut client = Client::connect(host.local_addr()).unwrap();
    let key = connected(&mut host, &mut client);
    host.send_raw(key, vec![0xff; 40]);
    host.send(key, &ServerMsg::Roster(vec!["Rex".into()]));
    let (_, msgs, ok) = pump(&mut host, &mut [&mut client], STEP, |_, m, _| {
        !m[0].is_empty()
    });
    assert!(ok);
    assert_eq!(client.garbage(), 1);
    assert_eq!(client.control_garbage(), 1);
    assert!(matches!(msgs[0][..], [ServerMsg::Roster(_)]));
}

#[test]
fn fourth_client_still_connects() {
    let mut host = host();
    let addr = host.local_addr();
    let mut cs: Vec<Client> = (0..4).map(|_| Client::connect(addr).unwrap()).collect();
    let mut refs: Vec<&mut Client> = cs.iter_mut().collect();
    let (_, _, ok) = pump(&mut host, &mut refs, STEP, |_, _, s| {
        s.iter().all(|s| *s == ClientState::Connected)
    });
    assert!(ok, "not all four connected");
}

#[test]
fn client_disconnect_is_seen() {
    let mut host = host();
    let mut client = Client::connect(host.local_addr()).unwrap();
    connected(&mut host, &mut client);
    client.disconnect();
    assert_eq!(client.state(), ClientState::Lost(LostReason::Left));
    let (_, _, ok) = pump(&mut host, &mut [], STEP, |e, _, _| {
        e.iter().any(|e| matches!(e, HostEvent::Disconnected(_)))
    });
    assert!(ok, "disconnect never seen");
}

#[test]
fn no_host_means_no_answer() {
    // Keep the socket bound (so the port cannot be reused) but never read it.
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut client = Client::connect(silent.local_addr().unwrap()).unwrap();
    let mut idle = host();
    let (_, _, ok) = pump(
        &mut idle,
        &mut [&mut client],
        Duration::from_millis(100),
        |_, _, s| matches!(s[0], ClientState::Lost(_)),
    );
    assert!(ok, "never gave up");
    assert_eq!(client.state(), ClientState::Lost(LostReason::NoAnswer));
    drop(silent);
}

fn beacon(magic: u32, protocol: u16) -> Beacon {
    Beacon {
        magic,
        protocol,
        name: "Rex's game".into(),
        level: "undertow.ron".into(),
        players: 1,
        max: 4,
        port: 4242,
    }
}

#[test]
fn beacons_round_trip() {
    let mut listener = LanListener::bind(0).unwrap();
    let to: SocketAddr = format!("127.0.0.1:{}", listener.local_addr().port())
        .parse()
        .unwrap();
    let mut sender = BeaconSender::bind().unwrap();
    let good = beacon(BEACON_MAGIC, PROTOCOL_VERSION);
    sender.send_to(&beacon(BEACON_MAGIC ^ 1, PROTOCOL_VERSION), to);
    sender.send_to(&beacon(BEACON_MAGIC, PROTOCOL_VERSION + 1), to);
    sender.send_to(
        &Beacon {
            level: "../../x.ron".into(),
            ..beacon(BEACON_MAGIC, PROTOCOL_VERSION)
        },
        to,
    );
    sender.send_to(&good, to);
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"junk", to)
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let found = listener.poll();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "127.0.0.1:4242".parse().unwrap());
    assert_eq!(found[0].1, good);
}

const DELAY: Duration = Duration::from_millis(100);

#[test]
fn lossy_link_delays_and_drops() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let mut link = LossyLink::new(server.local_addr().unwrap(), DELAY, 0.05, 7).unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    // Datagram `i` leaves at `i` ms; one link pump per millisecond of link time.
    let mut sent_at = Vec::new();
    let mut arrived = Vec::new();
    let mut buf = [0u8; 64];
    let via = link.addr();
    let mut drain = |now: Duration, arrived: &mut Vec<(u32, Duration)>| {
        while let Ok((n, from)) = server.recv_from(&mut buf) {
            assert_eq!(from, via, "the server sees the link as the client");
            let i = u32::from_le_bytes(buf[..n].try_into().unwrap());
            arrived.push((i, now));
        }
    };
    let total = 1000u32;
    for ms in 0..total + 200 {
        let now = Duration::from_millis(u64::from(ms));
        if ms < total {
            client.send_to(&ms.to_le_bytes(), via).unwrap();
            sent_at.push(now);
        }
        link.pump(now);
        drain(now, &mut arrived);
    }
    std::thread::sleep(Duration::from_millis(20));
    drain(Duration::from_millis(u64::from(total + 200)), &mut arrived);
    let n = arrived.len();
    assert!((930..=970).contains(&n), "{n} of {total} arrived");
    for (i, at) in &arrived {
        assert!(
            *at >= sent_at[*i as usize] + DELAY,
            "datagram {i} early at {at:?}"
        );
    }
    let mut ids: Vec<u32> = arrived.iter().map(|(i, _)| *i).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "no datagram arrives twice");
}

#[test]
fn session_survives_loss() {
    let mut host = host();
    let mut link = LossyLink::new(host.local_addr(), DELAY, 0.05, 7).unwrap();
    let mut client = Client::connect(link.addr()).unwrap();
    client.send(&hello());
    let mut now = Duration::ZERO;
    let mut events = Vec::new();
    let mut got = Vec::new();
    let mut welcomed = false;
    let welcome = ServerMsg::Roster(vec!["Rex".into()]);
    for _ in 0..600 {
        for e in host.update(STEP) {
            if let HostEvent::Message(key, ClientMsg::Hello(_)) = e {
                host.send(key, &welcome);
                welcomed = true;
            }
            events.push(e);
        }
        host.flush();
        link.pump(now);
        got.extend(client.update(STEP));
        client.flush();
        link.pump(now);
        now += STEP;
        if !got.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(welcomed, "hello never arrived: {events:?}");
    assert_eq!(got, [welcome]);
    assert_eq!(client.state(), ClientState::Connected);
}
