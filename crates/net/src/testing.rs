//! Test helpers: [`LossyLink`], a UDP relay between one client and a server that delays and
//! drops datagrams, so tests can play over a bad network on 127.0.0.1.

use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use rr_core::rng::Rng;

/// Larger than any datagram netcode sends.
const MAX_DATAGRAM: usize = 2048;

/// A datagram waiting for its time.
struct Held {
    due: Duration,
    to_server: bool,
    bytes: Vec<u8>,
}

/// A UDP relay for tests. The client connects to [`LossyLink::addr`]; the server sees the
/// link's socket as the client's address. Every datagram, either way, is dropped with
/// probability `loss` (a seeded SplitMix64, so a run repeats) or held `delay` and sent on.
/// Time is passed to [`LossyLink::pump`], never read from the clock. One client per link.
pub struct LossyLink {
    socket: UdpSocket,
    addr: SocketAddr,
    server: SocketAddr,
    /// The client's address, learnt from its first datagram.
    client: Option<SocketAddr>,
    delay: Duration,
    loss: f32,
    rng: Rng,
    /// Oldest first: with one delay and a clock that never runs back, also in due order.
    held: VecDeque<Held>,
}

impl LossyLink {
    pub fn new(server: SocketAddr, delay: Duration, loss: f32, seed: u64) -> io::Result<LossyLink> {
        let bind: IpAddr = match server.ip() {
            IpAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
            IpAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
        };
        let socket = UdpSocket::bind((bind, 0))?;
        socket.set_nonblocking(true)?;
        let addr = socket.local_addr()?;
        Ok(LossyLink {
            socket,
            addr,
            server,
            client: None,
            delay,
            loss,
            rng: Rng::new(seed),
            held: VecDeque::new(),
        })
    }

    /// Where the client connects.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Takes in what arrived (at `now`) and sends on what is due by `now`. `now` must not run
    /// back between calls.
    pub fn pump(&mut self, now: Duration) {
        let mut buf = [0u8; MAX_DATAGRAM];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let to_server = from != self.server;
                    if to_server {
                        self.client = Some(from);
                    }
                    if !self.rng.chance(self.loss) {
                        self.held.push_back(Held {
                            due: now + self.delay,
                            to_server,
                            bytes: buf[..n].to_vec(),
                        });
                    }
                }
                // Windows reports an earlier send's ICMP "port unreachable" here.
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused
                    ) => {}
                Err(_) => break,
            }
        }
        while self.held.front().is_some_and(|h| h.due <= now) {
            let h = self.held.pop_front().expect("a held datagram");
            let to = if h.to_server {
                Some(self.server)
            } else {
                self.client
            };
            if let Some(to) = to {
                // Lost like any datagram if the far side is gone.
                let _ = self.socket.send_to(&h.bytes, to);
            }
        }
    }
}
