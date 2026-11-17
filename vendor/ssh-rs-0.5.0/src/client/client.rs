use crate::model::U32Iter;
use crate::{
    algorithm::compression::{CompressNone, Compression},
    config::algorithm::AlgList,
};
use crate::{algorithm::encryption::Encryption, config::Config};
use crate::{algorithm::encryption::EncryptionNone, model::Sequence};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

/// Metadata for a `forwarded-tcpip` channel confirmed while another channel
/// owned the stream read path (ssh4 multi-channel demux).
#[derive(Clone, Debug)]
pub(crate) struct PendingForwardedMeta {
    pub server_channel_no: u32,
    pub client_channel_no: u32,
    pub remote_window_size: u32,
    pub connected_address: String,
    pub connected_port: u32,
}

// the underlay connection
pub(crate) struct Client {
    pub(super) host_key: crate::host_key::Context,
    pub(super) sequence: Sequence,
    pub(super) config: Config,
    pub(super) negotiated: AlgList,
    pub(super) encryptor: Box<dyn Encryption>,
    pub(super) compressor: Box<dyn Compression>,
    pub(super) session_id: Vec<u8>,
    /// ssh4 local modification: bytes of the in-flight inbound packet.
    /// Reads that stop short (nonblocking stream, read timeout) park their
    /// partial packet here and resume on the next call, instead of dropping
    /// bytes and desynchronizing the transport.
    pub(crate) recv_buf: Vec<u8>,
    /// Shared channel id allocator (session + demux).
    pub(crate) channel_num: U32Iter,
    /// Inbound CHANNEL_DATA parked for a channel that was not reading.
    pub(crate) channel_inbox: HashMap<u32, VecDeque<u8>>,
    /// Remote window credits parked for channels that were not reading.
    pub(crate) window_credits: HashMap<u32, u32>,
    /// client_channel_no → server_channel_no for window-adjust routing.
    pub(crate) channel_peers: HashMap<u32, u32>,
    /// client channels the peer closed while another channel was reading.
    pub(crate) remote_closed: HashSet<u32>,
    pub(crate) remote_eof: HashSet<u32>,
    pub(crate) request_failures: HashSet<u32>,
    pub(crate) exit_statuses: HashMap<u32, u32>,
    /// Confirmed forwarded-tcpip opens waiting for accept.
    pub(crate) pending_forwarded: VecDeque<PendingForwardedMeta>,
}

impl Client {
    pub fn new(config: Config) -> Self {
        Self {
            host_key: crate::host_key::Context::capture(),
            config,
            encryptor: Box::<EncryptionNone>::default(),
            compressor: Box::<CompressNone>::default(),
            negotiated: AlgList::new(),
            session_id: vec![],
            sequence: Sequence::new(),
            recv_buf: vec![],
            channel_num: U32Iter::default(),
            channel_inbox: HashMap::new(),
            window_credits: HashMap::new(),
            channel_peers: HashMap::new(),
            remote_closed: HashSet::new(),
            remote_eof: HashSet::new(),
            request_failures: HashSet::new(),
            exit_statuses: HashMap::new(),
            pending_forwarded: VecDeque::new(),
        }
    }

    pub(crate) fn alloc_channel_no(&mut self) -> u32 {
        self.channel_num.next().unwrap()
    }

    pub(crate) fn register_channel(&mut self, client_no: u32, server_no: u32) {
        self.channel_peers.insert(client_no, server_no);
    }

    pub(crate) fn unregister_channel(&mut self, client_no: u32) {
        self.channel_peers.remove(&client_no);
        self.channel_inbox.remove(&client_no);
        self.window_credits.remove(&client_no);
        self.remote_closed.remove(&client_no);
        self.remote_eof.remove(&client_no);
        self.request_failures.remove(&client_no);
        self.exit_statuses.remove(&client_no);
    }

    /// Park CHANNEL_DATA for a channel that is not currently reading the stream.
    pub(crate) fn park_channel_payload(&mut self, client_no: u32, payload: &[u8]) {
        if payload.is_empty() {
            return;
        }
        self.channel_inbox
            .entry(client_no)
            .or_default()
            .extend(payload);
    }

    /// Drain all parked payload for `client_no` (may be empty).
    pub(crate) fn take_parked_payload(&mut self, client_no: u32) -> Vec<u8> {
        self.channel_inbox
            .remove(&client_no)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }

    pub(crate) fn park_window_credit(&mut self, client_no: u32, rws: u32) {
        if rws == 0 {
            return;
        }
        *self.window_credits.entry(client_no).or_insert(0) += rws;
    }

    pub(crate) fn park_remote_close(&mut self, client_no: u32) {
        self.remote_closed.insert(client_no);
    }

    pub(crate) fn server_channel_for(&self, client_no: u32) -> Option<u32> {
        self.channel_peers.get(&client_no).copied()
    }

    pub fn get_encryptor(&mut self) -> &mut dyn Encryption {
        self.encryptor.as_mut()
    }

    pub fn get_compressor(&mut self) -> &mut dyn Compression {
        self.compressor.as_mut()
    }

    pub fn get_seq(&mut self) -> &mut Sequence {
        &mut self.sequence
    }

    pub fn get_timeout(&self) -> Option<Duration> {
        self.config.timeout
    }

    pub fn set_timeout(&mut self, tm: Option<Duration>) {
        self.config.timeout = tm
    }
}

#[cfg(test)]
mod demux_tests {
    use super::*;

    #[test]
    fn park_and_take_channel_payload_roundtrip() {
        let mut client = Client::new(Config::default());
        client.register_channel(7, 70);
        client.park_channel_payload(7, b"hello");
        client.park_channel_payload(7, b" world");
        let got = client.take_parked_payload(7);
        assert_eq!(got, b"hello world");
        assert!(client.take_parked_payload(7).is_empty());
    }

    #[test]
    fn park_window_and_close_flags() {
        let mut client = Client::new(Config::default());
        client.register_channel(3, 30);
        client.park_window_credit(3, 100);
        client.park_window_credit(3, 50);
        assert_eq!(client.window_credits.get(&3).copied(), Some(150));
        client.park_remote_close(3);
        assert!(client.remote_closed.contains(&3));
        client.unregister_channel(3);
        assert!(!client.remote_closed.contains(&3));
        assert!(client.window_credits.get(&3).is_none());
    }
}
