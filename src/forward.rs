//! Session-owned local listeners and bounded bidirectional channel relays.
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    str::FromStr,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalForward {
    pub bind: String,
    pub port: u16,
    pub host: String,
    pub destination_port: u16,
}

fn matches_remote_bind(bind: &str, address: &str) -> bool {
    if bind == address {
        return true;
    }
    match (bind, address.parse::<IpAddr>()) {
        ("0.0.0.0", Ok(IpAddr::V4(_))) | ("::", Ok(IpAddr::V6(_))) => true,
        ("localhost", Ok(ip)) => ip.is_loopback(),
        _ => false,
    }
}

impl FromStr for LocalForward {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        let mut fields = Vec::new();
        let mut start = 0;
        let mut bracket = false;
        for (index, ch) in value.char_indices() {
            match ch {
                '[' if !bracket => bracket = true,
                ']' if bracket => bracket = false,
                ':' if !bracket => {
                    fields.push(&value[start..index]);
                    start = index + 1;
                }
                '[' | ']' => return Err("Invalid forwarding address brackets.".into()),
                _ => {}
            }
        }
        if bracket {
            return Err("Unclosed forwarding address bracket.".into());
        }
        fields.push(&value[start..]);
        let (bind, port, host, target) =
            match fields.as_slice() {
                [port, host, target] => ("127.0.0.1", *port, *host, *target),
                [bind, port, host, target] => (*bind, *port, *host, *target),
                _ => return Err(
                    "Local forward must be [bind-address:]port:host:port; bracket IPv6 addresses."
                        .into(),
                ),
            };
        let address = |value: &str| -> Result<String, String> {
            let value = value
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(value);
            if value.is_empty()
                || value
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '[' | ']'))
            {
                return Err("Invalid forwarding host.".into());
            }
            Ok(value.to_owned())
        };
        let parse_port = |value: &str| {
            value
                .parse::<u16>()
                .map_err(|_| "Invalid forwarding port.".to_owned())
        };
        let destination_port = parse_port(target)?;
        if destination_port == 0 {
            return Err("Forward destination port must be nonzero.".into());
        }
        Ok(Self {
            bind: address(if bind == "*" { "0.0.0.0" } else { bind })?,
            port: parse_port(port)?,
            host: address(host)?,
            destination_port,
        })
    }
}

/// Brackets addresses that contain `:` (IPv6) so specs parse back unchanged.
fn spec_address(address: &str) -> std::borrow::Cow<'_, str> {
    if address.contains(':') {
        format!("[{address}]").into()
    } else {
        address.into()
    }
}

/// The full `bind:port:host:port` spec, which parses back to the same forward.
impl std::fmt::Display for LocalForward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            spec_address(&self.bind),
            self.port,
            spec_address(&self.host),
            self.destination_port
        )
    }
}

pub struct Forwarder<S: Read + Write> {
    listeners: Vec<(TcpListener, Option<LocalForward>)>,
    relays: Vec<Relay<S>>,
    negotiations: Vec<Negotiation<S>>,
    remote: Vec<(LocalForward, Vec<SocketAddr>)>,
    connecting: Vec<RemoteConnect<S>>,
}
struct RemoteConnect<S: Read + Write> {
    channel: Option<ssh::LocalDirectTcpip<S>>,
    result: mpsc::Receiver<io::Result<TcpStream>>,
}
impl<S: Read + Write> Drop for RemoteConnect<S> {
    fn drop(&mut self) {
        if let Some(channel) = &mut self.channel {
            channel.close_now();
        }
    }
}
struct Negotiation<S: Read + Write> {
    handshake: Option<crate::socks::Handshake>,
    channel: Option<ssh::LocalDirectTcpip<S>>,
}
impl<S: Read + Write> Drop for Negotiation<S> {
    fn drop(&mut self) {
        if let Some(channel) = &mut self.channel {
            channel.close_now();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicForward {
    pub bind: String,
    pub port: u16,
}
/// The full `bind:port` spec, which parses back to the same forward.
impl std::fmt::Display for DynamicForward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", spec_address(&self.bind), self.port)
    }
}
impl FromStr for DynamicForward {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        let spec: LocalForward = format!("{value}:unused:1").parse()?;
        Ok(Self {
            bind: spec.bind,
            port: spec.port,
        })
    }
}
struct Relay<S: Read + Write> {
    socket: TcpStream,
    channel: ssh::LocalDirectTcpip<S>,
    outgoing: Vec<u8>,
    offset: usize,
    input_eof: bool,
    remote_eof: bool,
    output_shutdown: bool,
    incoming: Vec<u8>,
    sent: usize,
}
impl<S: Read + Write> Drop for Relay<S> {
    fn drop(&mut self) {
        self.channel.close_now();
    }
}

impl<S: Read + Write> Forwarder<S> {
    pub fn bind(specs: &[LocalForward]) -> Result<Self, String> {
        let mut listeners = Vec::new();
        for spec in specs {
            let listener = TcpListener::bind((spec.bind.as_str(), spec.port)).map_err(|e| {
                format!("Cannot bind local forward {}:{}: {e}", spec.bind, spec.port)
            })?;
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            listeners.push((listener, Some(spec.clone())));
        }
        Ok(Self {
            listeners,
            relays: Vec::new(),
            negotiations: Vec::new(),
            remote: Vec::new(),
            connecting: Vec::new(),
        })
    }
    pub fn addresses(&self) -> Vec<String> {
        self.listeners
            .iter()
            .filter(|(_, spec)| spec.is_some())
            .filter_map(|(listener, _)| {
                listener
                    .local_addr()
                    .ok()
                    .map(|address| address.to_string())
            })
            .collect()
    }
    pub fn bind_dynamic(&mut self, specs: &[DynamicForward]) -> Result<Vec<String>, String> {
        let mut addresses = Vec::new();
        for spec in specs {
            let listener = TcpListener::bind((spec.bind.as_str(), spec.port))
                .map_err(|e| format!("Cannot bind SOCKS listener: {e}"))?;
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            addresses.push(
                listener
                    .local_addr()
                    .map_err(|e| e.to_string())?
                    .to_string(),
            );
            self.listeners.push((listener, None));
        }
        Ok(addresses)
    }
    pub fn bind_remote(
        &mut self,
        session: &mut ssh::LocalSession<S>,
        specs: &[LocalForward],
    ) -> Result<Vec<String>, String> {
        let mut addresses = Vec::new();
        for spec in specs {
            let destinations: Vec<_> = (spec.host.as_str(), spec.destination_port)
                .to_socket_addrs()
                .map_err(|e| format!("Cannot resolve remote forward destination: {e}"))?
                .take(16)
                .collect();
            if destinations.is_empty() {
                return Err("Remote forward destination has no addresses.".into());
            }
            let mut bound = spec.clone();
            bound.port = session
                .request_tcpip_forward(&spec.bind, spec.port)
                .map_err(|e| e.to_string())?;
            if self
                .remote
                .iter()
                .any(|(other, _)| other.bind == bound.bind && other.port == bound.port)
            {
                return Err("Server returned a duplicate remote listener.".into());
            }
            addresses.push(format!(
                "{}:{}",
                if bound.bind.contains(':') {
                    format!("[{}]", bound.bind)
                } else {
                    bound.bind.clone()
                },
                bound.port
            ));
            self.remote.push((bound, destinations));
        }
        Ok(addresses)
    }
    pub fn poll(&mut self, session: &mut ssh::LocalSession<S>) -> Vec<String> {
        let mut errors = Vec::new();
        for _ in 0..64 {
            let Some((mut channel, address, port)) =
                session.take_pending_forwarded_tcpip_with_destination()
            else {
                break;
            };
            let destination = self
                .remote
                .iter()
                .find(|(spec, _)| u32::from(spec.port) == port && spec.bind == address)
                .or_else(|| {
                    self.remote.iter().find(|(spec, _)| {
                        u32::from(spec.port) == port && matches_remote_bind(&spec.bind, &address)
                    })
                });
            if self.relays.len() + self.negotiations.len() + self.connecting.len() >= 64
                || destination.is_none()
            {
                channel.close_now();
                errors.push("Rejected an unconfigured or excess remote forward connection.".into());
                continue;
            }
            let addresses = destination.unwrap().1.clone();
            let (sender, result) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("ssh4-forward-connect".into())
                .spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut result = Err(io::Error::from(io::ErrorKind::TimedOut));
                    for address in addresses {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        result =
                            TcpStream::connect_timeout(&address, remaining).and_then(|socket| {
                                socket.set_nonblocking(true)?;
                                Ok(socket)
                            });
                        if result.is_ok() {
                            break;
                        }
                    }
                    let _ = sender.send(result);
                }) {
                Ok(_) => self.connecting.push(RemoteConnect {
                    channel: Some(channel),
                    result,
                }),
                Err(error) => {
                    channel.close_now();
                    errors.push(format!("Cannot start remote forward connection: {error}"));
                }
            }
        }
        let mut index = 0;
        while index < self.connecting.len() {
            match self.connecting[index].result.try_recv() {
                Ok(Ok(socket)) => {
                    let mut pending = self.connecting.swap_remove(index);
                    self.relays
                        .push(Relay::new(socket, pending.channel.take().unwrap()));
                }
                Err(mpsc::TryRecvError::Empty) => index += 1,
                result => {
                    errors.push(match result {
                        Ok(Err(error)) => format!("Remote forward destination failed: {error}"),
                        _ => "Remote forward connector stopped.".into(),
                    });
                    self.connecting.swap_remove(index);
                }
            }
        }
        for (listener, spec) in &self.listeners {
            if self.relays.len() + self.negotiations.len() + self.connecting.len() >= 64 {
                break;
            }
            match listener.accept() {
                Ok((socket, _)) => {
                    let Some(spec) = spec else {
                        match crate::socks::Handshake::new(socket) {
                            Ok(handshake) => self.negotiations.push(Negotiation {
                                handshake: Some(handshake),
                                channel: None,
                            }),
                            Err(error) => errors.push(error.to_string()),
                        }
                        continue;
                    };
                    match socket
                        .set_nonblocking(true)
                        .map_err(|e| e.to_string())
                        .and_then(|_| {
                            session
                                .open_direct_tcpip(&spec.host, spec.destination_port)
                                .map_err(|e| e.to_string())
                        }) {
                        Ok(channel) => self.relays.push(Relay::new(socket, channel)),
                        Err(error) => errors.push(format!("Local forward channel failed: {error}")),
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => errors.push(format!("Local forward accept failed: {error}")),
            }
        }
        let mut pending = 0;
        while pending < self.negotiations.len() {
            let negotiation = &mut self.negotiations[pending];
            let handshake = negotiation.handshake.as_mut().unwrap();
            match handshake.poll() {
                Ok(Some((host, port))) => {
                    match session.open_direct_tcpip(&host, port) {
                        Ok(channel) => {
                            negotiation.channel = Some(channel);
                            handshake.complete(true);
                        }
                        Err(error) => {
                            errors.push(format!("SOCKS channel failed: {error}"));
                            handshake.complete(false);
                        }
                    }
                    pending += 1;
                }
                Ok(None) if handshake.ready() => {
                    let mut negotiation = self.negotiations.swap_remove(pending);
                    let handshake = negotiation.handshake.take().unwrap();
                    let mut relay =
                        Relay::new(handshake.socket, negotiation.channel.take().unwrap());
                    // Client bytes that arrived with the SOCKS request go first.
                    relay.incoming = handshake.buffered;
                    self.relays.push(relay);
                }
                Ok(None) => pending += 1,
                Err(_) => {
                    self.negotiations.swap_remove(pending);
                }
            }
        }
        let mut index = 0;
        while index < self.relays.len() {
            match self.relays[index].poll() {
                Ok(true) => index += 1,
                result => {
                    if let Err(error) = result {
                        errors.push(format!("Local forward closed: {error}"));
                    }
                    self.relays.swap_remove(index);
                }
            }
        }
        errors
    }
}
impl<S: Read + Write> Relay<S> {
    fn new(socket: TcpStream, channel: ssh::LocalDirectTcpip<S>) -> Self {
        Self {
            socket,
            channel,
            outgoing: Vec::new(),
            offset: 0,
            input_eof: false,
            remote_eof: false,
            output_shutdown: false,
            incoming: Vec::new(),
            sent: 0,
        }
    }
    fn poll(&mut self) -> io::Result<bool> {
        let mut buffer = [0; 8192];
        if !self.input_eof && self.sent == self.incoming.len() {
            self.incoming.clear();
            self.sent = 0;
            match self.socket.read(&mut buffer) {
                Ok(0) => {
                    self.input_eof = true;
                    self.channel
                        .send_eof()
                        .map_err(|e| io::Error::other(e.to_string()))?;
                }
                Ok(length) => self.incoming.extend_from_slice(&buffer[..length]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if self.sent < self.incoming.len() {
            match self.channel.try_write(&self.incoming[self.sent..]) {
                Ok(length) => self.sent += length,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if self.offset == self.outgoing.len() && !self.remote_eof {
            self.outgoing.clear();
            self.offset = 0;
            match self.channel.read(&mut buffer) {
                Ok(0) => self.remote_eof = true,
                Ok(length) => self.outgoing.extend_from_slice(&buffer[..length]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if self.offset < self.outgoing.len() {
            match self.socket.write(&self.outgoing[self.offset..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(length) => self.offset += length,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if self.remote_eof && self.offset == self.outgoing.len() {
            if !self.output_shutdown {
                self.socket.shutdown(std::net::Shutdown::Write)?;
                self.output_shutdown = true;
            }
            if self.input_eof || self.channel.remote_closed() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_bind_matching_preserves_address_families_and_loopback_scope() {
        assert!(matches_remote_bind("0.0.0.0", "127.0.0.1"));
        assert!(matches_remote_bind("::", "::1"));
        assert!(matches_remote_bind("localhost", "127.0.0.2"));
        assert!(matches_remote_bind("localhost", "::1"));
        assert!(!matches_remote_bind("0.0.0.0", "::1"));
        assert!(!matches_remote_bind("::", "127.0.0.1"));
        assert!(!matches_remote_bind("localhost", "192.0.2.1"));
        assert!(!matches_remote_bind("127.0.0.1", "127.0.0.2"));
        assert!(!matches_remote_bind("0.0.0.0", "unconfigured.test"));
    }
    #[test]
    fn parses_ipv6_and_defaults_to_loopback_without_ambiguous_ports() {
        let spec: LocalForward = "8080:database:5432".parse().unwrap();
        assert_eq!(spec.bind, "127.0.0.1");
        assert_eq!(spec.port, 8080);
        let spec: LocalForward = "[::1]:0:[2001:db8::1]:443".parse().unwrap();
        assert_eq!(spec.bind, "::1");
        assert_eq!(spec.host, "2001:db8::1");
        for invalid in [
            "1:host:0",
            "65536:host:80",
            "1:host",
            "[::1:1:host:80",
            "1:bad host:80",
        ] {
            assert!(invalid.parse::<LocalForward>().is_err(), "{invalid}");
        }
    }
    #[test]
    fn displayed_specs_parse_back_to_the_same_forward() {
        for (spec, shown) in [
            ("8080:database:5432", "127.0.0.1:8080:database:5432"),
            ("*:80:web:8080", "0.0.0.0:80:web:8080"),
            ("[::1]:0:[2001:db8::1]:443", "[::1]:0:[2001:db8::1]:443"),
        ] {
            let forward: LocalForward = spec.parse().unwrap();
            assert_eq!(forward.to_string(), shown);
            assert_eq!(shown.parse::<LocalForward>().unwrap(), forward);
        }
        for (spec, shown) in [("1080", "127.0.0.1:1080"), ("[::]:1080", "[::]:1080")] {
            let forward: DynamicForward = spec.parse().unwrap();
            assert_eq!(forward.to_string(), shown);
            assert_eq!(shown.parse::<DynamicForward>().unwrap(), forward);
        }
    }
    #[test]
    fn listener_lifetime_matches_forwarder_and_bind_failure_is_reported() {
        let forwarder = Forwarder::<TcpStream>::bind(&["0:host:80".parse().unwrap()]).unwrap();
        let address = forwarder.addresses().remove(0);
        assert!(TcpListener::bind(&address).is_err());
        let occupied: LocalForward = format!("{address}:host:80").parse().unwrap();
        assert!(Forwarder::<TcpStream>::bind(&[occupied]).is_err());
        drop(forwarder);
        assert!(TcpListener::bind(&address).is_ok());
    }
}
