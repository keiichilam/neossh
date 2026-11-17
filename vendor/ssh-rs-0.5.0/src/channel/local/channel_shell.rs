use super::channel::Channel;
use crate::constant::{ssh_connection_code, ssh_str, ssh_transport_code};
use crate::error::SshResult;
use crate::model::{Data, TerminalSize};
use std::{
    io::{Read, Write},
    ops::{Deref, DerefMut},
};

pub struct ChannelShell<S: Read + Write>(Channel<S>);

impl<S> ChannelShell<S>
where
    S: Read + Write,
{
    pub(crate) fn open(channel: Channel<S>, tv: TerminalSize) -> SshResult<Self> {
        Self::open_with(channel, tv, None, &[])
    }

    /// ssh4 local modification: open a shell with an optional terminal type
    /// override for the pty-req and RFC 4254 §6.4 "env" requests sent before
    /// the shell request (the server only applies names listed in AcceptEnv).
    pub(crate) fn open_with(
        channel: Channel<S>,
        tv: TerminalSize,
        term: Option<&str>,
        envs: &[(String, String)],
    ) -> SshResult<Self> {
        // to open a shell channel, we need to request a pesudo-terminal
        let mut channel_shell = ChannelShell(channel);
        crate::phases::notify(crate::ConnectionPhase::PtyRequest);
        channel_shell.request_pty(tv, term.unwrap_or(ssh_str::XTERM_VAR))?;
        for (name, value) in envs {
            channel_shell.request_env(name, value)?;
        }
        crate::phases::notify(crate::ConnectionPhase::ShellRequest);
        channel_shell.get_shell()?;
        crate::phases::notify(crate::ConnectionPhase::ShellRequestSent);
        Ok(channel_shell)
    }

    fn request_pty(&mut self, tv: TerminalSize, term: &str) -> SshResult<()> {
        let tvs = tv.fetch();
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(self.server_channel_no)
            .put_str(ssh_str::PTY_REQ)
            .put_u8(false as u8)
            .put_str(term)
            .put_u32(tvs.0)
            .put_u32(tvs.1)
            .put_u32(tvs.2)
            .put_u32(tvs.3);
        let model = [
            128, // TTY_OP_ISPEED
            0, 1, 0xc2, 0,   // 115200
            129, // TTY_OP_OSPEED
            0, 1, 0xc2, 0,    // 115200 again
            0_u8, // TTY_OP_END
        ];
        data.put_u8s(&model);
        self.send(data)
    }

    /// ssh4 local modification: SSH_MSG_CHANNEL_REQUEST "env" (RFC 4254 §6.4).
    fn request_env(&mut self, name: &str, value: &str) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(self.server_channel_no)
            .put_str("env")
            .put_u8(false as u8)
            .put_str(name)
            .put_str(value);
        self.send(data)
    }

    fn get_shell(&mut self) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(self.server_channel_no)
            .put_str(ssh_str::SHELL)
            .put_u8(false as u8);
        self.send(data)
    }

    /// this method will try to read as much data as we can from the server,
    /// but it will block until at least one packet is received
    ///
    pub fn read(&mut self) -> SshResult<Vec<u8>> {
        let mut out = self.recv()?;
        while let Ok(Some(mut data)) = self.try_recv() {
            out.append(&mut data)
        }
        Ok(out)
    }

    /// Return currently available shell data without blocking. This keeps a
    /// desktop client responsive while it also services keyboard and resize
    /// messages on the same session worker.
    pub fn try_read(&mut self) -> SshResult<Option<Vec<u8>>> {
        const MAX_PACKETS: usize = 32;
        const MAX_BYTES: usize = 256 * 1024;
        let Some(mut out) = self.try_recv()? else {
            return Ok(None);
        };
        let mut packets = 1;
        while packets < MAX_PACKETS && out.len() < MAX_BYTES {
            let Some(mut data) = self.try_recv()? else {
                break;
            };
            out.append(&mut data);
            packets += 1;
        }
        Ok(Some(out))
    }

    /// Send available window credit without waiting for a channel window update.
    pub fn try_write(&mut self, bytes: &[u8]) -> SshResult<usize> {
        self.try_send_data(bytes)
    }

    /// ssh4 local modification: notify the remote PTY of a terminal resize
    /// using an SSH_MSG_CHANNEL_REQUEST "window-change" (RFC 4254 §6.7).
    ///
    pub fn window_change(&mut self, tv: TerminalSize) -> SshResult<()> {
        let tvs = tv.fetch();
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(self.server_channel_no)
            .put_str("window-change")
            .put_u8(false as u8)
            .put_u32(tvs.0)
            .put_u32(tvs.1)
            .put_u32(tvs.2)
            .put_u32(tvs.3);
        self.send(data)
    }

    /// ssh4 local modification: send a transport-level SSH_MSG_IGNORE
    /// (RFC 4253 §11.2) as a keep-alive probe. The server discards it, but
    /// the traffic prevents idle NAT/firewall timeouts.
    ///
    pub fn keepalive(&mut self) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_transport_code::IGNORE)
            .put_str("keepalive@ssh4");
        self.send(data)
    }

    /// this method send `buf` to the remote pty
    ///
    pub fn write(&mut self, buf: &[u8]) -> SshResult<()> {
        let _ = self.send_data(buf.to_vec())?;
        Ok(())
    }
}

impl<S> Deref for ChannelShell<S>
where
    S: Read + Write,
{
    type Target = Channel<S>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> DerefMut for ChannelShell<S>
where
    S: Read + Write,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
