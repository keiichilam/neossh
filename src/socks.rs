//! Incremental SOCKS5 CONNECT negotiation; destination DNS stays on the SSH server.
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, TcpStream},
    time::{Duration, Instant},
};

#[derive(PartialEq)]
enum Stage {
    Greeting,
    Request,
    Connecting,
    Finishing,
    Ready,
    Rejected,
}

pub struct Handshake {
    pub socket: TcpStream,
    pub buffered: Vec<u8>,
    reply: Vec<u8>,
    offset: usize,
    stage: Stage,
    deadline: Instant,
}

impl Handshake {
    pub fn new(socket: TcpStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            buffered: Vec::new(),
            reply: Vec::new(),
            offset: 0,
            stage: Stage::Greeting,
            deadline: Instant::now() + Duration::from_secs(10),
        })
    }
    pub fn complete(&mut self, success: bool) {
        self.reply = vec![5, if success { 0 } else { 1 }, 0, 1, 0, 0, 0, 0, 0, 0];
        self.offset = 0;
        self.stage = if success {
            Stage::Finishing
        } else {
            Stage::Rejected
        };
    }
    pub fn ready(&self) -> bool {
        self.stage == Stage::Ready
    }
    pub fn poll(&mut self) -> io::Result<Option<(String, u16)>> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self.offset < self.reply.len() {
            match self.socket.write(&self.reply[self.offset..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => self.offset += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) => return Err(error),
            }
            if self.offset < self.reply.len() {
                return Ok(None);
            }
        }
        if self.stage == Stage::Rejected {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if self.stage == Stage::Finishing {
            self.stage = Stage::Ready;
        }
        if matches!(self.stage, Stage::Ready | Stage::Connecting) {
            return Ok(None);
        }
        let mut bytes = [0; 512];
        let capacity = bytes.len() - self.buffered.len();
        if capacity == 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let needs_read = if self.stage == Stage::Greeting {
            self.buffered.len() < 2 || self.buffered.len() < 2 + usize::from(self.buffered[1])
        } else {
            matches!(request(&self.buffered), Ok(None))
        };
        if needs_read {
            match self.socket.read(&mut bytes[..capacity]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(count) => self.buffered.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if self.stage == Stage::Greeting {
            if self.buffered.len() < 2 {
                return Ok(None);
            }
            let length = 2 + usize::from(self.buffered[1]);
            if self.buffered.len() < length {
                return Ok(None);
            }
            let accepted = self.buffered[0] == 5 && self.buffered[2..length].contains(&0);
            self.reply = vec![5, if accepted { 0 } else { 255 }];
            self.offset = 0;
            self.buffered.drain(..length);
            self.stage = if accepted {
                Stage::Request
            } else {
                Stage::Rejected
            };
            return Ok(None);
        }
        match request(&self.buffered) {
            Ok(Some((host, port, consumed))) => {
                self.buffered.drain(..consumed);
                self.stage = Stage::Connecting;
                Ok(Some((host, port)))
            }
            Ok(None) => Ok(None),
            Err(code) => {
                self.complete(false);
                self.reply[1] = code;
                Ok(None)
            }
        }
    }
}

fn request(bytes: &[u8]) -> Result<Option<(String, u16, usize)>, u8> {
    if bytes.len() < 4 {
        return Ok(None);
    }
    if bytes[0] != 5 || bytes[2] != 0 {
        return Err(1);
    }
    if bytes[1] != 1 {
        return Err(7);
    }
    let (start, length) = match bytes[3] {
        1 => (4, 4),
        4 => (4, 16),
        3 => {
            if bytes.len() < 5 {
                return Ok(None);
            }
            (5, usize::from(bytes[4]))
        }
        _ => return Err(8),
    };
    if length == 0 {
        return Err(8);
    }
    let end = start + length;
    if bytes.len() < end + 2 {
        return Ok(None);
    }
    let host = match bytes[3] {
        1 => Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[start..end]).unwrap()).to_string(),
        4 => Ipv6Addr::from(<[u8; 16]>::try_from(&bytes[start..end]).unwrap()).to_string(),
        _ => {
            let name = std::str::from_utf8(&bytes[start..end]).map_err(|_| 8u8)?;
            if !name.is_ascii() || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(8);
            }
            name.to_owned()
        }
    };
    let port = u16::from_be_bytes([bytes[end], bytes[end + 1]]);
    if port == 0 {
        return Err(1);
    }
    Ok(Some((host, port, end + 2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_authentication_gets_a_reply_and_idle_clients_expire() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut handshake = Handshake::new(listener.accept().unwrap().0).unwrap();
        client.write_all(&[5, 1, 2]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while handshake.stage == Stage::Greeting {
            handshake.poll().unwrap();
            assert!(Instant::now() < deadline);
        }
        assert!(handshake.poll().is_err());
        let mut reply = [0; 2];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply, [5, 255]);
        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut handshake = Handshake::new(listener.accept().unwrap().0).unwrap();
        handshake.deadline = Instant::now();
        assert_eq!(
            handshake.poll().unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
    #[test]
    fn fragmented_addresses_and_pipelined_payload_preserve_boundaries() {
        let mut domain = vec![5, 1, 0, 3, 12];
        domain.extend_from_slice(b"fixture.test");
        domain.extend_from_slice(&443u16.to_be_bytes());
        for split in 0..domain.len() {
            assert!(request(&domain[..split]).unwrap().is_none());
        }
        let length = domain.len();
        domain.extend_from_slice(b"payload");
        assert_eq!(
            request(&domain).unwrap(),
            Some(("fixture.test".into(), 443, length))
        );
        assert_eq!(
            request(&[5, 1, 0, 1, 127, 0, 0, 1, 0, 80]).unwrap(),
            Some(("127.0.0.1".into(), 80, 10))
        );
        let mut ipv6 = vec![5, 1, 0, 4];
        ipv6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        ipv6.extend_from_slice(&22u16.to_be_bytes());
        assert_eq!(request(&ipv6).unwrap(), Some(("::1".into(), 22, 22)));
        assert_eq!(request(&[5, 3, 0, 1]), Err(7));
        assert_eq!(request(&[5, 1, 0, 9]), Err(8));
    }
}
