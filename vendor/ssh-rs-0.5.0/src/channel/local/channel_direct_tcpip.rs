//! Direct-tcpip channel as a bidirectional stream (ssh4 local addition).
//!
//! Used for local port forwarding and as the transport for one-hop ProxyJump.
//!
//! `Read` is **non-blocking** (returns `WouldBlock` when no data), matching
//! `TcpStream` after `set_nonblocking(true)` so nested SSH `try_read` / KEX
//! loops can interleave without freezing the worker.

use super::channel::Channel;
use crate::error::SshError;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    ops::{Deref, DerefMut},
};

/// An open `direct-tcpip` channel exposed as `Read + Write`.
pub struct ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    channel: Channel<S>,
    read_buf: VecDeque<u8>,
    eof: bool,
}

impl<S> ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    pub(crate) fn open(channel: Channel<S>) -> Self {
        Self {
            channel,
            read_buf: VecDeque::new(),
            eof: false,
        }
    }

    /// Non-blocking poll of the channel into `read_buf`.
    fn fill_buf(&mut self) -> io::Result<()> {
        if self.eof {
            return Ok(());
        }
        // Drain any available packets; stop when would-block / empty.
        loop {
            match self.channel.try_recv() {
                Ok(Some(data)) if !data.is_empty() => {
                    self.read_buf.extend(data);
                    // Keep reading while data is available.
                }
                Ok(Some(_)) => {
                    // Empty data payload; check close flags.
                    if self.channel.remote_closed() || self.channel.remote_eof() {
                        self.eof = true;
                    }
                    return Ok(());
                }
                Ok(None) => {
                    // No packet ready (or non-data handled). Check close.
                    if self.channel.remote_closed() || self.channel.remote_eof() {
                        self.eof = true;
                    }
                    return Ok(());
                }
                Err(SshError::TimeoutError) => return Ok(()),
                Err(error) => {
                    self.eof = true;
                    return Err(io::Error::new(io::ErrorKind::Other, error.to_string()));
                }
            }
            // Cap burst so one fill doesn't spin forever on a flood.
            if self.read_buf.len() >= 256 * 1024 {
                return Ok(());
            }
        }
    }

    fn take_from_buf(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.read_buf.len());
        for slot in buf.iter_mut().take(n) {
            *slot = self.read_buf.pop_front().unwrap();
        }
        n
    }

    /// Non-blocking read of any available channel data.
    ///
    /// - `Ok(0)` — EOF (peer closed the channel)
    /// - `Err(WouldBlock)` — no data yet
    pub fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf)
    }

    /// Send CHANNEL_CLOSE and unregister demux state (local-connect failure, etc.).
    pub fn close_now(&mut self) {
        self.channel.force_close();
    }

    /// Write only what the peer's channel window currently allows.
    pub fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.channel.try_send_data(bytes) {
            Ok(0) if !bytes.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            Ok(count) => Ok(count),
            Err(error) => Err(io::Error::new(io::ErrorKind::Other, error.to_string())),
        }
    }
}

impl<S> Read for ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.read_buf.is_empty() {
            return Ok(self.take_from_buf(buf));
        }
        self.fill_buf()?;
        if !self.read_buf.is_empty() {
            return Ok(self.take_from_buf(buf));
        }
        if self.eof {
            return Ok(0);
        }
        Err(io::Error::new(io::ErrorKind::WouldBlock, "no channel data"))
    }
}

impl<S> Write for ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Drain any inbound first so window updates progress.
        self.fill_buf()?;
        match self.channel.send_data(buf.to_vec()) {
            Ok(maybe_response) => {
                if !maybe_response.is_empty() {
                    self.read_buf.extend(maybe_response);
                }
                Ok(buf.len())
            }
            Err(error) => Err(io::Error::new(io::ErrorKind::Other, error.to_string())),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<S> Deref for ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    type Target = Channel<S>;
    fn deref(&self) -> &Self::Target {
        &self.channel
    }
}

impl<S> DerefMut for ChannelDirectTcpip<S>
where
    S: Read + Write,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.channel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::Client, config::Config};
    use std::{cell::RefCell, rc::Rc};

    struct FailedTransport {
        written: usize,
    }

    impl Read for FailedTransport {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "upstream verification failed",
            ))
        }
    }

    impl Write for FailedTransport {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.written += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_propagates_upstream_read_failure_without_sending_data() {
        let mut client = Client::new(Config::default());
        client.register_channel(1, 10);
        let stream = Rc::new(RefCell::new(FailedTransport { written: 0 }));
        let channel = Channel::new(10, 1, 65536, Rc::new(RefCell::new(client)), stream.clone());
        let mut tunnel = ChannelDirectTcpip::open(channel);
        let error = tunnel.write(b"private downstream packet").unwrap_err();
        assert!(error.to_string().contains("upstream verification failed"));
        assert_eq!(stream.borrow().written, 0);
    }
}
