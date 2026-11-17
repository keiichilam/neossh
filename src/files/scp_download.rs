//! Single-file legacy SCP receiver. Server filenames never select local paths.
use super::{scp::acknowledgement, Progress, RemoteFingerprint};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(PartialEq)]
enum Stage {
    Header,
    Data,
    Status,
    Closing,
}

pub(super) struct Job<S: Read + Write> {
    channel: ssh::LocalExec<S>,
    file: Option<crate::download::StagedDownload>,
    stage: Stage,
    incoming: Vec<u8>,
    ack: bool,
    remaining: u64,
    /// Edit sessions: the reviewed listing the file must still match.
    expected: Option<RemoteFingerprint>,
    /// The `T` (times) record matched `expected`.
    time_checked: bool,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}

/// `preserve` asks the source for a `T` record with the modification time.
fn command(remote: &str, preserve: bool) -> Result<String, String> {
    if remote.is_empty() || remote.contains(['\0', '\n', '\r']) || remote.ends_with('/') {
        return Err("Choose a single remote SCP filename without NUL or line breaks.".into());
    }
    let remote = if remote.starts_with('/') || remote.starts_with("./") {
        remote.to_owned()
    } else {
        format!("./{remote}")
    };
    let flags = if preserve { "-p -f" } else { "-f" };
    Ok(format!("exec scp {flags} {}", super::scp::quote(&remote)))
}

/// Modification time from an SCP `T<mtime> <usec> <atime> <usec>` record.
fn times(line: &[u8]) -> Result<u32, String> {
    let invalid = || "Invalid SCP time record.".to_owned();
    let text =
        std::str::from_utf8(line.strip_prefix(b"T").ok_or_else(invalid)?).map_err(|_| invalid())?;
    let fields: Vec<&str> = text.split(' ').collect();
    if fields.len() != 4
        || !fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    fields[0].parse().map_err(|_| invalid())
}

fn header(line: &[u8]) -> Result<u64, String> {
    // Four octal mode digits, decimal length, then a single basename. Modes and
    // names are intentionally not applied to the reviewed local destination.
    if line.len() < 9
        || line[0] != b'C'
        || !line[1..5].iter().all(|b| (b'0'..=b'7').contains(b))
        || line[5] != b' '
    {
        return Err("Invalid SCP file header; only one regular file is supported.".into());
    }
    let tail = &line[6..];
    let split = tail
        .iter()
        .position(|b| *b == b' ')
        .ok_or("Invalid SCP file length.")?;
    let size = &tail[..split];
    if size.is_empty() || !size.iter().all(u8::is_ascii_digit) {
        return Err("Invalid SCP file length.".into());
    }
    let size = std::str::from_utf8(size)
        .unwrap()
        .parse::<u64>()
        .map_err(|_| "SCP file length exceeds the supported range.")?;
    let name = &tail[split + 1..];
    if name.is_empty()
        || matches!(name, b"." | b"..")
        || name
            .iter()
            .any(|b| matches!(b, 0 | b'/' | b'\\' | b'\r' | b'\n'))
    {
        return Err("Invalid SCP server filename.".into());
    }
    Ok(size)
}

impl<S: Read + Write> Job<S> {
    pub(super) fn new(
        session: &mut ssh::LocalSession<S>,
        local: PathBuf,
        remote: &str,
        expected: Option<RemoteFingerprint>,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let command = command(remote, expected.is_some())?;
        let file = crate::download::StagedDownload::new(local)?;
        let mut channel = session.open_exec().map_err(|e| e.to_string())?;
        channel.exec_command(&command).map_err(|e| e.to_string())?;
        Ok(Self {
            channel,
            file: Some(file),
            stage: Stage::Header,
            incoming: Vec::new(),
            ack: true,
            remaining: 0,
            expected,
            time_checked: false,
            progress,
            cancel,
            deadline: Instant::now() + Duration::from_secs(30),
        })
    }
    pub(super) fn poll(&mut self) -> Result<bool, String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("SCP download cancelled.".into());
        }
        if Instant::now() >= self.deadline {
            return Err("SCP download timed out.".into());
        }
        if self.ack {
            if self.channel.try_write(&[0]).map_err(|e| e.to_string())? == 0 {
                return Ok(false);
            }
            self.ack = false;
            self.deadline = Instant::now() + Duration::from_secs(30);
            if self.stage == Stage::Closing {
                self.channel.send_eof().map_err(|e| e.to_string())?;
            }
        }
        // Drain buffered file data before accepting another packet, so a fast
        // sender cannot outpace bounded disk writes and exhaust the input cap.
        if let Some(bytes) = if self.incoming.len() <= 32768 {
            self.channel.try_read().map_err(|e| e.to_string())?
        } else {
            None
        } {
            if !bytes.is_empty() {
                self.deadline = Instant::now() + Duration::from_secs(30);
            }
            if self.incoming.len() + bytes.len() > 1024 * 1024 {
                return Err("SCP input exceeds the receive limit.".into());
            }
            self.incoming.extend(bytes);
        }
        match self.stage {
            Stage::Header => {
                if matches!(self.incoming.first(), Some(1 | 2)) {
                    acknowledgement(&mut self.incoming)?;
                } else if let Some(end) = self.incoming.iter().position(|b| *b == b'\n') {
                    if end > 4096 {
                        return Err("SCP file header exceeds 4 KiB.".into());
                    }
                    let changed = || {
                        "The remote file changed after the folder was listed. Refresh and try again."
                            .to_owned()
                    };
                    if let (Some(expected), Some(b'T')) = (self.expected, self.incoming.first()) {
                        // Edit sessions: `scp -p` reports the modification
                        // time first; it must match the reviewed listing.
                        if self.time_checked || times(&self.incoming[..end])? != expected.modified {
                            return Err(changed());
                        }
                        self.time_checked = true;
                        self.incoming.drain(..=end);
                        self.ack = true;
                        return Ok(false);
                    }
                    self.remaining = header(&self.incoming[..end])?;
                    // Edit sessions record the listed size and time; either
                    // differing means the server copy changed after review.
                    if let Some(expected) = self.expected {
                        if !self.time_checked {
                            return Err(
                                "The server did not report the remote file's modification time."
                                    .into(),
                            );
                        }
                        if expected.size != self.remaining {
                            return Err(changed());
                        }
                    }
                    self.progress.total.store(self.remaining, Ordering::Relaxed);
                    self.incoming.drain(..=end);
                    self.stage = Stage::Data;
                    self.ack = true;
                } else if self.incoming.len() > 4096 {
                    return Err("SCP file header exceeds 4 KiB.".into());
                }
            }
            Stage::Data => {
                let count = self.remaining.min(self.incoming.len() as u64).min(32768) as usize;
                if count > 0 {
                    let file = self.file.as_mut().ok_or("Download already finished.")?;
                    file.append(&self.incoming[..count])?;
                    self.incoming.drain(..count);
                    self.remaining -= count as u64;
                    self.progress.bytes.store(file.bytes, Ordering::Relaxed);
                }
                if self.remaining == 0 {
                    self.stage = Stage::Status;
                }
            }
            Stage::Status => {
                if acknowledgement(&mut self.incoming)? {
                    self.stage = Stage::Closing;
                    self.ack = true;
                }
            }
            Stage::Closing => {
                if !self.incoming.is_empty() {
                    return Err("SCP sent unexpected data after the requested file.".into());
                }
                if self.channel.remote_closed() {
                    if self.channel.received_exit_status() != Some(0) {
                        return Err(
                            "SCP download did not finish with a successful exit status.".into()
                        );
                    }
                    self.file
                        .take()
                        .ok_or("Download already finished.")?
                        .finish()?;
                    return Ok(true);
                }
            }
        }
        // A close may be observed together with buffered payload; consume that
        // payload on subsequent bounded polls before treating it as truncation.
        if self.channel.remote_closed()
            && !self.ack
            && self.incoming.is_empty()
            && self.stage != Stage::Closing
        {
            return Err("SCP channel closed before download completion.".into());
        }
        Ok(false)
    }
}
impl<S: Read + Write> Drop for Job<S> {
    fn drop(&mut self) {
        self.channel.force_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_scp_source_emits_one_literal_file_and_binary_contents() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        let directory = tempfile::tempdir().unwrap();
        let name = "-literal '$(touch sentinel).bin";
        std::fs::write(directory.path().join(name), b"A\0B\xff").unwrap();
        let mut child = std::process::Command::new(shell)
            .args(["-c", &command(name, false).unwrap()])
            .current_dir(directory.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"\0\0\0").unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let end = result.stdout.iter().position(|b| *b == b'\n').unwrap();
        assert_eq!(header(&result.stdout[..end]).unwrap(), 4);
        assert_eq!(&result.stdout[end + 1..], b"A\0B\xff\0");
        assert!(!directory.path().join("sentinel").exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn rejects_malformed_lengths_recursive_records_and_server_path_traversal() {
        assert_eq!(header(b"C0644 0 empty").unwrap(), 0);
        assert_eq!(header(b"C0600 123 name with spaces").unwrap(), 123);
        for bad in [
            b"C0644 -1 bad".as_slice(),
            b"C0644 18446744073709551616 bad",
            b"D0755 0 folder",
            b"C0999 2 bad",
            b"C0644 1 ../escape",
            b"C0644 1 ..",
            b"C0644 1 /absolute",
            b"C0644 1 ",
            b"C0644 1 a\0b",
        ] {
            assert!(header(bad).is_err(), "{bad:?}");
        }
    }
    #[test]
    fn remote_path_is_one_literal_shell_argument() {
        assert_eq!(
            command("-file '$(touch bad)", false).unwrap(),
            "exec scp -f './-file '\\''$(touch bad)'"
        );
        assert_eq!(command("/a b", true).unwrap(), "exec scp -p -f '/a b'");
        assert!(command("/folder/", false).is_err());
        assert!(command("line\nbreak", true).is_err());
    }
    #[test]
    fn real_scp_source_reports_the_modification_time_first_when_preserving() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        if !std::path::Path::new(shell).is_file() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("edited.conf");
        std::fs::write(&path, b"abc").unwrap();
        let modified = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut child = std::process::Command::new(shell)
            .args(["-c", &command("edited.conf", true).unwrap()])
            .current_dir(directory.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"\0\0\0\0").unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        let mut lines = result.stdout.split(|b| *b == b'\n');
        assert_eq!(u64::from(times(lines.next().unwrap()).unwrap()), modified);
        assert_eq!(header(lines.next().unwrap()).unwrap(), 3);
    }
    #[test]
    fn time_records_need_four_decimal_fields() {
        assert_eq!(times(b"T1700000000 0 1700000001 0"), Ok(1_700_000_000));
        for bad in [
            &b"T1700000000 0 1700000001"[..],
            b"T-1 0 1 0",
            b"T 0 1 0",
            b"C0644 1 x",
            b"T99999999999 0 1 0",
            b"T1 0 1 0 ",
        ] {
            assert!(times(bad).is_err(), "{bad:?}");
        }
    }
}
