//! Edit-session replacement over a POSIX shell.
//!
//! This is the single, reviewed exception to fresh's no-overwrite rule: an
//! "Edit" session may replace the remote file it downloaded, and only while the
//! server copy still has the size and modification time recorded at download
//! (or at the previous sync). Ordinary uploads keep refusing to overwrite.
//!
//! Used when the server lacks the SFTP `posix-rename@openssh.com` extension
//! (the staging file was already written over SFTP) and by the SCP backend.
use super::{Progress, RemoteFingerprint};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub(super) const CONFLICT: &str =
    "The remote file changed on the server since it was downloaded or last synced, so it was not replaced.";

/// One literal shell word; relative paths gain `./` so they cannot become options.
pub(super) fn remote_word(value: &str) -> String {
    super::scp::quote(&if value.starts_with('/') {
        value.to_owned()
    } else {
        format!("./{value}")
    })
}

/// Shell commands that replace `destination` with `payload` (a shell word that
/// names the uploaded copy in the destination's folder) only while the
/// destination is still the regular file described by `expected`.
///
/// Normally the copy receives the original owner, group and permission bits
/// (setuid/setgid dropped, as an in-place write by an unprivileged user would)
/// before `mv -f -T` renames it over the original, so readers see either the
/// old or the new file. When a rename would change the owner or group (they
/// cannot be given to the copy) or split hard links, the contents are written
/// into the original file instead, which keeps its owner, group, ACLs and
/// links. Prints `size mtime mode uid gid links` of the result, then `marker`.
pub(super) fn replace_tail(
    payload: &str,
    destination: &str,
    expected: RemoteFingerprint,
    marker: &str,
    conflict: &str,
) -> String {
    format!(
        r#"dest={destination}
if stat -c %s . >/dev/null 2>&1; then
    meta() {{ stat -c '%s %Y %a %u %g %h' "$1"; }}
else
    meta() {{ stat -f '%z %m %Lp %u %g %l' "$1"; }}
fi
current=missing
if test -f "$dest" && test ! -L "$dest"; then current=$(meta "$dest"); fi
set -- $current
if test "$#" -ne 6 || test "$1 $2" != '{size} {modified}'; then
    printf '%s\n' '{conflict}'
    exit 3
fi
case $3 in ''|*[!0-7]*) exit 1;; esac
mode=$(printf '%o' "$((0$3 & 511))")
owner=$4:$5
links=$6
set -- $(meta {payload})
if test "$links" = 1 && {{ test "$4:$5" = "$owner" || chown "$owner" {payload} 2>/dev/null; }}; then
    chmod "$mode" {payload}
    new=$(meta {payload})
    mv -f -T -- {payload} "$dest"
else
    cat {payload} > "$dest"
    new=$(meta "$dest")
fi
printf '%s\n%s\n' "$new" '{marker}'
"#,
        destination = remote_word(destination),
        size = expected.size,
        modified = expected.modified,
    )
}

#[derive(Debug, PartialEq)]
pub(super) enum Outcome {
    Replaced(RemoteFingerprint),
    Conflict,
}

/// Interpret the output of a script ending in [`replace_tail`]. Only the
/// random marker lines count: a login script that prints a banner (some
/// hosts' `.bashrc` does, even for commands) must not turn a completed
/// replacement into a failure, or a conflict into a retryable error.
pub(super) fn outcome(
    output: &[u8],
    status: Option<u32>,
    marker: &str,
    conflict: &str,
) -> Result<Outcome, String> {
    let text = String::from_utf8_lossy(output);
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.contains(&conflict) {
        return Ok(Outcome::Conflict);
    }
    let failed = || {
        "Remote replacement failed. The host needs a POSIX shell, stat and GNU mv (-T), and the file or its folder must be writable.".to_owned()
    };
    if status != Some(0) {
        return Err(failed());
    }
    // The script's last two lines are the fingerprint and the marker, each
    // ended by a newline; anything a login script printed comes before them.
    let [.., fields, last, ""] = lines[..] else {
        return Err(failed());
    };
    if last != marker {
        return Err(failed());
    }
    let fields: Vec<&str> = fields.split(' ').collect();
    match fields[..] {
        [size, modified, _mode, _uid, _gid, _links] => Ok(Outcome::Replaced(RemoteFingerprint {
            size: size.parse().map_err(|_| failed())?,
            modified: modified.parse().map_err(|_| failed())?,
        })),
        _ => Err(failed()),
    }
}

fn rename_command(
    staging: &str,
    destination: &str,
    expected: RemoteFingerprint,
    marker: &str,
    conflict: &str,
) -> String {
    format!(
        "set -eu\numask 077\nstage={staging}\ntrap 'rm -f -- \"$stage\"' EXIT\ntrap 'exit 1' HUP INT TERM\n{tail}",
        staging = remote_word(staging),
        tail = replace_tail("\"$stage\"", destination, expected, marker, conflict),
    )
}

pub(super) fn markers() -> (String, String) {
    let random = rand::random::<u128>();
    (
        format!("SSH4_EDIT_COMPLETE_{random:032x}"),
        format!("SSH4_EDIT_CONFLICT_{random:032x}"),
    )
}

/// Record a script outcome in the task's progress; conflicts become errors.
pub(super) fn finish(outcome: Outcome, progress: &Progress) -> Result<(), String> {
    match outcome {
        Outcome::Replaced(fingerprint) => {
            *progress
                .fingerprint
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(fingerprint);
            Ok(())
        }
        Outcome::Conflict => {
            progress.conflict.store(true, Ordering::Release);
            Err(CONFLICT.into())
        }
    }
}

/// Publishes an SFTP-written staging file with `mv -f -T` when the server
/// does not offer `posix-rename@openssh.com`. The script removes the staging
/// file on every failure path, including a detected conflict.
pub(super) struct RenameJob<S: Read + Write> {
    channel: ssh::LocalExec<S>,
    bytes: Vec<u8>,
    marker: String,
    conflict: String,
    staging: String,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}
impl<S: Read + Write> RenameJob<S> {
    pub(super) fn new(
        session: &mut ssh::LocalSession<S>,
        staging: String,
        destination: &str,
        expected: RemoteFingerprint,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let (marker, conflict) = markers();
        let script = rename_command(&staging, destination, expected, &marker, &conflict);
        let mut channel = session.open_exec().map_err(|e| e.to_string())?;
        channel.exec_command(&script).map_err(|e| e.to_string())?;
        Ok(Self {
            channel,
            bytes: Vec::new(),
            marker,
            conflict,
            staging,
            progress,
            cancel,
            deadline: Instant::now() + Duration::from_secs(30),
        })
    }
    pub(super) fn poll(&mut self) -> Result<bool, String> {
        if self.cancel.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            self.progress.uncertain.store(true, Ordering::Release);
            return Err(format!("Edit upload stopped while replacing the remote file. It may already have been replaced, and temporary file {:?} may remain; refresh before retrying.", self.staging));
        }
        if let Some(bytes) = self.channel.try_read().map_err(|e| e.to_string())? {
            if self.bytes.len() + bytes.len() > 65536 {
                return Err("Shell replacement output exceeds 64 KiB.".into());
            }
            self.bytes.extend(bytes);
        }
        if !self.channel.remote_closed() {
            return Ok(false);
        }
        let status = self.channel.received_exit_status();
        // The script removes the staging file on its own failures; it remains
        // only if the shell never ran the script (e.g. an SFTP-only account).
        let outcome =
            outcome(&self.bytes, status, &self.marker, &self.conflict).map_err(|error| {
                format!(
                    "{error} If the shell could not run, temporary file {:?} may remain.",
                    self.staging
                )
            })?;
        finish(outcome, &self.progress).map(|()| true)
    }
}
impl<S: Read + Write> Drop for RenameJob<S> {
    fn drop(&mut self) {
        self.channel.force_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fingerprint(path: &Path) -> RemoteFingerprint {
        let metadata = std::fs::metadata(path).unwrap();
        RemoteFingerprint {
            size: metadata.len(),
            modified: metadata
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        }
    }
    fn shell() -> Option<&'static str> {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        Path::new(shell).is_file().then_some(shell)
    }

    #[test]
    fn script_quotes_literal_paths_and_reports_typed_outcomes() {
        let expected = RemoteFingerprint {
            size: 7,
            modified: 1_700_000_000,
        };
        let text = rename_command("/t/.ssh4-edit-1.part", "/t/a'b;$(x)\nc", expected, "M", "C");
        assert!(text.contains("dest='/t/a'\\''b;$(x)\nc'"));
        assert!(text.contains("stage='/t/.ssh4-edit-1.part'"));
        assert!(text.contains("'7 1700000000'"));
        assert!(text.contains("mv -f -T -- \"$stage\" \"$dest\""));
        assert!(text.contains("cat \"$stage\" > \"$dest\""));
        assert_eq!(remote_word("-x"), "'./-x'");
        assert_eq!(outcome(b"C\n", Some(3), "M", "C"), Ok(Outcome::Conflict));
        let replaced = Ok(Outcome::Replaced(RemoteFingerprint {
            size: 12,
            modified: 1_700_000_001,
        }));
        assert_eq!(
            outcome(b"12 1700000001 644 1000 1000 1\nM\n", Some(0), "M", "C"),
            replaced
        );
        // Login banners printed before the script's own output are ignored.
        let banner = b"Welcome to host\n\x1b[0m\n12 1700000001 644 0 4 1\nM\n";
        assert_eq!(outcome(banner, Some(0), "M", "C"), replaced);
        assert_eq!(
            outcome(b"motd\nC\n", Some(3), "M", "C"),
            Ok(Outcome::Conflict)
        );
        for (bytes, status) in [
            (&b"12 1700000001 644 0 0 1\nM\n"[..], Some(1)),
            (b"12 1700000001 644 0 0 1\nM\n", None),
            (b"12 1700000001 644\nM\n", Some(0)),
            (b"x 1700000001 644 0 0 1\nM\n", Some(0)),
            (b"12 -1 644 0 0 1\nM\n", Some(0)),
            (b"12 1700000001 644 0 0 1\nN\n", Some(0)),
            (b"12 1700000001 644 0 0 1\nM\ntrailing\n", Some(0)),
            (b"12 1700000001 644 0 0 1\nM", Some(0)),
            (b"M\n", Some(0)),
            (b"Cx\n", Some(3)),
        ] {
            assert!(outcome(bytes, status, "M", "C").is_err(), "{bytes:?}");
        }
        let progress = Progress::default();
        assert_eq!(finish(Outcome::Conflict, &progress), Err(CONFLICT.into()));
        assert!(progress.conflict.load(Ordering::Acquire));
    }

    #[test]
    fn real_shell_replaces_only_an_unchanged_regular_file() {
        let Some(shell) = shell() else { return };
        let directory = tempfile::tempdir().unwrap();
        let name = "edit '$(touch sentinel).txt";
        let destination = directory.path().join(name);
        std::fs::write(&destination, b"original").unwrap();
        let expected = fingerprint(&destination);
        let run = |staging: &str, expected: RemoteFingerprint| {
            std::fs::write(directory.path().join(staging), b"edited bytes\0\xff").unwrap();
            let output = std::process::Command::new(shell)
                .args([
                    "-c",
                    &rename_command(staging, name, expected, "DONE", "CLASH"),
                ])
                .current_dir(directory.path())
                .output()
                .unwrap();
            (output.stdout, output.status.code().map(|code| code as u32))
        };
        // A stale fingerprint is a conflict: the server copy survives and the
        // staging file is removed.
        let stale = RemoteFingerprint {
            size: expected.size + 1,
            ..expected
        };
        let (stdout, status) = run(".ssh4-edit-a.part", stale);
        assert_eq!(
            outcome(&stdout, status, "DONE", "CLASH"),
            Ok(Outcome::Conflict)
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        assert!(!directory.path().join(".ssh4-edit-a.part").exists());
        // The matching fingerprint replaces the file atomically.
        let (stdout, status) = run(".ssh4-edit-b.part", expected);
        let Ok(Outcome::Replaced(new)) = outcome(&stdout, status, "DONE", "CLASH") else {
            panic!("{}", String::from_utf8_lossy(&stdout))
        };
        assert_eq!(std::fs::read(&destination).unwrap(), b"edited bytes\0\xff");
        assert_eq!(new, fingerprint(&destination));
        assert!(!directory.path().join(".ssh4-edit-b.part").exists());
        // A deleted destination is also a conflict and is not recreated.
        std::fs::remove_file(&destination).unwrap();
        let (stdout, status) = run(".ssh4-edit-c.part", new);
        assert_eq!(
            outcome(&stdout, status, "DONE", "CLASH"),
            Ok(Outcome::Conflict)
        );
        assert!(!destination.exists());
        // A directory in its place is never replaced.
        std::fs::create_dir(&destination).unwrap();
        let (stdout, status) = run(".ssh4-edit-d.part", new);
        assert_eq!(
            outcome(&stdout, status, "DONE", "CLASH"),
            Ok(Outcome::Conflict)
        );
        assert!(destination.is_dir());
        assert!(!directory.path().join("sentinel").exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn real_shell_writes_hard_linked_files_in_place() {
        let Some(shell) = shell() else { return };
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("shared.conf");
        let other = directory.path().join("other-name.conf");
        std::fs::write(&destination, b"original").unwrap();
        if std::fs::hard_link(&destination, &other).is_err() {
            return; // File system without hard links.
        }
        let expected = fingerprint(&destination);
        std::fs::write(directory.path().join("stage"), b"edited").unwrap();
        let output = std::process::Command::new(shell)
            .args([
                "-c",
                &rename_command("stage", "shared.conf", expected, "DONE", "CLASH"),
            ])
            .current_dir(directory.path())
            .output()
            .unwrap();
        let status = output.status.code().map(|code| code as u32);
        let Ok(Outcome::Replaced(new)) = outcome(&output.stdout, status, "DONE", "CLASH") else {
            panic!("{}", String::from_utf8_lossy(&output.stdout))
        };
        // Both names still share the edited contents; nothing else remains.
        assert_eq!(std::fs::read(&destination).unwrap(), b"edited");
        assert_eq!(std::fs::read(&other).unwrap(), b"edited");
        assert_eq!(new, fingerprint(&destination));
        assert!(!directory.path().join("stage").exists());
    }

    #[cfg(unix)]
    #[test]
    fn real_shell_preserves_permission_bits_without_setuid() {
        use std::os::unix::fs::PermissionsExt;
        let Some(shell) = shell() else { return };
        if std::process::Command::new(shell)
            .args([
                "-c",
                "mv -T --help >/dev/null 2>&1 && stat -c %s . >/dev/null",
            ])
            .status()
            .map_or(true, |status| !status.success())
        {
            return; // GNU mv/stat are a documented requirement of this backend.
        }
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("script.sh");
        std::fs::write(&destination, b"old").unwrap();
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o4751)).unwrap();
        let expected = fingerprint(&destination);
        std::fs::write(directory.path().join("stage"), b"new").unwrap();
        let output = std::process::Command::new(shell)
            .args([
                "-c",
                &rename_command("stage", "script.sh", expected, "DONE", "CLASH"),
            ])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o751);
    }
}
