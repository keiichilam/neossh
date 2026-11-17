//! SFTP subsystem channel (ssh4 local addition).
//!
//! Opens a session channel, requests the `sftp` subsystem, and exposes raw
//! channel data I/O for an application-level SFTP protocol client.

use super::channel::Channel;
use crate::{
    constant::{ssh_connection_code, ssh_str},
    error::{SshError, SshResult},
    model::Data,
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    ops::{Deref, DerefMut},
};
use tracing::*;

pub struct ChannelSftp<S>
where
    S: Read + Write,
{
    channel: Channel<S>,
    read_buf: VecDeque<u8>,
    eof: bool,
}

impl<S> ChannelSftp<S>
where
    S: Read + Write,
{
    pub fn open(mut channel: Channel<S>) -> SshResult<Self> {
        info!("sftp subsystem request");
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(channel.server_channel_no)
            .put_str(ssh_str::SUBSYSTEM)
            .put_u8(true as u8) // want reply
            .put_str("sftp");
        channel.send(data)?;
        // CHANNEL_FAILURE raises in handle_msg; CHANNEL_SUCCESS is ignored.
        // Drain control messages so the first application read is SFTP data.
        for _ in 0..8 {
            match channel.try_recv() {
                Ok(Some(early)) => {
                    // Rare: data before we finished draining control.
                    return Ok(Self {
                        channel,
                        read_buf: VecDeque::from(early),
                        eof: false,
                    });
                }
                Ok(None) => break,
                Err(SshError::TimeoutError) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(Self {
            channel,
            read_buf: VecDeque::new(),
            eof: false,
        })
    }

    fn fill_buf(&mut self) -> SshResult<()> {
        if self.eof {
            return Ok(());
        }
        loop {
            match self.channel.try_recv() {
                Ok(Some(data)) if !data.is_empty() => self.read_buf.extend(data),
                Ok(Some(_)) => {
                    if self.channel.remote_closed() || self.channel.remote_eof() {
                        self.eof = true;
                    }
                    return Ok(());
                }
                Ok(None) => {
                    if self.channel.remote_closed() || self.channel.remote_eof() {
                        self.eof = true;
                    }
                    return Ok(());
                }
                Err(SshError::TimeoutError) => return Ok(()),
                Err(e) => {
                    self.eof = true;
                    return Err(e);
                }
            }
            if self.read_buf.len() >= 256 * 1024 {
                return Ok(());
            }
        }
    }

    fn fill_buf_blocking(&mut self) -> SshResult<()> {
        self.fill_buf()?;
        if !self.read_buf.is_empty() || self.eof {
            return Ok(());
        }
        match self.channel.recv() {
            Ok(data) if data.is_empty() => {
                self.eof = true;
                Ok(())
            }
            Ok(data) => {
                self.read_buf.extend(data);
                Ok(())
            }
            Err(e) => {
                self.eof = true;
                Err(e)
            }
        }
    }

    /// Write raw SFTP packet bytes (including the 4-byte length prefix if already framed).
    pub fn try_write(&mut self, buf: &[u8]) -> SshResult<usize> {
        self.channel.try_send_data(buf)
    }

    /// Poll one framed SFTP packet without waiting for more channel data.
    pub fn try_read_packet(&mut self) -> SshResult<Option<Vec<u8>>> {
        if let Some(packet) = self.take_packet()? {
            return Ok(Some(packet));
        }
        let received = self.fill_buf();
        if let Some(packet) = self.take_packet()? {
            return Ok(Some(packet));
        }
        received?;
        if self.eof {
            return Err(SshError::GeneralError(
                "SFTP channel closed before complete packet".into(),
            ));
        }
        Ok(None)
    }

    fn take_packet(&mut self) -> SshResult<Option<Vec<u8>>> {
        if self.read_buf.len() >= 4 {
            let mut header = [0; 4];
            for (slot, byte) in header.iter_mut().zip(self.read_buf.iter()) {
                *slot = *byte;
            }
            let length = u32::from_be_bytes(header) as usize;
            if length == 0 || length > 1024 * 1024 {
                return Err(SshError::GeneralError("Invalid SFTP packet size".into()));
            }
            if self.read_buf.len() >= length + 4 {
                self.read_buf.drain(..4);
                return Ok(Some(self.read_buf.drain(..length).collect()));
            }
        }
        Ok(None)
    }

    pub fn close_now(&mut self) {
        self.channel.force_close();
    }

    /// Write raw SFTP packet bytes (including the 4-byte length prefix if already framed).
    pub fn write_all(&mut self, buf: &[u8]) -> SshResult<()> {
        let _ = self.fill_buf();
        let extra = self.channel.send_data(buf.to_vec())?;
        if !extra.is_empty() {
            self.read_buf.extend(extra);
        }
        Ok(())
    }

    /// Read exactly `n` bytes (blocking).
    pub fn read_exact(&mut self, n: usize) -> SshResult<Vec<u8>> {
        while self.read_buf.len() < n && !self.eof {
            self.fill_buf_blocking()?;
        }
        if self.read_buf.len() < n {
            return Err(SshError::GeneralError(
                "SFTP channel closed before complete packet".into(),
            ));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.read_buf.pop_front().unwrap());
        }
        Ok(out)
    }

    /// Read one SFTP packet (uint32 length + body).
    pub fn read_packet(&mut self) -> SshResult<Vec<u8>> {
        let len_bytes = self.read_exact(4)?;
        let len =
            u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        if len == 0 || len > 1024 * 1024 {
            return Err(SshError::GeneralError(format!(
                "Invalid SFTP packet length {len}"
            )));
        }
        let mut body = self.read_exact(len)?;
        let mut full = len_bytes;
        full.append(&mut body);
        Ok(full)
    }
}

impl<S> Deref for ChannelSftp<S>
where
    S: Read + Write,
{
    type Target = Channel<S>;
    fn deref(&self) -> &Self::Target {
        &self.channel
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use crate::{client::Client, config::Config};
    use std::{cell::RefCell, io::Cursor, rc::Rc};
    fn channel(bytes: &[u8]) -> ChannelSftp<Cursor<Vec<u8>>> {
        ChannelSftp {
            channel: Channel::new(
                1,
                1,
                65536,
                Rc::new(RefCell::new(Client::new(Config::default()))),
                Rc::new(RefCell::new(Cursor::new(Vec::new()))),
            ),
            read_buf: bytes.iter().copied().collect(),
            eof: false,
        }
    }
    #[test]
    fn partial_frames_are_retained_and_complete_frames_precede_transport_eof() {
        let frame = [0, 0, 0, 5, 2, 0, 0, 0, 3];
        for split in 0..frame.len() {
            let mut channel = channel(&frame[..split]);
            assert!(channel.take_packet().unwrap().is_none());
            assert_eq!(channel.read_buf.len(), split);
            channel.read_buf.extend(&frame[split..]);
            assert_eq!(channel.try_read_packet().unwrap().unwrap(), &frame[4..]);
        }
        let mut channel = channel(&frame.repeat(2));
        assert_eq!(channel.try_read_packet().unwrap().unwrap(), &frame[4..]);
        assert_eq!(channel.try_read_packet().unwrap().unwrap(), &frame[4..]);
    }
    #[test]
    fn invalid_frame_lengths_fail_without_allocating_a_body() {
        for header in [[0, 0, 0, 0], [0, 16, 0, 1], [255, 255, 255, 255]] {
            assert!(channel(&header).take_packet().is_err());
        }
    }
}

impl<S> DerefMut for ChannelSftp<S>
where
    S: Read + Write,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.channel
    }
}
