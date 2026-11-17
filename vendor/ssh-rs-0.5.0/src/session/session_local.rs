use std::{
    cell::RefCell,
    io::{Read, Write},
    rc::Rc,
    time::Duration,
};
use tracing::*;

#[cfg(feature = "scp")]
use crate::channel::LocalScp;
use crate::{
    channel::{LocalChannel, LocalExec, LocalSftp, LocalShell},
    client::{Client, PendingForwardedMeta},
    constant::{size, ssh_channel_fail_code, ssh_connection_code, ssh_str},
    error::{SshError, SshResult},
    model::TerminalSize,
    model::{Data, Packet, RcMut, SecPacket},
};

pub struct LocalSession<S>
where
    S: Read + Write,
{
    client: RcMut<Client>,
    stream: RcMut<S>,
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::config::Config;
    use std::io::{self, Cursor};

    struct Duplex {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Duplex {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.input.read(bytes)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn session(messages: Vec<Data>) -> LocalSession<Duplex> {
        let mut peer = Client::new(Config::default());
        let mut wire = Vec::new();
        for message in messages {
            message.pack(&mut peer).write_stream(&mut wire).unwrap();
        }
        LocalSession::new(
            Client::new(Config::default()),
            Duplex {
                input: Cursor::new(wire),
                output: Vec::new(),
            },
        )
    }
    fn forwarded(address: &str, port: u32, channel: u32) -> Data {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_OPEN)
            .put_str(ssh_str::FORWARDED_TCPIP)
            .put_u32(channel)
            .put_u32(65536)
            .put_u32(8192)
            .put_str(address)
            .put_u32(port)
            .put_str("127.0.0.1")
            .put_u32(5555);
        data
    }
    #[test]
    fn global_request_reply_flag_is_respected() {
        let mut session = session(vec![]);
        for want_reply in [0, 1, 255] {
            session.stream.borrow_mut().output.clear();
            let mut request = Data::new();
            request.put_str("unknown@fixture").put_u8(want_reply);
            session.reject_global_request(request).unwrap();
            assert_eq!(session.stream.borrow().output.is_empty(), want_reply == 0);
        }
    }
    #[test]
    fn remote_forward_request_retains_interleaved_channel_traffic() {
        let mut payload = Data::new();
        payload
            .put_u8(ssh_connection_code::CHANNEL_DATA)
            .put_u32(42)
            .put_u8s(b"terminal bytes");
        let mut eof = Data::new();
        eof.put_u8(ssh_connection_code::CHANNEL_EOF).put_u32(42);
        let mut success = Data::new();
        success
            .put_u8(ssh_connection_code::REQUEST_SUCCESS)
            .put_u32(43000);
        let mut session = session(vec![
            forwarded("127.0.0.1", 42000, 70),
            payload,
            eof,
            success,
        ]);
        session.client.borrow_mut().register_channel(42, 99);
        assert_eq!(
            session.request_tcpip_forward("127.0.0.1", 0).unwrap(),
            43000
        );
        assert_eq!(
            session.client.borrow_mut().take_parked_payload(42),
            b"terminal bytes"
        );
        assert!(session.client.borrow().remote_eof.contains(&42));
        let (_, address, port) = session
            .take_pending_forwarded_tcpip_with_destination()
            .unwrap();
        assert_eq!((address.as_str(), port), ("127.0.0.1", 42000));
        assert!(session.take_pending_forwarded_tcpip().is_none());
    }
    #[test]
    fn opening_a_channel_retains_multiple_incoming_forward_destinations() {
        let mut success = Data::new();
        success
            .put_u8(ssh_connection_code::CHANNEL_OPEN_CONFIRMATION)
            .put_u32(42)
            .put_u32(99)
            .put_u32(65536)
            .put_u32(8192);
        let mut session = session(vec![
            forwarded("127.0.0.1", 42000, 70),
            forwarded("::1", 42001, 71),
            success,
        ]);
        assert_eq!(session.receive_open_channel().unwrap(), (99, 65536));
        for expected in [("127.0.0.1", 42000), ("::1", 42001)] {
            let (_, address, port) = session
                .take_pending_forwarded_tcpip_with_destination()
                .unwrap();
            assert_eq!((address.as_str(), port), expected);
        }
    }
    #[test]
    fn allocated_forwarding_port_must_be_present_nonzero_and_fit_u16() {
        for port in [None, Some(0), Some(65536), Some(u32::MAX)] {
            let mut success = Data::new();
            success.put_u8(ssh_connection_code::REQUEST_SUCCESS);
            if let Some(port) = port {
                success.put_u32(port);
            }
            assert!(session(vec![success])
                .request_tcpip_forward("127.0.0.1", 0)
                .is_err());
        }
        let mut success = Data::new();
        success.put_u8(ssh_connection_code::REQUEST_SUCCESS);
        assert_eq!(
            session(vec![success])
                .request_tcpip_forward("127.0.0.1", 43210)
                .unwrap(),
            43210
        );
    }
}

impl<S> LocalSession<S>
where
    S: Read + Write,
{
    pub(crate) fn new(client: Client, stream: S) -> Self {
        Self {
            client: Rc::new(RefCell::new(client)),
            stream: Rc::new(RefCell::new(stream)),
        }
    }

    /// close the local session and consume it
    ///
    pub fn close(self) {
        info!("Client close");
        drop(self)
    }

    /// Modify the timeout setting
    /// in case the user wants to change the timeout during ssh operations.
    ///
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.client.borrow_mut().set_timeout(timeout)
    }

    /// open a [LocalExec] channel which can excute commands
    ///
    pub fn open_exec(&mut self) -> SshResult<LocalExec<S>> {
        let channel = self.open_channel()?;
        channel.exec()
    }

    /// open a [LocalScp] channel which can download/upload files/directories
    ///
    #[cfg(feature = "scp")]
    pub fn open_scp(&mut self) -> SshResult<LocalScp<S>> {
        let channel = self.open_channel()?;
        channel.scp()
    }

    /// open a [LocalShell] channel which can download/upload files/directories
    ///
    pub fn open_shell(&mut self) -> SshResult<LocalShell<S>> {
        self.open_shell_terminal(TerminalSize::from(80, 24))
    }

    /// open a [LocalShell] channel
    ///
    /// custom terminal dimensions
    ///
    pub fn open_shell_terminal(&mut self, tv: TerminalSize) -> SshResult<LocalShell<S>> {
        let channel = self.open_channel()?;
        channel.shell(tv)
    }

    /// Open a [LocalShell] with a terminal type override and requested
    /// environment variables (ssh4 local addition).
    pub fn open_shell_terminal_env(
        &mut self,
        tv: TerminalSize,
        term: Option<&str>,
        envs: &[(String, String)],
    ) -> SshResult<LocalShell<S>> {
        let channel = self.open_channel()?;
        channel.shell_with(tv, term, envs)
    }

    /// Open an SFTP subsystem channel (ssh4 local addition).
    pub fn open_sftp(&mut self) -> SshResult<LocalSftp<S>> {
        let channel = self.open_channel()?;
        LocalSftp::open(channel)
    }

    pub fn get_raw_io(&mut self) -> RcMut<S> {
        self.stream.clone()
    }

    /// open a raw channel
    ///
    /// need call `.exec()`, `.shell()`, `.scp()` and so on to convert it to a specific channel
    ///
    pub fn open_channel(&mut self) -> SshResult<LocalChannel<S>> {
        info!("channel opened.");

        let client_channel_no = self.client.borrow_mut().alloc_channel_no();
        self.send_open_channel(client_channel_no)?;
        let (server_channel_no, remote_window_size) = self.receive_open_channel()?;
        self.client
            .borrow_mut()
            .register_channel(client_channel_no, server_channel_no);

        Ok(LocalChannel::new(
            server_channel_no,
            client_channel_no,
            remote_window_size,
            self.client.clone(),
            self.stream.clone(),
        ))
    }

    /// Open a `direct-tcpip` channel to `host:port` (local forward / ProxyJump hop).
    ///
    /// ssh4 local addition.
    pub fn open_direct_tcpip(
        &mut self,
        host: &str,
        port: u16,
    ) -> SshResult<crate::channel::LocalDirectTcpip<S>> {
        info!("direct-tcpip open to {host}:{port}");
        let client_channel_no = self.client.borrow_mut().alloc_channel_no();
        self.send_open_direct_tcpip(client_channel_no, host, port)?;
        let (server_channel_no, remote_window_size) = self.receive_open_channel()?;
        self.client
            .borrow_mut()
            .register_channel(client_channel_no, server_channel_no);
        let channel = LocalChannel::new(
            server_channel_no,
            client_channel_no,
            remote_window_size,
            self.client.clone(),
            self.stream.clone(),
        );
        Ok(crate::channel::LocalDirectTcpip::open(channel))
    }

    /// Request the server to listen for remote port forwards (`-R`).
    ///
    /// Returns the port actually bound (may differ when `bind_port` is 0).
    /// ssh4 local addition.
    pub fn request_tcpip_forward(&mut self, bind_addr: &str, bind_port: u16) -> SshResult<u16> {
        info!("tcpip-forward request {bind_addr}:{bind_port}");
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::GLOBAL_REQUEST)
            .put_str(ssh_str::TCPIP_FORWARD)
            .put_u8(true as u8)
            .put_str(bind_addr)
            .put_u32(bind_port as u32);
        data.pack(&mut self.client.borrow_mut())
            .write_stream(&mut *self.stream.borrow_mut())?;

        loop {
            let mut resp = Data::unpack(SecPacket::from_stream(
                &mut *self.stream.borrow_mut(),
                &mut self.client.borrow_mut(),
            )?)?;
            let message_code = resp.get_u8();
            match message_code {
                ssh_connection_code::REQUEST_SUCCESS => {
                    if bind_port == 0 {
                        if resp.len() != 4 {
                            return Err(SshError::GeneralError(
                                "Missing or malformed allocated forwarding port".into(),
                            ));
                        }
                        let port = resp.get_u32();
                        return u16::try_from(port)
                            .ok()
                            .filter(|port| *port != 0)
                            .ok_or_else(|| {
                                SshError::GeneralError("Invalid allocated forwarding port".into())
                            });
                    }
                    return Ok(bind_port);
                }
                ssh_connection_code::REQUEST_FAILURE => {
                    return Err(SshError::GeneralError(
                        "tcpip-forward request rejected by server".into(),
                    ));
                }
                ssh_connection_code::GLOBAL_REQUEST => {
                    self.reject_global_request(resp)?;
                }
                ssh_connection_code::CHANNEL_OPEN => {
                    self.confirm_or_reject_channel_open(resp)?;
                }
                ssh_connection_code::CHANNEL_DATA
                | ssh_connection_code::CHANNEL_EXTENDED_DATA
                | ssh_connection_code::CHANNEL_WINDOW_ADJUST
                | ssh_connection_code::CHANNEL_EOF
                | ssh_connection_code::CHANNEL_CLOSE
                | ssh_connection_code::CHANNEL_REQUEST
                | ssh_connection_code::CHANNEL_SUCCESS
                | ssh_connection_code::CHANNEL_FAILURE => {
                    self.demux_channel_msg(message_code, resp)?;
                }
                x => {
                    debug!("Ignore ssh msg {} while waiting for tcpip-forward reply", x);
                }
            }
        }
    }

    /// Take a `forwarded-tcpip` channel confirmed while another channel owned the
    /// stream (multi-channel demux). `None` if the queue is empty.
    pub fn take_pending_forwarded_tcpip(&mut self) -> Option<crate::channel::LocalDirectTcpip<S>> {
        self.take_pending_forwarded_tcpip_with_destination()
            .map(|(channel, _, _)| channel)
    }

    /// Take a queued channel together with the server-reported listener address
    /// and port. Callers must match these against their configured forwards.
    pub fn take_pending_forwarded_tcpip_with_destination(
        &mut self,
    ) -> Option<(crate::channel::LocalDirectTcpip<S>, String, u32)> {
        let meta = self.client.borrow_mut().pending_forwarded.pop_front()?;
        let channel = LocalChannel::new(
            meta.server_channel_no,
            meta.client_channel_no,
            meta.remote_window_size,
            self.client.clone(),
            self.stream.clone(),
        );
        Some((
            crate::channel::LocalDirectTcpip::open(channel),
            meta.connected_address,
            meta.connected_port,
        ))
    }

    /// Block until the server opens a `forwarded-tcpip` channel (remote forward accept).
    ///
    /// ssh4 local addition. Drains demux-queued opens first. Non-OPEN channel
    /// messages for already-registered channels are parked (not discarded) so
    /// concurrent `-R` relays keep working while this path owns the stream.
    pub fn accept_forwarded_tcpip(&mut self) -> SshResult<crate::channel::LocalDirectTcpip<S>> {
        if let Some(ch) = self.take_pending_forwarded_tcpip() {
            return Ok(ch);
        }
        loop {
            let mut data = Data::unpack(SecPacket::from_stream(
                &mut *self.stream.borrow_mut(),
                &mut self.client.borrow_mut(),
            )?)?;
            let message_code = data.get_u8();
            match message_code {
                ssh_connection_code::CHANNEL_OPEN => {
                    self.confirm_or_reject_channel_open(data)?;
                }
                ssh_connection_code::GLOBAL_REQUEST => {
                    self.reject_global_request(data)?;
                }
                // Demux channel traffic for live relays — never drop from the wire.
                ssh_connection_code::CHANNEL_DATA
                | ssh_connection_code::CHANNEL_EXTENDED_DATA
                | ssh_connection_code::CHANNEL_WINDOW_ADJUST
                | ssh_connection_code::CHANNEL_EOF
                | ssh_connection_code::CHANNEL_CLOSE
                | ssh_connection_code::CHANNEL_REQUEST
                | ssh_connection_code::CHANNEL_SUCCESS
                | ssh_connection_code::CHANNEL_FAILURE => {
                    self.demux_channel_msg(message_code, data)?;
                }
                x => {
                    debug!("Ignore ssh msg {} while waiting for forwarded-tcpip", x);
                }
            }
            if let Some(ch) = self.take_pending_forwarded_tcpip() {
                return Ok(ch);
            }
        }
    }

    fn reject_global_request(&mut self, mut data: Data) -> SshResult<()> {
        let _name = data.get_u8s();
        if data.get_u8() != 0 {
            let mut reply = Data::new();
            reply.put_u8(ssh_connection_code::REQUEST_FAILURE);
            reply
                .pack(&mut self.client.borrow_mut())
                .write_stream(&mut *self.stream.borrow_mut())?;
        }
        Ok(())
    }

    /// Park/route a channel-scoped message while accept owns the stream.
    /// Uses the same Client parking maps as `LocalChannel` demux.
    fn demux_channel_msg(&mut self, message_code: u8, mut data: Data) -> SshResult<()> {
        match message_code {
            ssh_connection_code::CHANNEL_DATA => {
                let cc = data.get_u32();
                let payload = data.get_u8s();
                self.park_foreign_data(cc, payload)?;
            }
            ssh_connection_code::CHANNEL_EXTENDED_DATA => {
                let cc = data.get_u32();
                let _dtype = data.get_u32();
                let payload = data.get_u8s();
                self.park_foreign_data(cc, payload)?;
            }
            ssh_connection_code::CHANNEL_WINDOW_ADJUST => {
                let cc = data.get_u32();
                let rws = data.get_u32();
                self.client.borrow_mut().park_window_credit(cc, rws);
            }
            ssh_connection_code::CHANNEL_CLOSE => {
                let cc = data.get_u32();
                self.client.borrow_mut().park_remote_close(cc);
            }
            ssh_connection_code::CHANNEL_EOF => {
                let cc = data.get_u32();
                self.client.borrow_mut().remote_eof.insert(cc);
            }
            ssh_connection_code::CHANNEL_FAILURE => {
                let cc = data.get_u32();
                let mut client = self.client.borrow_mut();
                if client.server_channel_for(cc).is_some() {
                    client.request_failures.insert(cc);
                }
            }
            ssh_connection_code::CHANNEL_REQUEST | ssh_connection_code::CHANNEL_SUCCESS => {
                // No action required for remote-forward accept.
            }
            _ => {}
        }
        Ok(())
    }

    fn park_foreign_data(&mut self, client_channel_no: u32, payload: Vec<u8>) -> SshResult<()> {
        let len = payload.len() as u32;
        let server_no = {
            let mut client = self.client.borrow_mut();
            client.park_channel_payload(client_channel_no, &payload);
            client.server_channel_for(client_channel_no)
        };
        if let Some(server_no) = server_no {
            let mut msg = Data::new();
            msg.put_u8(ssh_connection_code::CHANNEL_WINDOW_ADJUST)
                .put_u32(server_no)
                .put_u32(len);
            msg.pack(&mut self.client.borrow_mut())
                .write_stream(&mut *self.stream.borrow_mut())?;
        }
        Ok(())
    }

    fn confirm_or_reject_channel_open(&mut self, mut data: Data) -> SshResult<()> {
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
            fail.pack(&mut self.client.borrow_mut())
                .write_stream(&mut *self.stream.borrow_mut())?;
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
        conf.pack(&mut self.client.borrow_mut())
            .write_stream(&mut *self.stream.borrow_mut())?;

        let mut client = self.client.borrow_mut();
        client.register_channel(client_channel_no, server_channel_no);
        client.pending_forwarded.push_back(PendingForwardedMeta {
            server_channel_no,
            client_channel_no,
            remote_window_size,
            connected_address,
            connected_port,
        });
        Ok(())
    }

    // open channel request
    fn send_open_channel(&mut self, client_channel_no: u32) -> SshResult<()> {
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_OPEN)
            .put_str(ssh_str::SESSION)
            .put_u32(client_channel_no)
            .put_u32(size::LOCAL_WINDOW_SIZE)
            .put_u32(size::BUF_SIZE as u32);
        data.pack(&mut self.client.borrow_mut())
            .write_stream(&mut *self.stream.borrow_mut())
    }

    fn send_open_direct_tcpip(
        &mut self,
        client_channel_no: u32,
        host: &str,
        port: u16,
    ) -> SshResult<()> {
        // RFC 4254 §7.2
        let mut data = Data::new();
        data.put_u8(ssh_connection_code::CHANNEL_OPEN)
            .put_str(ssh_str::DIRECT_TCPIP)
            .put_u32(client_channel_no)
            .put_u32(size::LOCAL_WINDOW_SIZE)
            .put_u32(size::BUF_SIZE as u32)
            .put_str(host)
            .put_u32(port as u32)
            .put_str("127.0.0.1")
            .put_u32(0);
        data.pack(&mut self.client.borrow_mut())
            .write_stream(&mut *self.stream.borrow_mut())
    }

    // get the response of the channel request
    fn receive_open_channel(&mut self) -> SshResult<(u32, u32)> {
        loop {
            let mut data = Data::unpack(SecPacket::from_stream(
                &mut *self.stream.borrow_mut(),
                &mut self.client.borrow_mut(),
            )?)?;

            let message_code = data.get_u8();
            match message_code {
                ssh_connection_code::CHANNEL_OPEN => {
                    self.confirm_or_reject_channel_open(data)?;
                }
                // Successfully open a channel
                ssh_connection_code::CHANNEL_DATA
                | ssh_connection_code::CHANNEL_EXTENDED_DATA
                | ssh_connection_code::CHANNEL_WINDOW_ADJUST
                | ssh_connection_code::CHANNEL_EOF
                | ssh_connection_code::CHANNEL_CLOSE
                | ssh_connection_code::CHANNEL_REQUEST
                | ssh_connection_code::CHANNEL_FAILURE => {
                    self.demux_channel_msg(message_code, data)?;
                }
                ssh_connection_code::CHANNEL_OPEN_CONFIRMATION => {
                    data.get_u32();
                    let server_channel_no = data.get_u32();
                    let remote_window_size = data.get_u32();
                    // remote packet size, currently don't need it
                    data.get_u32();
                    return Ok((server_channel_no, remote_window_size));
                }
                /*
                    byte CHANNEL_OPEN_FAILURE
                    uint32 recipient channel
                    uint32 reason code
                    string description，ISO-10646 UTF-8 [RFC3629]
                    string language tag，[RFC3066]
                */
                // Fail to open a channel
                ssh_connection_code::CHANNEL_OPEN_FAILURE => {
                    data.get_u32();
                    // error code
                    let code = data.get_u32();
                    // error detail: By default is utf-8
                    let description =
                        String::from_utf8(data.get_u8s()).unwrap_or_else(|_| String::from("error"));
                    // language tag, assume to be en-US
                    data.get_u8s();

                    let err_msg = match code {
                        ssh_channel_fail_code::ADMINISTRATIVELY_PROHIBITED => {
                            format!("ADMINISTRATIVELY_PROHIBITED: {}", description)
                        }
                        ssh_channel_fail_code::CONNECT_FAILED => {
                            format!("CONNECT_FAILED: {}", description)
                        }
                        ssh_channel_fail_code::UNKNOWN_CHANNEL_TYPE => {
                            format!("UNKNOWN_CHANNEL_TYPE: {}", description)
                        }
                        ssh_channel_fail_code::RESOURCE_SHORTAGE => {
                            format!("RESOURCE_SHORTAGE: {}", description)
                        }
                        _ => description,
                    };
                    return Err(SshError::GeneralError(err_msg));
                }
                ssh_connection_code::GLOBAL_REQUEST => {
                    self.reject_global_request(data)?;
                    continue;
                }
                x => {
                    debug!("Ignore ssh msg {}", x);
                    continue;
                }
            }
        }
    }
}
