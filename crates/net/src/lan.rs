//! LAN discovery: a host announces itself with [`Beacon`]s, a browser listens for them.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

use crate::protocol::{BEACON_PORT, Beacon, decode, encode};

/// Sends beacons from a broadcast-enabled socket.
pub struct BeaconSender {
    socket: UdpSocket,
}

impl BeaconSender {
    pub fn bind() -> io::Result<BeaconSender> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        Ok(BeaconSender { socket })
    }

    /// Send to one address (loopback tests, or a directed broadcast). Errors are dropped: a
    /// beacon is a best-effort hint.
    pub fn send_to(&mut self, beacon: &Beacon, to: SocketAddr) {
        let _ = self.socket.send_to(&encode(beacon), to);
    }

    pub fn broadcast(&mut self, beacon: &Beacon) {
        self.send_to(beacon, (Ipv4Addr::BROADCAST, BEACON_PORT).into());
    }
}

/// Collects beacons on a UDP port without blocking.
pub struct LanListener {
    socket: UdpSocket,
}

impl LanListener {
    pub fn bind(port: u16) -> io::Result<LanListener> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
        socket.set_nonblocking(true)?;
        Ok(LanListener { socket })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.socket
            .local_addr()
            .unwrap_or_else(|_| (Ipv4Addr::UNSPECIFIED, 0).into())
    }

    /// Every compatible beacon waiting, with the game address (sender's IP, `beacon.port`).
    /// Bad datagrams and other games' beacons are skipped.
    pub fn poll(&mut self) -> Vec<(SocketAddr, Beacon)> {
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if let Ok(beacon) = decode::<Beacon>(&buf[..n])
                        && beacon.is_ours()
                    {
                        out.push((SocketAddr::new(from.ip(), beacon.port), beacon));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // WouldBlock, or a real error: nothing more now.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(_) => break,
            }
        }
        out
    }
}
