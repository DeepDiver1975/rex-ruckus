//! Real UDP on 127.0.0.1: host and client sessions, garbage, and LAN beacons.

use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use rr_core::collide::Body;
use rr_core::combat::{Combat, level_seed};
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::glam::Vec2;
use rr_core::inventory::Inventory;
use rr_core::map::{KeySet, Map};
use rr_core::mechanics::Mechanics;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::{PlayerSnap, Snapshot};
use rr_core::vitals::Vitals;
use rr_core::weapons::Arsenal;
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
fn garbage_is_counted_not_fatal() {
    let mut host = host();
    let addr = host.local_addr();
    let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
    for junk in [&[][..], &[0xff; 3][..], &[7u8; 1200][..], b"hello host"] {
        raw.send_to(junk, addr).unwrap();
    }
    std::thread::sleep(Duration::from_millis(20));
    let _ = host.update(STEP);
    let mut client = Client::connect(addr).unwrap();
    connected(&mut host, &mut client);
    assert_eq!(client.garbage(), 0);
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
    assert_eq!(client.state(), ClientState::Lost(LostReason::Closed));
    let (_, _, ok) = pump(&mut host, &mut [], STEP, |e, _, _| {
        e.iter().any(|e| matches!(e, HostEvent::Disconnected(_)))
    });
    assert!(ok, "disconnect never seen");
}

#[test]
fn no_host_means_no_answer() {
    // A port nothing listens on: bind, note the port, drop.
    let dead: SocketAddr = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let mut client = Client::connect(dead).unwrap();
    let mut idle = host();
    let (_, _, ok) = pump(
        &mut idle,
        &mut [&mut client],
        Duration::from_millis(100),
        |_, _, s| matches!(s[0], ClientState::Lost(_)),
    );
    assert!(ok, "never gave up");
    assert_eq!(client.state(), ClientState::Lost(LostReason::NoAnswer));
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
