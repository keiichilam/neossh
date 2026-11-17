//! Nonblocking Unix agent setup; the caller owns every pending socket.
use socket2::{Domain, SockAddr, Socket, Type};
use std::{
    io,
    os::unix::{io::AsRawFd, net::UnixStream},
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(super) fn connect(
    path: &Path,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<UnixStream, String> {
    let address = SockAddr::unix(path).map_err(|error| error.to_string())?;
    loop {
        super::check_request(stop, deadline)?;
        let socket =
            Socket::new(Domain::UNIX, Type::STREAM, None).map_err(|error| error.to_string())?;
        socket
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        match socket.connect(&address) {
            Ok(()) => {
                super::check_request(stop, deadline)?;
                return Ok(socket.into());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // Linux AF_UNIX reports EAGAIN when the listener backlog is full.
                // No connection is pending: retry with a fresh owned descriptor.
                drop(socket);
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) if error.raw_os_error() == Some(libc::EINPROGRESS) => {
                loop {
                    super::check_request(stop, deadline)?;
                    let mut event = libc::pollfd {
                        fd: socket.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    let wait = deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(25) as i32;
                    // The descriptor stays owned by socket; poll has one initialized
                    // element and waits at most 25 ms before checking cancellation.
                    let result = unsafe { libc::poll(&mut event, 1, wait) };
                    if result < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(error.to_string());
                    }
                    if result == 0 {
                        continue;
                    }
                    if let Some(error) = socket.take_error().map_err(|error| error.to_string())? {
                        return Err(error.to_string());
                    }
                    socket.peer_addr().map_err(|error| error.to_string())?;
                    super::check_request(stop, deadline)?;
                    return Ok(socket.into());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        sync::atomic::Ordering,
    };

    #[test]
    fn connects_nonblocking_and_missing_or_cancelled_endpoints_fail() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent.sock");
        let stop = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(2);
        assert!(connect(&path, deadline, &stop).is_err());
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut client = connect(&path, deadline, &stop).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        assert_eq!(
            client.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        client.write_all(b"agent").unwrap();
        let mut data = [0; 5];
        server.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"agent");
        stop.store(true, Ordering::Release);
        assert!(connect(&path, deadline, &stop)
            .unwrap_err()
            .contains("cancelled"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn full_listener_backlog_obeys_deadline_and_cancellation() {
        use std::sync::Arc;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent.sock");
        let address = SockAddr::unix(&path).unwrap();
        let listener = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        listener.bind(&address).unwrap();
        listener.listen(0).unwrap();
        let first = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        first.connect(&address).unwrap();
        let start = Instant::now();
        assert!(connect(
            &path,
            start + Duration::from_millis(70),
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(1));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            connect(&path, Instant::now() + Duration::from_secs(5), &worker_stop)
        });
        std::thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        stop.store(true, Ordering::Release);
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
