//! Bounded shell fallback on the authenticated session after subsystem refusal.
use super::{Action, Entry, Listing, Response};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// A remote path as one shell word that never reads as an option.
fn path(value: &str) -> String {
    super::edit::remote_word(value)
}
fn command(directory: &str, action: Option<&Action>, marker: &str) -> Result<String, String> {
    let body = match action {
        None => format!(r#"cd -P {directory}
printf '%s\000' "$PWD"
if stat -c '%f %s %Y' . >/dev/null 2>&1; then style=g; else style=b; fi
export style
find . ! -name . -prune -exec sh -c '
for file do
    if test "$style" = g; then meta=$(stat -c "%f %s %Y" -- "$file") || exit 1
    else meta=$(stat -f "%p %z %m" "$file") || exit 1; fi
    printf "%s\000%s\000%s\000" "$style" "$meta" "${{file#./}}"
done' sh {{}} +
"#, directory = path(directory)),
        Some(Action::CreateFile { path: value }) => format!("(set -C; : > {})\n", path(value)),
        // 755 like the SFTP request; the script's umask 077 would give 700.
        Some(Action::CreateDirectory { path: value }) => format!("(umask 022 && mkdir {})\n", path(value)),
        Some(Action::SetPermissions { path: value, mode }) => format!("test ! -L {p}\nchmod {mode:04o} {p}\n", p = path(value)),
        Some(Action::RemoveFile { path: value }) => format!("test ! -d {p} || test -L {p}\nrm -f {p}\n", p = path(value)),
        Some(Action::RemoveDirectory { path: value }) => format!("rmdir {}\n", path(value)),
        Some(Action::Rename { from, to }) => format!("test ! -e {to}\ntest ! -L {to}\nmv -n -T -- {from} {to}\ntest ! -e {from}\ntest ! -L {from}\n", from = path(from), to = path(to)),
        _ => return Err("Transfer reached the shell file-action worker.".into()),
    };
    Ok(format!(
        "set -eu\numask 077\n{body}printf '%s\\000' '{marker}'\n"
    ))
}

fn listing(bytes: &[u8]) -> Result<Listing, String> {
    let mut fields = bytes.split(|b| *b == 0);
    let path = text(fields.next().ok_or("Missing shell directory path.")?)?;
    if !path.starts_with('/') {
        return Err("Shell listing returned a non-absolute directory.".into());
    }
    let mut entries = Vec::new();
    loop {
        let style = fields.next().ok_or("Truncated shell listing.")?;
        if style.is_empty() && fields.next().is_none() {
            break;
        }
        let radix = match style {
            b"g" => 16,
            b"b" => 8,
            _ => return Err("Invalid shell stat format.".into()),
        };
        let metadata = text(fields.next().ok_or("Missing shell file metadata.")?)?;
        let values: Vec<_> = metadata.split_whitespace().collect();
        if values.len() != 3 {
            return Err("Invalid shell file metadata.".into());
        }
        let permissions =
            u32::from_str_radix(values[0], radix).map_err(|_| "Invalid shell file mode.")?;
        let size = values[1]
            .parse::<u64>()
            .map_err(|_| "Invalid shell file size.")?;
        let modified = values[2]
            .parse::<i64>()
            .map_err(|_| "Invalid shell file time.")?;
        let name = text(fields.next().ok_or("Missing shell filename.")?)?;
        if name.is_empty() || matches!(name.as_str(), "." | "..") || name.contains('/') {
            return Err("Invalid shell filename.".into());
        }
        if entries.len() == 10000 {
            return Err("Shell directory exceeds 10000 entries.".into());
        }
        entries.push(Entry {
            name,
            size: Some(size),
            permissions: Some(permissions),
            modified: u32::try_from(modified).ok(),
        });
    }
    super::sort_entries(&mut entries);
    Ok(Listing { path, entries })
}
fn text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 4096 {
        return Err("Shell listing field exceeds 4096 bytes.".into());
    }
    String::from_utf8(bytes.into()).map_err(|_| "Shell listing contains a non-UTF-8 path.".into())
}

pub(super) struct Job<S: Read + Write> {
    channel: ssh::LocalExec<S>,
    bytes: Vec<u8>,
    marker: Vec<u8>,
    directory: bool,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}
impl<S: Read + Write> Job<S> {
    pub(super) fn new(
        session: &mut ssh::LocalSession<S>,
        directory: &str,
        action: Option<&Action>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let marker = format!("SSH4_FILES_COMPLETE_{:032x}", rand::random::<u128>());
        let script = command(directory, action, &marker)?;
        let mut channel = session.open_exec().map_err(|e| e.to_string())?;
        channel.exec_command(&script).map_err(|e| e.to_string())?;
        Ok(Self {
            channel,
            bytes: Vec::new(),
            marker: format!("{marker}\0").into_bytes(),
            directory: action.is_none(),
            cancel,
            deadline: Instant::now() + Duration::from_secs(30),
        })
    }
    pub(super) fn poll(&mut self) -> Result<Option<Response>, String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("Shell file request cancelled. A submitted remote change may already have completed; refresh before retrying.".into());
        }
        if Instant::now() >= self.deadline {
            return Err("Shell file request timed out. A submitted remote change may already have completed; refresh before retrying.".into());
        }
        if let Some(bytes) = self.channel.try_read().map_err(|e| e.to_string())? {
            if self.bytes.len() + bytes.len() > 8 * 1024 * 1024 {
                return Err("Shell file response exceeds 8 MiB.".into());
            }
            self.bytes.extend(bytes);
        }
        if !self.channel.remote_closed() {
            return Ok(None);
        }
        if self.channel.received_exit_status() != Some(0) || !self.bytes.ends_with(&self.marker) {
            return Err("Shell file operation failed. Check the path and permissions; the host needs POSIX tools and stat (GNU mv is required for rename).".into());
        }
        let payload = &self.bytes[..self.bytes.len() - self.marker.len()];
        if self.directory {
            listing(payload).map(Response::Directory).map(Some)
        } else if payload.is_empty() {
            Ok(Some(Response::Done))
        } else {
            Err("Unexpected output from the shell file operation.".into())
        }
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
    fn listing_preserves_whitespace_names_and_rejects_malformed_records() {
        let result = listing(b"/tmp\0g\081a4 3 123\0a\nb 'x\0").unwrap();
        assert_eq!(result.entries[0].name, "a\nb 'x");
        assert_eq!(result.entries[0].permissions, Some(0o100644));
        assert!(listing(b"/tmp\0g\081a4 3 123\0../bad\0").is_err());
        assert!(listing(b"/tmp\0g\081a4 3 123\0unterminated").is_err());
        assert!(listing(b"/tmp\0g\0wrong\0file\0").is_err());
    }
    #[test]
    fn real_shell_actions_preserve_existing_targets_and_literal_names() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        let dir = tempfile::tempdir().unwrap();
        let run = |action: Option<&Action>| {
            std::process::Command::new(shell)
                .args(["-c", &command(".", action, "DONE").unwrap()])
                .current_dir(dir.path())
                .output()
                .unwrap()
        };
        let name = "a '$(touch sentinel).txt";
        let create = Action::CreateFile { path: name.into() };
        assert!(run(Some(&create)).status.success());
        std::fs::write(dir.path().join(name), b"original").unwrap();
        assert!(!run(Some(&create)).status.success());
        assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), b"original");
        let result = run(None);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.ends_with(b"DONE\0"));
        let files = listing(&result.stdout[..result.stdout.len() - 5]).unwrap();
        assert_eq!(files.entries.len(), 1);
        assert_eq!(files.entries[0].name, name);
        let rename = Action::Rename {
            from: name.into(),
            to: "moved".into(),
        };
        std::fs::write(dir.path().join("moved"), b"other").unwrap();
        assert!(!run(Some(&rename)).status.success());
        assert_eq!(std::fs::read(dir.path().join("moved")).unwrap(), b"other");
        assert!(run(Some(&Action::RemoveFile {
            path: "moved".into()
        }))
        .status
        .success());
        assert!(run(Some(&rename)).status.success());
        assert!(run(Some(&Action::SetPermissions {
            path: "moved".into(),
            mode: 0o600
        }))
        .status
        .success());
        assert!(run(Some(&Action::CreateDirectory {
            path: "folder".into()
        }))
        .status
        .success());
        std::fs::write(dir.path().join("folder/keep"), b"keep").unwrap();
        assert!(!run(Some(&Action::RemoveDirectory {
            path: "folder".into()
        }))
        .status
        .success());
        assert!(!dir.path().join("sentinel").exists());
    }
}
