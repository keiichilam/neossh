//! Bounded DNS/TCP setup and cancellation-aware SSH transport.
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

static CONNECTING: AtomicUsize = AtomicUsize::new(0);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        CONNECTING.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Stream {
    socket: TcpStream,
    stop: Arc<AtomicBool>,
}
impl Stream {
    fn check(&self) -> io::Result<()> {
        if self.stop.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Connection cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}
impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.socket.read(bytes)
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        self.socket.flush()
    }
}

pub fn connect(
    host: String,
    port: u16,
    timeout: Duration,
    stop: &Arc<AtomicBool>,
) -> Result<Stream, String> {
    setup(timeout, stop, move || {
        (host.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| addresses.take(16).collect())
            .map_err(|e| format!("Host lookup failed: {e}"))
    })
}

fn setup(
    timeout: Duration,
    stop: &Arc<AtomicBool>,
    resolve: impl FnOnce() -> Result<Vec<SocketAddr>, String> + Send + 'static,
) -> Result<Stream, String> {
    if stop.load(Ordering::Acquire) {
        return Err("Connection cancelled.".into());
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or("Connection timeout is too large.")?;
    CONNECTING
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < 8).then_some(count + 1)
        })
        .map_err(|_| "Network setup workers are still busy. Try again shortly.")?;
    let guard = Guard;
    let cancellation = stop.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("ssh-network-setup".into())
        .spawn(move || {
            let _guard = guard;
            let result = (|| {
                let addresses = resolve()?;
                let mut error = "Host lookup returned no addresses.".to_string();
                for (index, address) in addresses.iter().enumerate() {
                    if cancellation.load(Ordering::Acquire) {
                        return Err("Connection cancelled.".into());
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err("Connection setup timed out.".into());
                    }
                    // Reserve time for other returned addresses if one silently drops SYNs.
                    let attempt = remaining / (addresses.len() - index) as u32;
                    match TcpStream::connect_timeout(address, attempt) {
                        Ok(socket) => {
                            socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                            socket.set_nodelay(true).map_err(|e| e.to_string())?;
                            return Ok(socket);
                        }
                        Err(e) => error = format!("TCP connection to {address} failed: {e}"),
                    }
                }
                Err(error)
            })();
            let _ = sender.send(result);
        })
        .map_err(|e| e.to_string())?;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Connection cancelled.".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Connection setup timed out.".into());
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(20))) {
            Ok(result) => {
                let stream = Stream {
                    socket: result?,
                    stop: stop.clone(),
                };
                stream.check().map_err(|e| e.to_string())?;
                return Ok(stream);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("Network setup worker ended.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_a_silent_ssh_handshake_closes_the_transport_promptly() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stream = setup(Duration::from_secs(2), &stop, move || Ok(vec![address])).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = ssh::create_session()
                .username("fixture")
                .password("unused")
                .timeout(Some(Duration::from_secs(15)))
                .connect_bio(stream);
            done_tx.send(result.err().map(|e| e.to_string())).unwrap();
        });
        let mut buffer = [0; 256];
        let count = peer.read(&mut buffer).unwrap();
        assert!(count > 0, "SSH must start negotiating before cancellation");
        stop.store(true, Ordering::Release);
        let error = done_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert!(error.to_lowercase().contains("cancel"), "{error}");
        worker.join().unwrap();
        assert_eq!(
            peer.read(&mut buffer).unwrap(),
            0,
            "SSH transport remained open"
        );
    }
    #[test]
    fn cancelled_and_expired_lookups_do_not_hold_the_caller() {
        for cancel in [true, false] {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let (release_tx, release_rx) = mpsc::sync_channel(1);
            let caller = std::thread::spawn(move || {
                setup(
                    if cancel {
                        Duration::from_secs(10)
                    } else {
                        Duration::from_millis(100)
                    },
                    &flag,
                    move || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(Vec::new())
                    },
                )
                .err()
                .unwrap()
            });
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let began = Instant::now();
            if cancel {
                stop.store(true, Ordering::Release);
            }
            let message = caller.join().unwrap();
            assert!(began.elapsed() < Duration::from_secs(1));
            assert!(message.contains(if cancel { "cancelled" } else { "timed out" }));
            release_tx.send(()).unwrap();
        }
    }
    #[test]
    fn falls_back_to_other_addresses_and_transport_checks_cancellation() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let mut stream = setup(Duration::from_secs(2), &stop, move || {
            Ok(vec!["127.0.0.1:0".parse().unwrap(), address])
        })
        .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        stream.write_all(b"test").unwrap();
        let mut buffer = [0; 4];
        peer.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"test");
        stop.store(true, Ordering::Release);
        assert_eq!(
            stream.read(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert_eq!(
            stream.write(b"input").unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert!(setup(
            Duration::from_secs(1),
            &Arc::new(AtomicBool::new(false)),
            || Ok(vec![])
        )
        .err()
        .unwrap()
        .contains("no addresses"));
    }
}
