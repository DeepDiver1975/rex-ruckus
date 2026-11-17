//! The LAN game list of the Join screen: a [`LanListener`] is bound while the screen is shown
//! and the beacons it hears become [`LanGames`]; a game not heard for [`LAN_TIMEOUT`] is dropped.

use std::net::SocketAddr;

use bevy::prelude::*;
use rr_net::{BEACON_PORT, Beacon, LanListener};

use crate::menu::{MenuScreen, Screen};

/// Seconds a LAN game stays listed without a new beacon.
pub const LAN_TIMEOUT: f32 = 3.0;

/// A game heard on the LAN.
#[derive(Debug, Clone, PartialEq)]
pub struct LanGame {
    /// Where to connect: the sender's IP and the beacon's port.
    pub addr: SocketAddr,
    pub beacon: Beacon,
    /// Seconds since its last beacon.
    pub age: f32,
}

impl LanGame {
    /// The Join screen's button text.
    pub fn label(&self) -> String {
        let b = &self.beacon;
        format!("{} - {} ({}/{})", b.name, b.level, b.players, b.max)
    }
}

/// The LAN games the Join screen offers.
#[derive(Resource, Default)]
pub struct LanGames {
    /// Bound while the Join screen is shown.
    listener: Option<LanListener>,
    /// The last bind failed: the screen says so (and keeps the address field).
    pub unavailable: bool,
    pub games: Vec<LanGame>,
}

impl LanGames {
    /// Ages the list by `dt`, takes in `heard` and drops games older than [`LAN_TIMEOUT`].
    /// Returns whether the list as shown (addresses and labels) changed.
    pub fn update(&mut self, heard: Vec<(SocketAddr, Beacon)>, dt: f32) -> bool {
        let before: Vec<(SocketAddr, String)> = self.shown();
        for game in &mut self.games {
            game.age += dt;
        }
        for (addr, beacon) in heard {
            match self.games.iter_mut().find(|g| g.addr == addr) {
                Some(game) => {
                    game.beacon = beacon;
                    game.age = 0.0;
                }
                None => self.games.push(LanGame {
                    addr,
                    beacon,
                    age: 0.0,
                }),
            }
        }
        self.games.retain(|g| g.age < LAN_TIMEOUT);
        self.shown() != before
    }

    fn shown(&self) -> Vec<(SocketAddr, String)> {
        self.games.iter().map(|g| (g.addr, g.label())).collect()
    }
}

/// Listens for beacons while the Join screen is shown; a change of the list rebuilds the screen.
pub fn browse_lan(time: Res<Time>, mut lan: ResMut<LanGames>, mut screen: ResMut<MenuScreen>) {
    if screen.0 != Some(Screen::Join) {
        if lan.listener.is_some() || lan.unavailable || !lan.games.is_empty() {
            *lan = LanGames::default();
        }
        return;
    }
    if lan.listener.is_none() && !lan.unavailable {
        match LanListener::bind(BEACON_PORT) {
            Ok(listener) => lan.listener = Some(listener),
            Err(e) => {
                warn!("no LAN search: {e}");
                lan.unavailable = true;
                screen.set_changed();
                return;
            }
        }
    }
    let lan = lan.bypass_change_detection();
    let heard = lan
        .listener
        .as_mut()
        .map(LanListener::poll)
        .unwrap_or_default();
    if lan.update(heard, time.delta_secs()) {
        screen.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_net::{BEACON_MAGIC, PROTOCOL_VERSION};

    fn beacon(name: &str, players: u8) -> Beacon {
        Beacon {
            magic: BEACON_MAGIC,
            protocol: PROTOCOL_VERSION,
            name: name.into(),
            level: "rubble_run.ron".into(),
            players,
            max: 4,
            port: 27960,
        }
    }

    #[test]
    fn games_are_listed_refreshed_and_dropped_after_silence() {
        let a: SocketAddr = "10.0.0.2:27960".parse().unwrap();
        let b: SocketAddr = "10.0.0.3:27960".parse().unwrap();
        let mut lan = LanGames::default();
        assert!(lan.update(vec![(a, beacon("Ann", 1))], 0.0));
        assert_eq!(lan.games[0].label(), "Ann - rubble_run.ron (1/4)");
        assert!(!lan.update(vec![(a, beacon("Ann", 1))], 1.0), "same list");
        assert!(lan.update(vec![(a, beacon("Ann", 2)), (b, beacon("Bo", 1))], 1.0));
        assert_eq!(lan.games.len(), 2);
        // Ann keeps calling, Bo goes quiet.
        for _ in 0..2 {
            assert!(!lan.update(vec![(a, beacon("Ann", 2))], 1.0));
        }
        assert!(lan.update(vec![(a, beacon("Ann", 2))], 1.0));
        assert_eq!(lan.games.len(), 1);
        assert_eq!(lan.games[0].addr, a);
    }
}
