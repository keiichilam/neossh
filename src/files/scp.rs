//! Incremental legacy SCP upload, staged before atomic no-replace publication
//! (or, for edit sessions only, a fingerprint-checked atomic replacement).
use super::{edit, Progress, RemoteFingerprint};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(PartialEq)]
enum Stage {
    Greeting,
    Header,
    Data,
    FinalAck,
    Closing,
}
pub(super) struct Job<S: Read + Write> {
    channel: ssh::LocalExec<S>,
    upload: crate::upload::Upload,
    stage: Stage,
    outgoing: Vec<u8>,
    offset: usize,
    incoming: Vec<u8>,
    written: u64,
    marker: Vec<u8>,
    /// Edit-session replacement: (completion marker, conflict marker).
    replace: Option<(String, String)>,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}
/// Whether the upload script printed its completion marker (`marker_line`,
/// newline included) as a whole line. It does so only after `ln -T` published
/// the file under `set -e`, so output around it (a login or logout script)
/// must not turn a completed upload into a reported failure.
fn published(incoming: &[u8], marker_line: &[u8]) -> bool {
    let marker = marker_line.strip_suffix(b"\n").unwrap_or(marker_line);
    let mut lines: Vec<&[u8]> = incoming.split(|&byte| byte == b'\n').collect();
    lines.pop(); // Text after the last newline is not a complete line.
    lines.contains(&marker)
}
pub(super) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
/// Quoted mktemp template in the destination's folder and the quoted destination.
fn paths(destination: &str, template: &str) -> Result<(String, String), String> {
    let (parent, name) = destination.rsplit_once('/').unwrap_or((".", destination));
    if name.is_empty() || matches!(name, "." | "..") || destination.contains('\0') {
        return Err("Choose a remote filename for SCP upload.".into());
    }
    let parent = if parent.is_empty() { "/" } else { parent };
    let parent = if parent.starts_with('/') || parent.starts_with("./") {
        parent.to_owned()
    } else {
        format!("./{parent}")
    };
    let destination = if destination.starts_with('/') || destination.starts_with("./") {
        destination.to_owned()
    } else {
        format!("./{destination}")
    };
    Ok((quote(&format!("{parent}/{template}")), quote(&destination)))
}
fn command(destination: &str, marker: &str) -> Result<String, String> {
    let (template, destination) = paths(destination, ".ssh4-scp-XXXXXXXXXX")?;
    Ok(format!(
        r#"set -eu
umask 077
stage=$(mktemp -d {template})
trap 'rm -f "$stage/payload"; rmdir "$stage"' EXIT
trap 'exit 1' HUP INT TERM
scp -t "$stage/payload"
ln -T -- "$stage/payload" {destination}
printf '%s\n' '{marker}'
"#
    ))
}
/// Edit sessions only: receive into a private sibling folder, then replace the
/// destination only while it still matches `expected` (see `edit::replace_tail`).
fn replace_command(
    destination: &str,
    expected: RemoteFingerprint,
    marker: &str,
    conflict: &str,
) -> Result<String, String> {
    let (template, _) = paths(destination, ".ssh4-edit-XXXXXXXXXX")?;
    Ok(format!(
        r#"set -eu
umask 077
stage=$(mktemp -d {template})
trap 'rm -f "$stage/payload"; rmdir "$stage"' EXIT
trap 'exit 1' HUP INT TERM
scp -t "$stage/payload"
{tail}"#,
        tail = edit::replace_tail(
            "\"$stage/payload\"",
            destination,
            expected,
            marker,
            conflict
        )
    ))
}
// Consume one SCP status, retaining incomplete fragmented errors.
pub(super) fn acknowledgement(bytes: &mut Vec<u8>) -> Result<bool, String> {
    match bytes.first().copied() {
        None => Ok(false),
        Some(0) => {
            bytes.remove(0);
            Ok(true)
        }
        Some(1 | 2) => {
            if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
                let detail = String::from_utf8_lossy(&bytes[1..end])
                    .chars()
                    .flat_map(char::escape_debug)
                    .collect::<String>();
                Err(format!("SCP server rejected the transfer: {detail}"))
            } else if bytes.len() > 4096 {
                Err("SCP error response exceeds 4 KiB.".into())
            } else {
                Ok(false)
            }
        }
        Some(_) => Err("Invalid SCP acknowledgement. This host may not support legacy SCP.".into()),
    }
}
impl<S: Read + Write> Job<S> {
    pub(super) fn new(
        session: &mut ssh::LocalSession<S>,
        local: &Path,
        remote: String,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let upload = crate::upload::Upload::new(local, remote)?;
        let marker = format!("SSH4_SCP_COMPLETE_{:032x}", rand::random::<u128>());
        let command = command(&upload.destination, &marker)?;
        Self::start(session, upload, command, &marker, None, progress, cancel)
    }
    /// Edit sessions only: upload `local`, then replace `remote` if unchanged.
    pub(super) fn replace(
        session: &mut ssh::LocalSession<S>,
        local: &Path,
        remote: String,
        expected: RemoteFingerprint,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let upload = crate::upload::Upload::for_replace(local, remote)?;
        let (marker, conflict) = edit::markers();
        let command = replace_command(&upload.destination, expected, &marker, &conflict)?;
        let replace = Some((marker.clone(), conflict));
        Self::start(session, upload, command, &marker, replace, progress, cancel)
    }
    fn start(
        session: &mut ssh::LocalSession<S>,
        upload: crate::upload::Upload,
        command: String,
        marker: &str,
        replace: Option<(String, String)>,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let mut channel = session.open_exec().map_err(|e| e.to_string())?;
        channel.exec_command(&command).map_err(|e| e.to_string())?;
        progress.total.store(upload.total, Ordering::Relaxed);
        Ok(Self {
            channel,
            upload,
            stage: Stage::Greeting,
            outgoing: Vec::new(),
            offset: 0,
            incoming: Vec::new(),
            written: 0,
            marker: format!("{marker}\n").into_bytes(),
            replace,
            progress,
            cancel,
            deadline: Instant::now() + Duration::from_secs(30),
        })
    }
    fn queue(&mut self, bytes: Vec<u8>) {
        self.outgoing = bytes;
        self.offset = 0;
    }
    pub(super) fn poll(&mut self) -> Result<bool, String> {
        if self.cancel.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            if self.stage == Stage::Closing {
                // All data was sent; the script may already have published it.
                self.progress.uncertain.store(true, Ordering::Release);
            }
            return Err("SCP upload stopped. A remote .ssh4-scp-* temporary folder or completed destination may remain; verify before retrying.".into());
        }
        if self.offset < self.outgoing.len() {
            let count = self
                .channel
                .try_write(&self.outgoing[self.offset..])
                .map_err(|e| e.to_string())?;
            self.offset += count;
            if count > 0 {
                self.deadline = Instant::now() + Duration::from_secs(30);
            }
            if self.stage == Stage::Data {
                self.written += count as u64;
                self.progress.bytes.store(self.written, Ordering::Relaxed);
            }
        }
        if let Some(bytes) = self.channel.try_read().map_err(|e| e.to_string())? {
            if !bytes.is_empty() {
                self.deadline = Instant::now() + Duration::from_secs(30);
            }
            if self.incoming.len() + bytes.len() > 65536 {
                return Err("SCP response exceeds 64 KiB.".into());
            }
            self.incoming.extend(bytes);
        }
        if self.stage == Stage::Closing {
            if self.channel.remote_closed() {
                if let Some((marker, conflict)) = &self.replace {
                    let status = self.channel.received_exit_status();
                    let outcome = edit::outcome(&self.incoming, status, marker, conflict)?;
                    return edit::finish(outcome, &self.progress).map(|()| true);
                }
                if published(&self.incoming, &self.marker) {
                    return Ok(true);
                }
                return Err("SCP publication failed. The destination must not exist, and the host must support mktemp, scp and ln -T.".into());
            }
            return Ok(false);
        }
        if self.channel.remote_closed() {
            return Err("SCP channel closed before upload completion.".into());
        }
        if self.offset != self.outgoing.len() {
            return Ok(false);
        }
        match self.stage {
            Stage::Greeting if acknowledgement(&mut self.incoming)? => {
                self.queue(format!("C0600 {} payload\n", self.upload.total).into_bytes());
                self.stage = Stage::Header;
            }
            Stage::Header if acknowledgement(&mut self.incoming)? => {
                self.stage = Stage::Data;
            }
            Stage::Data => {
                if !self.incoming.is_empty() {
                    return Err("Unexpected SCP response during file data.".into());
                }
                let bytes = self.upload.read_chunk(self.written)?;
                if bytes.is_empty() {
                    self.queue(vec![0]);
                    self.stage = Stage::FinalAck;
                } else {
                    self.queue(bytes);
                }
            }
            Stage::FinalAck if acknowledgement(&mut self.incoming)? => {
                self.channel.send_eof().map_err(|e| e.to_string())?;
                self.stage = Stage::Closing;
            }
            _ => {}
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
    fn real_scp_receiver_stages_publishes_and_refuses_existing_destination() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        if !Path::new(shell).is_file() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let run = || {
            let mut child = std::process::Command::new(shell)
                .args(["-c", &command("result.bin", "DONE").unwrap()])
                .current_dir(directory.path())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"C0600 4 payload\nA\0B\xff\0")
                .unwrap();
            child.wait_with_output().unwrap()
        };
        let result = run();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"\0\0\0DONE\n");
        assert_eq!(
            std::fs::read(directory.path().join("result.bin")).unwrap(),
            b"A\0B\xff"
        );
        std::fs::write(directory.path().join("result.bin"), b"original").unwrap();
        assert!(!run().status.success());
        assert_eq!(
            std::fs::read(directory.path().join("result.bin")).unwrap(),
            b"original"
        );
        std::fs::remove_file(directory.path().join("result.bin")).unwrap();
        std::fs::create_dir(directory.path().join("result.bin")).unwrap();
        assert!(!run().status.success());
        assert_eq!(
            std::fs::read_dir(directory.path().join("result.bin"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn real_scp_replacement_requires_the_recorded_fingerprint() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        if !Path::new(shell).is_file() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("edited.txt");
        std::fs::write(&destination, b"original").unwrap();
        let metadata = std::fs::metadata(&destination).unwrap();
        let expected = RemoteFingerprint {
            size: metadata.len(),
            modified: metadata
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        };
        let run = |expected: RemoteFingerprint| {
            let mut child = std::process::Command::new(shell)
                .args([
                    "-c",
                    &replace_command("edited.txt", expected, "DONE", "CLASH").unwrap(),
                ])
                .current_dir(directory.path())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"C0600 4 payload\nA\0B\xff\0")
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.stdout.starts_with(b"\0\0\0"), "{output:?}");
            let status = output.status.code().map(|code| code as u32);
            edit::outcome(&output.stdout[3..], status, "DONE", "CLASH")
        };
        let stale = RemoteFingerprint {
            modified: expected.modified - 1,
            ..expected
        };
        assert_eq!(run(stale), Ok(edit::Outcome::Conflict));
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        let Ok(edit::Outcome::Replaced(new)) = run(expected) else {
            panic!("matching fingerprint was not replaced")
        };
        assert_eq!(std::fs::read(&destination).unwrap(), b"A\0B\xff");
        assert_eq!(new.size, 4);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        // The previous fingerprint is now stale.
        assert_eq!(run(expected), Ok(edit::Outcome::Conflict));
        assert_eq!(std::fs::read(&destination).unwrap(), b"A\0B\xff");
    }
    #[test]
    fn fragmented_ack_errors_are_bounded_and_escaped() {
        let mut bytes = vec![];
        assert!(!acknowledgement(&mut bytes).unwrap());
        bytes.extend(b"\0\0");
        assert!(acknowledgement(&mut bytes).unwrap());
        assert_eq!(bytes, [0]);
        let mut error = b"\x01denied\x1b[".to_vec();
        assert!(!acknowledgement(&mut error).unwrap());
        error.push(b'\n');
        assert!(!acknowledgement(&mut error).unwrap_err().contains('\x1b'));
        assert!(acknowledgement(&mut [vec![2], vec![b'x'; 4097]].concat()).is_err());
    }
    #[test]
    fn completion_marker_counts_only_as_a_whole_line() {
        let marker = b"SSH4_SCP_COMPLETE_00ff\n";
        for incoming in [
            &marker[..],
            b"SSH4_SCP_COMPLETE_00ff\nlogout script\n",
            b"motd\nSSH4_SCP_COMPLETE_00ff\n",
        ] {
            assert!(published(incoming, marker), "{incoming:?}");
        }
        for incoming in [
            &b""[..],
            b"SSH4_SCP_COMPLETE_00ff",
            b"xSSH4_SCP_COMPLETE_00ff\n",
            b"SSH4_SCP_COMPLETE_00ffx\n",
        ] {
            assert!(!published(incoming, marker), "{incoming:?}");
        }
    }
    #[test]
    fn command_quotes_literal_paths_and_never_uses_remote_name_as_protocol_header() {
        let text = command("/tmp/a'b;$(literal)\nname", "DONE").unwrap();
        assert!(text.contains("'/tmp/a'\\''b;$(literal)\nname'"));
        assert!(text.contains("ln -T --"));
        assert!(command("/tmp/", "DONE").is_err());
        assert!(command("-option", "DONE").unwrap().contains("'./-option'"));
    }
}
