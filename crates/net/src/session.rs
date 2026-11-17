//! Host and client sessions over renet (netcode, unsecure: LAN play, no accounts).

use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use renet::{ChannelConfig, ConnectionConfig, RenetClient, RenetServer, SendType, ServerEvent};
use renet_netcode::{
    ClientAuthentication, NetcodeClientTransport, NetcodeDisconnectReason, NetcodeServerTransport,
    ServerAuthentication, ServerConfig,
};
use rr_core::players::MAX_PLAYERS;

use crate::protocol::{
    ClientMsg, GAME_PORT, NETCODE_PROTOCOL_ID, ServerMsg, decode_client, decode_server, encode,
};

/// Everything but state: hello, welcome, refusals, roster, level loads. Reliable and ordered.
pub const CH_CONTROL: u8 = 0;
/// Snapshots and inputs: a late one is worthless, so unreliable.
pub const CH_STATE: u8 = 1;

pub type ClientKey = u64;

fn connection_config() -> ConnectionConfig {
    let channels = || {
        vec![
            ChannelConfig {
                channel_id: CH_CONTROL,
                max_memory_usage_bytes: 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(300),
                },
            },
            ChannelConfig {
                channel_id: CH_STATE,
                max_memory_usage_bytes: 256 * 1024,
                send_type: SendType::Unreliable,
            },
        ]
    };
    ConnectionConfig {
        server_channels_config: channels(),
        client_channels_config: channels(),
        ..Default::default()
    }
}

fn since_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

pub enum HostEvent {
    Connected(ClientKey),
    Disconnected(ClientKey),
    Message(ClientKey, ClientMsg),
    /// A datagram `decode_client` rejected; counted and ignored.
    Garbage(ClientKey),
}

impl fmt::Debug for HostEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostEvent::Connected(k) => write!(f, "Connected({k})"),
            HostEvent::Disconnected(k) => write!(f, "Disconnected({k})"),
            HostEvent::Message(k, m) => write!(f, "Message({k}, {m:?})"),
            HostEvent::Garbage(k) => write!(f, "Garbage({k})"),
        }
    }
}

pub struct Host {
    server: RenetServer,
    transport: NetcodeServerTransport,
    addr: SocketAddr,
}

impl Host {
    /// Open a host on `addr`. Netcode admits `MAX_PLAYERS` clients, so a fourth guest connects
    /// and the lobby can refuse it in words.
    pub fn bind(addr: SocketAddr) -> io::Result<Host> {
        let socket = UdpSocket::bind(addr)?;
        let addr = socket.local_addr()?;
        let config = ServerConfig {
            current_time: since_epoch(),
            max_clients: MAX_PLAYERS,
            protocol_id: NETCODE_PROTOCOL_ID,
            public_addresses: vec![addr],
            authentication: ServerAuthentication::Unsecure,
        };
        let transport = NetcodeServerTransport::new(config, socket)?;
        Ok(Host {
            server: RenetServer::new(connection_config()),
            transport,
            addr,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Receive and return what happened since the last call.
    pub fn update(&mut self, dt: Duration) -> Vec<HostEvent> {
        self.server.update(dt);
        // A socket error is not fatal for a hosted game: the clients time out on their own.
        let _ = self.transport.update(dt, &mut self.server);
        let mut out = Vec::new();
        let mut gone = Vec::new();
        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    out.push(HostEvent::Connected(client_id));
                }
                ServerEvent::ClientDisconnected { client_id, .. } => gone.push(client_id),
            }
        }
        for key in self.server.clients_id() {
            for ch in [CH_CONTROL, CH_STATE] {
                while let Some(bytes) = self.server.receive_message(key, ch) {
                    out.push(match decode_client(&bytes) {
                        Ok(msg) => HostEvent::Message(key, msg),
                        Err(_) => HostEvent::Garbage(key),
                    });
                }
            }
        }
        out.extend(gone.into_iter().map(HostEvent::Disconnected));
        out
    }

    pub fn send(&mut self, to: ClientKey, msg: &ServerMsg) {
        let ch = match msg {
            ServerMsg::Snapshot { .. } => CH_STATE,
            _ => CH_CONTROL,
        };
        self.server.send_message(to, ch, encode(msg));
    }

    pub fn disconnect(&mut self, who: ClientKey) {
        self.server.disconnect(who);
    }

    pub fn flush(&mut self) {
        self.transport.send_packets(&mut self.server);
    }

    /// Tell everyone the game is closed and send it now.
    pub fn shutdown(&mut self) {
        self.flush();
        self.transport.disconnect_all(&mut self.server);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostReason {
    NoAnswer,
    TimedOut,
    Denied,
    Closed,
}

impl fmt::Display for LostReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LostReason::NoAnswer => "No answer from the host",
            LostReason::TimedOut => "Connection lost",
            LostReason::Denied => "The host turned us away",
            LostReason::Closed => "The host closed the game",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    Connecting,
    Connected,
    Lost(LostReason),
}

pub struct Client {
    conn: RenetClient,
    transport: NetcodeClientTransport,
    garbage: u32,
}

impl Client {
    pub fn connect(server: SocketAddr) -> io::Result<Client> {
        let bind: IpAddr = match server.ip() {
            IpAddr::V4(ip) if ip.is_loopback() => Ipv4Addr::LOCALHOST.into(),
            IpAddr::V6(ip) if ip.is_loopback() => Ipv6Addr::LOCALHOST.into(),
            IpAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
            IpAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
        };
        let socket = UdpSocket::bind((bind, 0))?;
        let now = since_epoch();
        let client_id = (now.as_nanos() as u64).rotate_left(16) ^ u64::from(std::process::id());
        let auth = ClientAuthentication::Unsecure {
            protocol_id: NETCODE_PROTOCOL_ID,
            client_id,
            server_addr: server,
            user_data: None,
        };
        let transport = NetcodeClientTransport::new(now, auth, socket)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Client {
            conn: RenetClient::new(connection_config()),
            transport,
            garbage: 0,
        })
    }

    pub fn update(&mut self, dt: Duration) -> Vec<ServerMsg> {
        self.conn.update(dt);
        // Errors here are disconnects; `state` reports them.
        let _ = self.transport.update(dt, &mut self.conn);
        let mut out = Vec::new();
        for ch in [CH_CONTROL, CH_STATE] {
            while let Some(bytes) = self.conn.receive_message(ch) {
                match decode_server(&bytes) {
                    Ok(msg) => out.push(msg),
                    Err(_) => self.garbage += 1,
                }
            }
        }
        out
    }

    /// Datagrams from the host that did not decode.
    pub fn garbage(&self) -> u32 {
        self.garbage
    }

    pub fn send(&mut self, msg: &ClientMsg) {
        let ch = match msg {
            ClientMsg::Input(_) => CH_STATE,
            _ => CH_CONTROL,
        };
        self.conn.send_message(ch, encode(msg));
    }

    pub fn flush(&mut self) {
        // A failure means the connection is gone; `state` reports it.
        let _ = self.transport.send_packets(&mut self.conn);
    }

    pub fn state(&self) -> ClientState {
        match self.transport.disconnect_reason() {
            Some(reason) => ClientState::Lost(match reason {
                NetcodeDisconnectReason::ConnectTokenExpired
                | NetcodeDisconnectReason::ConnectionRequestTimedOut
                | NetcodeDisconnectReason::ConnectionResponseTimedOut => LostReason::NoAnswer,
                NetcodeDisconnectReason::ConnectionTimedOut => LostReason::TimedOut,
                NetcodeDisconnectReason::ConnectionDenied => LostReason::Denied,
                NetcodeDisconnectReason::DisconnectedByClient
                | NetcodeDisconnectReason::DisconnectedByServer => LostReason::Closed,
            }),
            None if self.conn.is_disconnected() => ClientState::Lost(LostReason::TimedOut),
            None if self.conn.is_connected() => ClientState::Connected,
            None => ClientState::Connecting,
        }
    }

    /// Leave now; the host sees it on its next update.
    pub fn disconnect(&mut self) {
        self.transport.disconnect();
    }
}

/// An IPv4 or IPv6 address with an optional port (default [`GAME_PORT`]). No DNS lookup: it
/// would block the menu.
pub fn parse_addr(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Some(addr);
    }
    text.parse::<IpAddr>()
        .ok()
        .map(|ip| SocketAddr::new(ip, GAME_PORT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr_forms() {
        assert_eq!(
            parse_addr("192.168.1.5"),
            Some("192.168.1.5:27960".parse().unwrap())
        );
        assert_eq!(
            parse_addr("10.0.0.2:4000"),
            Some("10.0.0.2:4000".parse().unwrap())
        );
        assert_eq!(
            parse_addr("[::1]:27960"),
            Some("[::1]:27960".parse().unwrap())
        );
        assert_eq!(parse_addr("::1"), Some("[::1]:27960".parse().unwrap()));
        assert_eq!(parse_addr(""), None);
        assert_eq!(parse_addr("host.local"), None);
        assert_eq!(parse_addr("1.2.3.4:x"), None);
    }
}
