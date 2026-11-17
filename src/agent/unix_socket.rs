//! Windows AF_UNIX agent sockets (SSH_AUTH_SOCK).
use std::{
    net::TcpStream,
    os::windows::io::FromRawSocket,
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinSock::*;

static WINSOCK: OnceLock<Result<(), i32>> = OnceLock::new();
fn socket_error() -> String {
    std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() }).to_string()
}

pub(super) fn exchange(
    path: &Path,
    body: &[u8],
    timeout: Duration,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    super::check_request(stop, deadline)?;
    let initialized = WINSOCK.get_or_init(|| {
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        let result = unsafe { WSAStartup(0x0202, &mut data) };
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    });
    if let Err(error) = initialized {
        return Err(std::io::Error::from_raw_os_error(*error).to_string());
    }
    let path = path
        .to_str()
        .ok_or("Agent socket path must be UTF-8.")?
        .as_bytes();
    let mut address = SOCKADDR_UN {
        sun_family: AF_UNIX,
        ..Default::default()
    };
    if path.is_empty() || path.len() >= address.sun_path.len() || path.contains(&0) {
        return Err("Invalid agent socket path.".into());
    }
    for (destination, source) in address.sun_path.iter_mut().zip(path) {
        *destination = *source as i8;
    }
    let raw = unsafe { socket(AF_UNIX as i32, SOCK_STREAM, 0) };
    if raw == INVALID_SOCKET {
        return Err(socket_error());
    }
    // TcpStream supplies byte-stream IO and ownership; no IP-specific methods are used.
    let mut stream = unsafe { TcpStream::from_raw_socket(raw as _) };
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    let connected = unsafe {
        connect(
            raw,
            &address as *const SOCKADDR_UN as *const SOCKADDR,
            std::mem::size_of::<SOCKADDR_UN>() as i32,
        )
    };
    if connected == SOCKET_ERROR {
        let error = unsafe { WSAGetLastError() };
        if !matches!(error, WSAEWOULDBLOCK | WSAEINPROGRESS) {
            return Err(std::io::Error::from_raw_os_error(error).to_string());
        }
        loop {
            super::check_request(stop, deadline)?;
            let mut poll = WSAPOLLFD {
                fd: raw,
                events: POLLWRNORM,
                revents: 0,
            };
            let ready = unsafe { WSAPoll(&mut poll, 1, 100) };
            if ready == SOCKET_ERROR {
                return Err(socket_error());
            }
            if ready > 0 {
                if let Some(error) = stream.take_error().map_err(|e| e.to_string())? {
                    return Err(error.to_string());
                }
                break;
            }
        }
    }
    super::stream_exchange(&mut stream, body, deadline, stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn windows_unix_socket_agent_round_trip() {
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { WSAStartup(0x0202, &mut data) }, 0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.sock");
        let text = path.to_str().unwrap().as_bytes();
        let mut address = SOCKADDR_UN {
            sun_family: AF_UNIX,
            ..Default::default()
        };
        assert!(text.len() < address.sun_path.len());
        for (dest, src) in address.sun_path.iter_mut().zip(text) {
            *dest = *src as i8;
        }
        let raw = unsafe { socket(AF_UNIX as i32, SOCK_STREAM, 0) };
        assert_ne!(raw, INVALID_SOCKET);
        let listener_owner = unsafe { TcpStream::from_raw_socket(raw as _) };
        listener_owner.set_nonblocking(true).unwrap();
        assert_eq!(
            unsafe {
                bind(
                    raw,
                    &address as *const SOCKADDR_UN as *const SOCKADDR,
                    std::mem::size_of::<SOCKADDR_UN>() as i32,
                )
            },
            0,
            "{}",
            socket_error()
        );
        assert_eq!(unsafe { listen(raw, 1) }, 0);
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let peer = loop {
                let peer = unsafe { accept(raw, std::ptr::null_mut(), std::ptr::null_mut()) };
                if peer != INVALID_SOCKET {
                    break peer;
                }
                assert_eq!(unsafe { WSAGetLastError() }, WSAEWOULDBLOCK);
                assert!(Instant::now() < deadline, "Agent client did not connect");
                std::thread::sleep(Duration::from_millis(10));
            };
            let mut stream = unsafe { TcpStream::from_raw_socket(peer as _) };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(request, [0, 0, 0, 1, 11]);
            stream.write_all(&[0, 0, 0, 5, 12, 0, 0, 0, 0]).unwrap();
            drop(listener_owner);
        });
        let result = exchange(
            &path,
            &[11],
            Duration::from_secs(2),
            &std::sync::atomic::AtomicBool::new(false),
        );
        server.join().unwrap();
        assert_eq!(result.unwrap(), [12, 0, 0, 0, 0]);
    }
}
