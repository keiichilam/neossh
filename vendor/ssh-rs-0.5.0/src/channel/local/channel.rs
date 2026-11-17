use std::io::{Read, Write};

use crate::{
    algorithm::Digest,
    client::{Client, PendingForwardedMeta},
    config::algorithm::AlgList,
    constant::{size, ssh_channel_fail_code, ssh_connection_code, ssh_str},
    error::{SshError, SshResult},
    model::{Data, FlowControl, Packet, RcMut, SecPacket},
};
use crate::{constant::ssh_transport_code, model::TerminalSize};
use tracing::*;

#[cfg(feature = "scp")]
use super::ChannelScp;
use super::{ChannelExec, ChannelShell};

#[derive(Debug)]
pub(super) enum ChannelRead {
    Data(Vec<u8>),
    Code(#[allow(dead_code)] u8),
}

pub struct Channel<S>
where
    S: Read + Write,
{
    pub(crate) server_channel_no: u32,
    pub(crate) client_channel_no: u32,
    pub(crate) remote_close: bool,
    remote_eof: bool,
    pub(crate) local_close: bool,
    pub(crate) flow_control: FlowControl,
    pub(crate) client: RcMut<Client>,
    pub(crate) stream: RcMut<S>,
    pub(crate) exit_status: u32,
    exit_status_received: bool,
    pub(crate) terminate_msg: String,
}

impl<S> Channel<S>
where
    S: Read + Write,
{
    pub(crate) fn new(
        server_channel_no: u32,
        client_channel_no: u32,
        remote_window: u32,
        client: RcMut<Client>,
        stream: RcMut<S>,
    ) -> Self {
        Self {
            server_channel_no,
            client_channel_no,
            remote_close: false,
            remote_eof: false,
            local_close: false,
            flow_control: FlowControl::new(remote_window),
            client,
            stream,
            exit_status: 0,
            exit_status_received: false,
            terminate_msg: "".to_owned(),
        }
    }

    /// convert the raw channel to an [self::ChannelExec]
    ///
    pub fn exec(self) -> SshResult<ChannelExec<S>> {
        info!("exec opened.");
        Ok(ChannelExec::open(self))
    }

    /// convert the raw channel to an [self::ChannelScp]
    ///
    #[cfg(feature = "scp")]
    pub fn scp(self) -> SshResult<ChannelScp<S>> {
        info!("scp opened.");
        Ok(ChannelScp::open(self))
    }

    /// convert the raw channel to an [self::ChannelShell]
    ///
    /// with `row` lines & `column` characters per one line
    ///
    pub fn shell(self, tv: TerminalSize) -> SshResult<ChannelShell<S>> {
        info!("shell opened.");
        ChannelShell::open(self, tv)
    }

    /// ssh4 local modification: shell with terminal type override and
    /// requested environment variables (RFC 4254 §6.4).
    pub fn shell_with(
        self,
        tv: TerminalSize,
        term: Option<&str>,
        envs: &[(String, String)],
    ) -> SshResult<ChannelShell<S>> {
        info!("shell opened.");
        ChannelShell::open_with(self, tv, term, envs)
    }

    /// close the channel gracefully, but do not consume it
    ///
    pub fn close(&mut self) -> SshResult<()> {
        info!("channel close.");
        self.send_close()?;
        self.receive_close()
    }

    /// Signal end of local input while continuing to receive peer output.
    pub fn send_eof(&mut self) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_EOF)
            .put_u32(self.server_channel_no);
        self.send(data)
    }

    /// <https://datatracker.ietf.org/doc/html/rfc4254#section-6.10>
    ///
    /// Return the command execute status
    ///
    pub fn exit_status(&self) -> SshResult<u32> {
        Ok(self.exit_status)
    }

    /// Distinguish an explicit successful exit from a channel closed without status.
    pub fn received_exit_status(&self) -> Option<u32> {
        self.exit_status_received.then_some(self.exit_status)
    }

    /// <https://datatracker.ietf.org/doc/html/rfc4254#section-6.10>
    ///
    /// Return the terminate message if the command excution was 'killed' by a signal
    ///
    pub fn terminate_msg(&self) -> SshResult<String> {
        Ok(self.terminate_msg.clone())
    }

    fn send_close(&mut self) -> SshResult<()> {
        if self.local_close {
            return Ok(());
        }
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_CLOSE)
            .put_u32(self.server_channel_no);
        self.local_close = true;
        self.send(data)
    }

    fn receive_close(&mut self) -> SshResult<()> {
        if self.remote_close {
            return Ok(());
        }
        let _ = self.recv_to_end()?;
        Ok(())
    }

    pub(super) fn send(&mut self, data: Data) -> SshResult<()> {
        data.pack(&mut self.client.borrow_mut())
            .write_stream(&mut *self.stream.borrow_mut())
    }

    // only send SSH_MSG_CHANNEL_DATA will call this,
    // for auto adjust the window size
    /// Send channel data bytes (used by shell/exec and direct-tcpip streams).
    pub(crate) fn send_data(&mut self, mut buf: Vec<u8>) -> SshResult<Vec<u8>> {
        let mut maybe_response = vec![];

        loop {
            // first adjust the data to the max size we can send
            let maybe_remain = self.flow_control.tune_on_send(&mut buf);

            // send it
            let mut data = Data::new();
            data.put_u8(ssh_connection_code::CHANNEL_DATA)
                .put_u32(self.server_channel_no)
                .put_u8s(&buf);
            self.send(data)?;

            if maybe_remain.is_empty() {
                // if all send, return
                break;
            } else {
                buf = maybe_remain
            }

            // otherwise wait the server to adjust its window
            while !self.flow_control.can_send() {
                let buf = self.recv_once()?;

                if let ChannelRead::Data(mut data) = buf {
                    maybe_response.append(&mut data);
                }
            }
        }

        Ok(maybe_response)
    }

    /// Send at most one packet using available credit; never wait for a
    /// channel window adjustment. The caller retains any unsent suffix.
    pub(crate) fn try_send_data(&mut self, bytes: &[u8]) -> SshResult<usize> {
        self.apply_demux_state()?;
        if self.remote_close || self.local_close {
            return Err(SshError::GeneralError("Channel is closed".into()));
        }
        if bytes.is_empty() || !self.flow_control.can_send() {
            return Ok(0);
        }
        let mut payload = bytes[..bytes.len().min(8192)].to_vec();
        let _remaining = self.flow_control.tune_on_send(&mut payload);
        let count = payload.len();
        let mut packet = Data::new();
        packet
            .put_u8(ssh_connection_code::CHANNEL_DATA)
            .put_u32(self.server_channel_no)
            .put_u8s(&payload);
        self.send(packet)?;
        Ok(count)
    }

    /// this method will receive at least one data packet
    ///
    pub(crate) fn recv(&mut self) -> SshResult<Vec<u8>> {
        while !self.closed() {
            let maybe_recv = self.recv_once()?;

            if let ChannelRead::Data(data) = maybe_recv {
                return Ok(data);
            }
        }
        Ok(vec![])
    }

    pub(super) fn recv_to_end(&mut self) -> SshResult<Vec<u8>> {
        let mut resp = vec![];
        while !self.closed() {
            let mut read_this_time = self.recv()?;
            resp.append(&mut read_this_time);
        }
        Ok(resp)
    }

    pub(crate) fn try_recv(&mut self) -> SshResult<Option<Vec<u8>>> {
        // Apply demuxed state first (data/close/window parked while another channel read).
        self.apply_demux_state()?;
        if let Some(d) = self.take_inbox_data() {
            return Ok(Some(d));
        }
        if self.remote_close {
            return Ok(None);
        }

        let data = {
            match SecPacket::try_from_stream(
                &mut *self.stream.borrow_mut(),
                &mut self.client.borrow_mut(),
            )? {
                Some(pkt) => Data::unpack(pkt)?,
                None => return Ok(None),
            }
        };
        if let ChannelRead::Data(d) = self.handle_msg(data)? {
            Ok(Some(d))
        } else {
            // Another demux event may have filled our inbox.
            if let Some(d) = self.take_inbox_data() {
                Ok(Some(d))
            } else {
                Ok(None)
            }
        }
    }

    fn apply_demux_state(&mut self) -> SshResult<()> {
        if self
            .client
            .borrow_mut()
            .request_failures
            .remove(&self.client_channel_no)
        {
            return Err(SshError::GeneralError("channel failure.".into()));
        }
        let (closed, adj) = {
            let mut client = self.client.borrow_mut();
            if let Some(status) = client.exit_statuses.remove(&self.client_channel_no) {
                self.exit_status = status;
                self.exit_status_received = true;
            }
            self.remote_eof |= client.remote_eof.remove(&self.client_channel_no);
            let closed = client.remote_closed.remove(&self.client_channel_no);
            let adj = client.window_credits.remove(&self.client_channel_no);
            (closed, adj)
        };
        if closed {
            self.remote_close = true;
            // Reciprocal CLOSE when peer closed while another channel was reading.
            if !self.local_close {
                self.send_close()?;
            }
            // The peer may have sent data immediately before CLOSE while a
            // different channel owned the reader. Keep its inbox until drained;
            // force_close/drop removes the channel registration afterward.
        }
        if let Some(adj) = adj {
            self.recv_window_adjust(adj)?;
        }
        Ok(())
    }

    fn take_inbox_data(&mut self) -> Option<Vec<u8>> {
        let data = self
            .client
            .borrow_mut()
            .take_parked_payload(self.client_channel_no);
        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    }

    fn recv_once(&mut self) -> SshResult<ChannelRead> {
        let data = Data::unpack(SecPacket::from_stream(
            &mut *self.stream.borrow_mut(),
            &mut self.client.borrow_mut(),
        )?)?;
        self.handle_msg(data)
    }

    fn handle_msg(&mut self, mut data: Data) -> SshResult<ChannelRead> {
        let message_code = data.get_u8();
        match message_code {
            x @ ssh_transport_code::KEXINIT => {
                data.insert(0, message_code);
                let mut digest = Digest::new();
                digest.hash_ctx.set_i_s(&data);
                let server_algs = AlgList::unpack((data, &mut *self.client.borrow_mut()).into())?;
                self.client.borrow_mut().key_agreement(
                    &mut *self.stream.borrow_mut(),
                    server_algs,
                    &mut digest,
                )?;
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_DATA => {
                let cc = data.get_u32();
                let mut payload = data.get_u8s();
                if cc == self.client_channel_no {
                    self.flow_control.tune_on_recv(&mut payload);
                    self.send_window_adjust(payload.len() as u32)?;
                    return Ok(ChannelRead::Data(payload));
                }
                // Demux: park for the owner channel and credit its window.
                self.demux_foreign_data(cc, payload)?;
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_EXTENDED_DATA => {
                let cc = data.get_u32();
                let data_type_code = data.get_u32();
                let mut payload = data.get_u8s();
                debug!("Recv extended data with type {data_type_code}");
                if cc == self.client_channel_no {
                    self.flow_control.tune_on_recv(&mut payload);
                    self.send_window_adjust(payload.len() as u32)?;
                    return Ok(ChannelRead::Data(payload));
                }
                self.demux_foreign_data(cc, payload)?;
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::GLOBAL_REQUEST => {
                let _name = data.get_u8s();
                if data.get_u8() != 0 {
                    let mut reply = Data::new();
                    reply.put_u8(ssh_connection_code::REQUEST_FAILURE);
                    self.send(reply)?;
                }
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_WINDOW_ADJUST => {
                let cc = data.get_u32();
                let rws = data.get_u32();
                if cc == self.client_channel_no {
                    self.recv_window_adjust(rws)?;
                } else {
                    self.client.borrow_mut().park_window_credit(cc, rws);
                }
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_EOF => {
                let recipient = data.get_u32();
                if recipient == self.client_channel_no {
                    self.remote_eof = true;
                } else {
                    self.client.borrow_mut().remote_eof.insert(recipient);
                }
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_REQUEST => {
                let cc = data.get_u32();
                if cc != self.client_channel_no {
                    let request = data.get_u8s();
                    if request == b"exit-status" && data.len() >= 5 {
                        let want_reply = data.get_u8();
                        let status = data.get_u32();
                        let mut client = self.client.borrow_mut();
                        if want_reply == 0 && client.channel_peers.contains_key(&cc) {
                            client.exit_statuses.insert(cc, status);
                        }
                    }
                    return Ok(ChannelRead::Code(x));
                }
                if cc == self.client_channel_no {
                    let status: Vec<u8> = data.get_u8s();
                    if let Ok(status_string) = String::from_utf8(status.clone()) {
                        match status_string.as_str() {
                            "exit-status" => {
                                let _ = self.handle_exit_status(&mut data);
                            }
                            "exit-signal" => {
                                let _ = self.handle_exit_signal(&mut data);
                            }
                            s => {
                                debug!("Currently ignore request {}", s);
                            }
                        }
                    }
                }
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_SUCCESS => {
                debug!("Currently ignore message {}", x);
                Ok(ChannelRead::Code(x))
            }
            x @ ssh_connection_code::CHANNEL_FAILURE => {
                // byte CHANNEL_FAILURE; uint32 recipient channel
                let cc = data.get_u32();
                if cc == self.client_channel_no {
                    Err(SshError::GeneralError("channel failure.".to_owned()))
                } else {
                    let mut client = self.client.borrow_mut();
                    if client.server_channel_for(cc).is_some() {
                        client.request_failures.insert(cc);
                    }
                    Ok(ChannelRead::Code(x))
                }
            }
            x @ ssh_connection_code::CHANNEL_CLOSE => {
                let cc = data.get_u32();
                if cc == self.client_channel_no {
                    self.remote_close = true;
                    self.send_close()?;
                    self.client
                        .borrow_mut()
                        .unregister_channel(self.client_channel_no);
                } else {
                    self.client.borrow_mut().park_remote_close(cc);
                }
                Ok(ChannelRead::Code(x))
            }
            // Concurrent forwarded-tcpip while this channel owns the stream:
            // confirm and queue for accept (multi-channel -R demux).
            x @ ssh_connection_code::CHANNEL_OPEN => {
                self.demux_channel_open(data)?;
                Ok(ChannelRead::Code(x))
            }
            x => {
                debug!("Currently ignore message {}", x);
                Ok(ChannelRead::Code(x))
            }
        }
    }

    fn demux_foreign_data(&mut self, client_channel_no: u32, payload: Vec<u8>) -> SshResult<()> {
        let len = payload.len() as u32;
        let server_no = {
            let mut client = self.client.borrow_mut();
            client.park_channel_payload(client_channel_no, &payload);
            client.server_channel_for(client_channel_no)
        };
        // Credit the peer window for the owner channel (server channel id).
        if let Some(server_no) = server_no {
            let mut data = Data::new();
            data.put_u8(ssh_connection_code::CHANNEL_WINDOW_ADJUST)
                .put_u32(server_no)
                .put_u32(len);
            self.send(data)?;
        }
        Ok(())
    }

    /// Best-effort CLOSE + demux unregister when a channel is dropped mid-relay.
    pub fn force_close(&mut self) {
        if !self.local_close {
            let _ = self.send_close();
        }
        self.client
            .borrow_mut()
            .unregister_channel(self.client_channel_no);
    }

    #[cfg(test)]
    fn handle_msg_for_test(&mut self, data: Data) -> SshResult<ChannelRead> {
        self.handle_msg(data)
    }

    fn demux_channel_open(&mut self, mut data: Data) -> SshResult<()> {
        let channel_type = String::from_utf8_lossy(&data.get_u8s()).into_owned();
        let server_channel_no = data.get_u32();
        let remote_window_size = data.get_u32();
        let _max_packet = data.get_u32();
        if channel_type != ssh_str::FORWARDED_TCPIP {
            let mut fail = Data::new();
            fail.put_u8(ssh_connection_code::CHANNEL_OPEN_FAILURE)
                .put_u32(server_channel_no)
                .put_u32(ssh_channel_fail_code::UNKNOWN_CHANNEL_TYPE)
                .put_str("unsupported channel type")
                .put_str("");
            self.send(fail)?;
            return Ok(());
        }
        let connected_address = String::from_utf8(data.get_u8s())?;
        let connected_port = data.get_u32();
        let _originator_addr = data.get_u8s();
        let _originator_port = data.get_u32();

        let client_channel_no = self.client.borrow_mut().alloc_channel_no();
        let mut conf = Data::new();
        conf.put_u8(ssh_connection_code::CHANNEL_OPEN_CONFIRMATION)
            .put_u32(server_channel_no)
            .put_u32(client_channel_no)
            .put_u32(size::LOCAL_WINDOW_SIZE)
            .put_u32(size::BUF_SIZE as u32);
        self.send(conf)?;

        let mut client = self.client.borrow_mut();
        client.register_channel(client_channel_no, server_channel_no);
        client.pending_forwarded.push_back(PendingForwardedMeta {
            server_channel_no,
            client_channel_no,
            remote_window_size,
            connected_address,
            connected_port,
        });
        debug!("Queued concurrent forwarded-tcpip channel {client_channel_no}");
        Ok(())
    }

    fn handle_exit_status(&mut self, data: &mut Data) -> SshResult<()> {
        if data.len() < 5 {
            return Err(SshError::GeneralError("Truncated exit status.".into()));
        }
        let maybe_false = data.get_u8();
        if maybe_false == 0 {
            self.exit_status = data.get_u32();
            self.exit_status_received = true;
        }
        Ok(())
    }

    fn handle_exit_signal(&mut self, data: &mut Data) -> SshResult<()> {
        let maybe_false = data.get_u8();
        if maybe_false == 0 {
            let sig_name = String::from_utf8(data.get_u8s())?;
            self.terminate_msg += &format!("Current request is terminated by signal: {sig_name}\n");
            let coredumped = data.get_u8();
            self.terminate_msg += &format!("Coredumped: {}\n", {
                if coredumped == 0 {
                    "False"
                } else {
                    "True"
                }
            });
            let err_msg = String::from_utf8(data.get_u8s())?;
            self.terminate_msg += &format!("Error message:\n{err_msg}\n");
        }
        Ok(())
    }

    fn send_window_adjust(&mut self, to_add: u32) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_WINDOW_ADJUST)
            .put_u32(self.server_channel_no)
            .put_u32(to_add);
        self.flow_control.on_send(to_add);
        self.send(data)
    }

    fn recv_window_adjust(&mut self, to_add: u32) -> SshResult<()> {
        self.flow_control.on_recv(to_add);
        Ok(())
    }

    /// Return if the channel is closed
    ///
    pub fn closed(&self) -> bool {
        self.local_close && self.remote_close
    }

    /// True after the peer has sent CHANNEL_CLOSE for this channel.
    pub fn remote_closed(&self) -> bool {
        self.remote_close
    }
    /// True when the peer has finished sending data but may still receive it.
    pub fn remote_eof(&self) -> bool {
        self.remote_eof
    }
}

#[cfg(test)]
mod demux_msg_tests {
    use super::*;
    use crate::config::Config;
    use std::io::{self, Read, Write};
    use std::{cell::RefCell, rc::Rc};

    /// Sink that accepts writes (for demux window-adjust / close).
    struct Sink(Vec<u8>);

    impl Read for Sink {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "no data"))
        }
    }
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn test_channel(client_no: u32, server_no: u32) -> Channel<Sink> {
        let mut client = Client::new(Config::default());
        client.register_channel(client_no, server_no);
        // Second peer for foreign-data parking tests.
        client.register_channel(2, 20);
        let client = Rc::new(RefCell::new(client));
        let stream = Rc::new(RefCell::new(Sink(Vec::new())));
        Channel::new(server_no, client_no, 1 << 20, client, stream)
    }

    #[test]
    fn foreign_exit_status_reaches_only_its_owner() {
        let mut reader = test_channel(1, 10);
        let mut owner = Channel::new(20, 2, 65536, reader.client.clone(), reader.stream.clone());
        assert_eq!(owner.received_exit_status(), None);
        let mut request = Data::new();
        request
            .put_u8(ssh_connection_code::CHANNEL_REQUEST)
            .put_u32(2)
            .put_str("exit-status")
            .put_u8(0)
            .put_u32(17);
        reader.handle_msg_for_test(request).unwrap();
        assert_eq!(reader.received_exit_status(), None);
        owner.apply_demux_state().unwrap();
        assert_eq!(owner.received_exit_status(), Some(17));
        assert!(reader.client.borrow().exit_statuses.is_empty());
    }

    #[test]
    fn global_requests_only_receive_requested_replies() {
        let mut channel = test_channel(1, 10);
        for want_reply in [0, 1, 255] {
            channel.stream.borrow_mut().0.clear();
            let mut request = Data::new();
            request
                .put_u8(ssh_connection_code::GLOBAL_REQUEST)
                .put_str("unknown@fixture")
                .put_u8(want_reply);
            channel.handle_msg_for_test(request).unwrap();
            assert_eq!(channel.stream.borrow().0.is_empty(), want_reply == 0);
        }
    }

    #[test]
    fn forwarded_channel_keeps_its_listener_identity() {
        let mut channel = test_channel(1, 10);
        let mut request = Data::new();
        request
            .put_u8(ssh_connection_code::CHANNEL_OPEN)
            .put_str(ssh_str::FORWARDED_TCPIP)
            .put_u32(77)
            .put_u32(65536)
            .put_u32(8192)
            .put_str("::1")
            .put_u32(43210)
            .put_str("::1")
            .put_u32(12345);
        channel.handle_msg_for_test(request).unwrap();
        let meta = channel
            .client
            .borrow_mut()
            .pending_forwarded
            .pop_front()
            .unwrap();
        assert_eq!(meta.connected_address, "::1");
        assert_eq!(meta.connected_port, 43210);
        assert_eq!(meta.server_channel_no, 77);
    }

    #[test]
    fn foreign_channel_data_is_parked_not_dropped() {
        let mut ch = test_channel(1, 10);
        let mut msg = Data::new();
        msg.put_u8(ssh_connection_code::CHANNEL_DATA)
            .put_u32(2) // foreign client channel
            .put_u8s(b"relay-payload");
        // handle_msg consumes message code first in real path — feed full packet body style:
        // handle_msg does get_u8 for message code, so include it.
        let mut full = Data::new();
        full.put_u8(ssh_connection_code::CHANNEL_DATA)
            .put_u32(2)
            .put_u8s(b"relay-payload");
        ch.handle_msg_for_test(full).unwrap();
        let parked = ch.client.borrow_mut().take_parked_payload(2);
        assert_eq!(parked, b"relay-payload");
    }

    #[test]
    fn zero_window_try_send_keeps_data_with_caller_without_waiting() {
        let mut channel = test_channel(1, 10);
        channel.flow_control = FlowControl::new(0);
        assert_eq!(channel.try_send_data(b"pending input").unwrap(), 0);
        assert_eq!(channel.try_send_data(b"").unwrap(), 0);
    }

    #[test]
    fn parked_data_is_delivered_before_a_parked_close() {
        let mut channel = test_channel(1, 10);
        channel.local_close = true; // Avoid transport output in this unit fixture.
        channel
            .client
            .borrow_mut()
            .park_channel_payload(1, b"final bytes");
        channel.client.borrow_mut().park_remote_close(1);
        assert_eq!(channel.try_recv().unwrap(), Some(b"final bytes".to_vec()));
        assert!(channel.remote_closed());
    }

    #[test]
    fn eof_is_recipient_scoped_and_does_not_close_the_channel() {
        let mut channel = test_channel(1, 10);
        let mut foreign = Data::new();
        foreign.put_u8(ssh_connection_code::CHANNEL_EOF).put_u32(2);
        channel.handle_msg_for_test(foreign).unwrap();
        assert!(!channel.remote_eof());
        assert!(channel.client.borrow().remote_eof.contains(&2));
        channel
            .client
            .borrow_mut()
            .park_channel_payload(1, b"before EOF");
        channel.client.borrow_mut().remote_eof.insert(1);
        assert_eq!(channel.try_recv().unwrap(), Some(b"before EOF".to_vec()));
        assert!(channel.remote_eof());
        assert!(!channel.remote_closed());
        assert!(!channel.local_close);
        assert!(channel.client.borrow().remote_eof.contains(&2));
    }

    #[test]
    fn channel_failure_for_foreign_id_does_not_err() {
        let mut ch = test_channel(1, 10);
        let mut full = Data::new();
        full.put_u8(ssh_connection_code::CHANNEL_FAILURE)
            .put_u32(99); // not us
        let r = ch.handle_msg_for_test(full).unwrap();
        assert!(matches!(r, ChannelRead::Code(_)));
    }

    #[test]
    fn foreign_request_failure_is_delivered_to_its_owner() {
        let mut channel = test_channel(1, 10);
        let mut request = Data::new();
        request
            .put_u8(ssh_connection_code::CHANNEL_FAILURE)
            .put_u32(2);
        channel.handle_msg_for_test(request).unwrap();
        channel.apply_demux_state().unwrap();
        let mut owner = Channel::new(20, 2, 65536, channel.client.clone(), channel.stream.clone());
        assert!(owner
            .try_recv()
            .unwrap_err()
            .to_string()
            .contains("channel failure"));
        assert!(!channel.client.borrow().request_failures.contains(&2));
    }

    #[test]
    fn channel_failure_for_own_id_errors() {
        let mut ch = test_channel(1, 10);
        let mut full = Data::new();
        full.put_u8(ssh_connection_code::CHANNEL_FAILURE).put_u32(1);
        let err = ch.handle_msg_for_test(full).unwrap_err();
        assert!(err.to_string().contains("channel failure"));
    }
}
