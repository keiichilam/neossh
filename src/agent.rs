//! Bounded SSH-agent RPC. Identities retain the backend that supplied them.
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_PACKET: usize = 1024 * 1024;
#[cfg(windows)]
pub(crate) mod pageant;
#[cfg(unix)]
#[path = "agent/unix_connect.rs"]
mod unix_connect;
#[cfg(windows)]
mod unix_socket;
#[derive(Clone)]
pub struct Identity {
    pub blob: Vec<u8>,
    pub comment: String,
    endpoint: PathBuf,
}

pub fn identities(stop: &AtomicBool) -> Result<Vec<Identity>, String> {
    let explicit = std::env::var_os("SSH_AUTH_SOCK")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    for endpoints in endpoint_plan(explicit, default_endpoints()) {
        let mut result = Vec::new();
        let mut reachable = false;
        for endpoint in endpoints {
            check_request(stop, Instant::now() + Duration::from_secs(5))?;
            if let Ok(keys) = exchange(&endpoint, &[11], Duration::from_secs(5), stop)
                .and_then(|response| parse_identities(&response, &endpoint))
            {
                reachable = true;
                result.extend(keys);
            }
        }
        if reachable {
            return Ok(result);
        }
    }
    Err("No configured SSH agent is available.".into())
}

/// Agent endpoints to try, in groups: the first group with a reachable agent
/// supplies the identities. `SSH_AUTH_SOCK` comes first. When it is
/// unreachable the platform defaults follow; on Windows, Git Bash and MSYS
/// often export an emulated socket path that native programs cannot open,
/// which would otherwise hide Pageant and the OpenSSH agent.
fn endpoint_plan(explicit: Option<PathBuf>, defaults: Vec<PathBuf>) -> Vec<Vec<PathBuf>> {
    match explicit {
        Some(path) => {
            let fallback: Vec<PathBuf> = defaults.into_iter().filter(|d| *d != path).collect();
            [vec![path], fallback]
                .into_iter()
                .filter(|group| !group.is_empty())
                .collect()
        }
        None => vec![defaults],
    }
}

fn default_endpoints() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            PathBuf::from(r"\\.\pipe\openssh-ssh-agent"),
            PathBuf::from("pageant:"),
        ]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

impl Identity {
    pub fn fingerprint(&self) -> String {
        use base64::Engine;
        use sha2::Digest;
        format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD
                .encode(sha2::Sha256::digest(&self.blob))
        )
    }
    pub fn sign(&self, data: &[u8], flags: u32, stop: &AtomicBool) -> Result<Vec<u8>, String> {
        let mut request = vec![13];
        put_string(&mut request, &self.blob)?;
        put_string(&mut request, data)?;
        request.extend_from_slice(&flags.to_be_bytes());
        let response = exchange(&self.endpoint, &request, Duration::from_secs(120), stop)?;
        let mut reader = Reader(&response);
        if reader.byte()? != 14 {
            return Err("SSH agent declined signing.".into());
        }
        let signature = reader.string()?.to_vec();
        if !reader.0.is_empty() {
            return Err("Trailing SSH agent signature data.".into());
        }
        Ok(signature)
    }
}

fn parse_identities(bytes: &[u8], endpoint: &std::path::Path) -> Result<Vec<Identity>, String> {
    let mut reader = Reader(bytes);
    if reader.byte()? != 12 {
        return Err("SSH agent declined listing identities.".into());
    }
    let count = reader.u32()? as usize;
    if count > 2048 {
        return Err("SSH agent returned too many identities.".into());
    }
    let mut identities = Vec::new();
    for _ in 0..count {
        let blob = reader.string()?.to_vec();
        let comment = String::from_utf8_lossy(reader.string()?).into_owned();
        if blob.is_empty() {
            return Err("SSH agent returned an empty key.".into());
        }
        identities.push(Identity {
            blob,
            comment,
            endpoint: endpoint.into(),
        });
    }
    if !reader.0.is_empty() {
        return Err("Trailing SSH agent identity data.".into());
    }
    Ok(identities)
}

fn put_string(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), String> {
    if value.len() > MAX_PACKET {
        return Err("SSH agent payload is too large.".into());
    }
    bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
    bytes.extend_from_slice(value);
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let value = self
            .0
            .get(..length)
            .ok_or("Truncated SSH agent response.")?;
        self.0 = &self.0[length..];
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

fn check_request(stop: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if stop.load(Ordering::Acquire) {
        return Err("Authentication cancelled.".into());
    }
    if Instant::now() >= deadline {
        return Err("SSH agent request timed out.".into());
    }
    Ok(())
}

/// Byte-stream RPC with a total deadline and resumable partial reads/writes.
/// The caller must configure nonblocking IO before entering this function.
fn stream_exchange(
    stream: &mut (impl std::io::Read + std::io::Write),
    body: &[u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut transfer = |buffer: &mut [u8], write: bool| -> Result<(), String> {
        let mut at = 0;
        while at < buffer.len() {
            check_request(stop, deadline)?;
            let result = if write {
                stream.write(&buffer[at..])
            } else {
                stream.read(&mut buffer[at..])
            };
            match result {
                Ok(0) => return Err("SSH agent closed the socket.".into()),
                Ok(count) => at += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    };
    let mut request = Vec::new();
    put_string(&mut request, body)?;
    transfer(&mut request, true)?;
    let mut header = [0; 4];
    transfer(&mut header, false)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_PACKET {
        return Err("Invalid SSH agent packet size.".into());
    }
    let mut response = vec![0; length];
    transfer(&mut response, false)?;
    Ok(response)
}

#[cfg(unix)]
fn exchange(
    endpoint: &std::path::Path,
    body: &[u8],
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    check_request(stop, deadline)?;
    let mut socket = unix_connect::connect(endpoint, deadline, stop)?;
    stream_exchange(&mut socket, body, deadline, stop)
}

#[cfg(windows)]
fn exchange(
    endpoint: &std::path::Path,
    body: &[u8],
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if endpoint == std::path::Path::new("pageant:") {
        return pageant::exchange_cancellable(body, timeout, stop);
    }
    use std::{os::windows::ffi::OsStrExt, ptr, time::Instant};
    use windows_sys::Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Pipes::WaitNamedPipeW, Threading::CreateEventW, IO::*},
    };
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let path = endpoint.to_string_lossy();
    if !path.starts_with(r"\\.\pipe\") {
        return unix_socket::exchange(endpoint, body, timeout, stop);
    }
    let wide: Vec<u16> = endpoint.as_os_str().encode_wide().chain(Some(0)).collect();
    let deadline = Instant::now() + timeout;
    loop {
        check_request(stop, deadline)?;
        if unsafe { WaitNamedPipeW(wide.as_ptr(), 50) } != 0 {
            break;
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_SEM_TIMEOUT {
            return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
        }
    }
    check_request(stop, deadline)?;
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let handle = Handle(handle);
    let transfer = |buffer: &mut [u8], write: bool| -> Result<(), String> {
        let mut at = 0;
        while at < buffer.len() {
            check_request(stop, deadline)?;
            let event = Handle(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) });
            if event.0.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = event.0;
            let mut count = 0;
            let success = unsafe {
                if write {
                    WriteFile(
                        handle.0,
                        buffer[at..].as_ptr(),
                        (buffer.len() - at) as u32,
                        ptr::null_mut(),
                        &mut overlapped,
                    )
                } else {
                    ReadFile(
                        handle.0,
                        buffer[at..].as_mut_ptr(),
                        (buffer.len() - at) as u32,
                        ptr::null_mut(),
                        &mut overlapped,
                    )
                }
            };
            if success == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let completion = loop {
                if let Err(error) = check_request(stop, deadline) {
                    break Err(error);
                }
                let done =
                    unsafe { GetOverlappedResultEx(handle.0, &overlapped, &mut count, 50, 0) };
                if done != 0 {
                    break Ok(());
                }
                let error = unsafe { GetLastError() };
                if error != WAIT_TIMEOUT && error != ERROR_IO_INCOMPLETE {
                    break Err(std::io::Error::from_raw_os_error(error as i32).to_string());
                }
            };
            if let Err(error) = completion {
                // Settle cancellation before the OS can outlive the borrowed buffer.
                unsafe {
                    CancelIoEx(handle.0, &overlapped);
                    GetOverlappedResult(handle.0, &overlapped, &mut count, 1);
                }
                return Err(error);
            }
            if count == 0 {
                return Err("SSH agent closed the pipe.".into());
            }
            at += count as usize;
        }
        Ok(())
    };
    let mut request = Vec::new();
    put_string(&mut request, body)?;
    transfer(&mut request, true)?;
    let mut header = [0; 4];
    transfer(&mut header, false)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_PACKET {
        return Err("Invalid SSH agent packet size.".into());
    }
    let mut response = vec![0; length];
    transfer(&mut response, false)?;
    Ok(response)
}

#[cfg(not(any(windows, unix)))]
fn exchange(_: &std::path::Path, _: &[u8], _: Duration, _: &AtomicBool) -> Result<Vec<u8>, String> {
    Err("SSH agents are not supported on this platform.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_agent_socket_falls_back_to_the_defaults_only_when_unreachable() {
        let path = |text: &str| PathBuf::from(text);
        let defaults = vec![path(r"\\.\pipe\openssh-ssh-agent"), path("pageant:")];
        assert_eq!(
            endpoint_plan(None, defaults.clone()),
            vec![defaults.clone()]
        );
        assert_eq!(
            endpoint_plan(Some(path("/tmp/ssh-x/agent.1")), defaults.clone()),
            [vec![path("/tmp/ssh-x/agent.1")], defaults.clone()]
        );
        // Naming a default does not try it twice.
        assert_eq!(
            endpoint_plan(Some(path("pageant:")), defaults),
            [
                vec![path("pageant:")],
                vec![path(r"\\.\pipe\openssh-ssh-agent")]
            ]
        );
        // Unix has no defaults: SSH_AUTH_SOCK alone, as before.
        assert_eq!(
            endpoint_plan(Some(path("/run/agent")), Vec::new()),
            [vec![path("/run/agent")]]
        );
    }
    #[test]
    fn socket_request_cancellation_interrupts_a_partial_response() {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
            sync::Arc,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let token = stop.clone();
        let client = std::thread::spawn(move || {
            let mut socket = TcpStream::connect(address).unwrap();
            socket.set_nonblocking(true).unwrap();
            stream_exchange(
                &mut socket,
                &[11],
                Instant::now() + Duration::from_secs(120),
                &token,
            )
        });
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut request = [0; 5];
        peer.read_exact(&mut request).unwrap();
        assert_eq!(request, [0, 0, 0, 1, 11]);
        peer.write_all(&[0, 0]).unwrap();
        let started = Instant::now();
        stop.store(true, Ordering::Release);
        assert!(client.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
        match peer.read(&mut [0]) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("Cancelled agent socket remained open: {result:?}"),
        }
    }

    #[test]
    fn socket_rpc_preserves_fragments_and_enforces_total_deadline() {
        struct Fragmented {
            bytes: std::collections::VecDeque<u8>,
            blocked: bool,
            written: Vec<u8>,
        }
        impl std::io::Read for Fragmented {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.blocked = !self.blocked;
                if self.blocked {
                    return Err(std::io::ErrorKind::WouldBlock.into());
                }
                match self.bytes.pop_front() {
                    Some(byte) => {
                        buffer[0] = byte;
                        Ok(1)
                    }
                    None => Err(std::io::ErrorKind::WouldBlock.into()),
                }
            }
        }
        impl std::io::Write for Fragmented {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.written.push(buffer[0]);
                Ok(1)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let stop = AtomicBool::new(false);
        let mut stream = Fragmented {
            bytes: [0, 0, 0, 5, 12, 0, 0, 0, 0].into(),
            blocked: false,
            written: Vec::new(),
        };
        assert_eq!(
            stream_exchange(
                &mut stream,
                &[11],
                Instant::now() + Duration::from_secs(1),
                &stop
            )
            .unwrap(),
            [12, 0, 0, 0, 0]
        );
        assert_eq!(stream.written, [0, 0, 0, 1, 11]);
        assert!(stream_exchange(
            &mut stream,
            &[11],
            Instant::now() + Duration::from_millis(20),
            &stop
        )
        .unwrap_err()
        .contains("timed out"));
    }
    #[test]
    fn malformed_agent_packets_fail_without_panics_or_unbounded_allocations() {
        for data in [
            &[][..],
            &[12, 255, 255, 255, 255],
            &[12, 0, 0, 0, 1, 255, 255, 255, 255],
            &[12, 0, 0, 0, 0, 0],
        ] {
            assert!(parse_identities(data, std::path::Path::new("agent")).is_err());
        }
        assert!(
            parse_identities(&[12, 0, 0, 0, 0], std::path::Path::new("agent"))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn routes_each_identity_to_its_original_endpoint() {
        let mut response = vec![12, 0, 0, 0, 1];
        put_string(&mut response, b"keyblob").unwrap();
        put_string(&mut response, b"label").unwrap();
        let keys = parse_identities(&response, std::path::Path::new("agent-a")).unwrap();
        assert_eq!(keys[0].endpoint, PathBuf::from("agent-a"));
        assert_eq!(keys[0].blob, b"keyblob");
    }
}
