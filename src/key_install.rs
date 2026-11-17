//! Reviewed public-key installation over a separate exec channel.
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

pub struct Key {
    line: String,
    pub fingerprint: String,
}
impl Key {
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut text = String::new();
        file.take(16385)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        Self::parse(&text)
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 16384
            || text.trim().lines().count() != 1
            || text.trim().chars().any(char::is_control)
        {
            return Err("Choose a single OpenSSH public key (maximum 16 KiB).".into());
        }
        let key = ssh_key::PublicKey::from_openssh(text.trim())
            .map_err(|_| "Invalid OpenSSH public key. Choose the .pub file, not a private key.")?;
        let encoded = key.to_openssh().map_err(|e| e.to_string())?;
        // Comments never enter the shell command or the remote authorization file.
        let line = encoded
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        if !line
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" @._-+/=".contains(&byte))
        {
            return Err("Unsupported public-key encoding.".into());
        }
        Ok(Self {
            line,
            fingerprint: key.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
        })
    }
    fn command(&self, marker: &str) -> String {
        format!(
            r#"set -eu
umask 077
test -n "$HOME"
test ! -L "$HOME/.ssh"
mkdir -p "$HOME/.ssh"
chmod 700 "$HOME/.ssh"
cd "$HOME/.ssh"
mkdir .ssh4-key-install-lock
tmp=
trap 'test -z "$tmp" || rm -f "$tmp"; rmdir .ssh4-key-install-lock' EXIT
trap 'exit 1' HUP INT TERM
test ! -L authorized_keys
test ! -e authorized_keys || test -f authorized_keys
tmp=$(mktemp .ssh4-authorized-XXXXXXXXXX)
if test -f authorized_keys; then cat authorized_keys > "$tmp"; fi
if ! awk -v wanted='{line}' '/^[ \t]*#/ {{ next }} {{ for (i = 1; i < NF; i++) if ($i " " $(i + 1) == wanted) found = 1 }} END {{ exit !found }}' "$tmp"; then
    printf '\n%s\n' '{line}' >> "$tmp"
fi
chmod 600 "$tmp"
mv -f "$tmp" authorized_keys
tmp=
printf '%s\n' '{marker}'
"#,
            line = self.line
        )
    }
}
pub struct Task {
    result: mpsc::Receiver<Result<(), String>>,
    cancel: Arc<AtomicBool>,
}
impl Task {
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.result.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Installation ended with the session; verify the remote key before retrying."
                    .into(),
            )),
        }
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
pub(crate) struct Request {
    key: Key,
    answer: mpsc::SyncSender<Result<(), String>>,
    cancel: Arc<AtomicBool>,
}
pub(crate) fn submit(sender: &mpsc::SyncSender<Request>, key: Key) -> Result<Task, String> {
    let (answer, result) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    sender
        .try_send(Request {
            key,
            answer,
            cancel: cancel.clone(),
        })
        .map_err(|_| "A key installation is already queued or the session is closed.")?;
    Ok(Task { result, cancel })
}
struct Active<S: Read + Write> {
    channel: ssh::LocalExec<S>,
    request: Request,
    marker: Vec<u8>,
    output: Vec<u8>,
    started: Instant,
}
pub(crate) struct Worker<S: Read + Write> {
    requests: mpsc::Receiver<Request>,
    active: Option<Active<S>>,
}
impl<S: Read + Write> Worker<S> {
    pub(crate) fn new(requests: mpsc::Receiver<Request>) -> Self {
        Self {
            requests,
            active: None,
        }
    }
    pub(crate) fn poll(&mut self, session: &mut ssh::LocalSession<S>) {
        if self.active.is_none() {
            if let Ok(request) = self.requests.try_recv() {
                if request.cancel.load(Ordering::Acquire) {
                    return;
                }
                let marker = format!("SSH4_KEY_INSTALLED_{:032x}", rand::random::<u128>());
                match session.open_exec().and_then(|mut channel| {
                    channel.exec_command(&request.key.command(&marker))?;
                    Ok(channel)
                }) {
                    Ok(channel) => {
                        self.active = Some(Active {
                            channel,
                            request,
                            marker: format!("{marker}\n").into_bytes(),
                            output: Vec::new(),
                            started: Instant::now(),
                        })
                    }
                    Err(error) => {
                        let _ = request.answer.send(Err(error.to_string()));
                    }
                }
            }
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let result = if active.request.cancel.load(Ordering::Acquire)
            || active.started.elapsed() > Duration::from_secs(30)
        {
            Some(Err("Installation stopped; the remote key may already be installed. Verify before retrying.".into()))
        } else {
            match active.channel.try_read() {
                Err(error) => Some(Err(error.to_string())),
                Ok(bytes) => {
                    if let Some(bytes) = bytes {
                        active.output.extend(bytes);
                    }
                    if active.output.len() > 65536 {
                        Some(Err("Installation produced too much output.".into()))
                    } else if active.channel.remote_closed() {
                        Some(
                            if active.output.ends_with(&active.marker)
                                && active.channel.received_exit_status() == Some(0)
                            {
                                Ok(())
                            } else {
                                Err("Remote key installation failed. The host must support POSIX shell tools and allow writes to ~/.ssh/authorized_keys.".into())
                            },
                        )
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(result) = result {
            let mut active = self.active.take().unwrap();
            active.channel.force_close();
            let _ = active.request.answer.send(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_install_preserves_existing_keys_is_idempotent_and_honors_lock() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        if !Path::new(shell).is_file() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join(".ssh");
        std::fs::create_dir(&folder).unwrap();
        let target = folder.join("authorized_keys");
        let original = b"# preserve this comment\nssh-ed25519 existing-key comment-without-newline";
        std::fs::write(&target, original).unwrap();
        let private =
            ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519)
                .unwrap();
        let key = Key::parse(&private.public_key().to_openssh().unwrap()).unwrap();
        let run = || {
            std::process::Command::new(shell)
                .arg("-c")
                .arg(key.command("DONE"))
                .env("HOME", home.path().to_string_lossy().replace('\\', "/"))
                .output()
                .unwrap()
        };
        let result = run();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"DONE\n");
        let installed = std::fs::read(&target).unwrap();
        assert!(installed.starts_with(original));
        assert!(installed.ends_with(format!("\n{}\n", key.line).as_bytes()));
        assert!(run().status.success());
        assert_eq!(std::fs::read(&target).unwrap(), installed);
        // An entry with options (even quoted ones) is the same key: adding a
        // bare copy would lift its restrictions.
        let restricted = format!(
            "command=\"/usr/bin/backup --to x\",restrict {} note\n",
            key.line
        );
        std::fs::write(&target, &restricted).unwrap();
        assert!(run().status.success());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), restricted);
        // A commented-out copy does not count as installed.
        let disabled = format!("# {}\n", key.line);
        std::fs::write(&target, &disabled).unwrap();
        assert!(run().status.success());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            format!("{disabled}\n{}\n", key.line)
        );
        std::fs::write(&target, &installed).unwrap();
        std::fs::create_dir(folder.join(".ssh4-key-install-lock")).unwrap();
        assert!(!run().status.success());
        assert_eq!(std::fs::read(&target).unwrap(), installed);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    }
    #[test]
    fn rejects_private_multiline_and_options_and_strips_shell_comments() {
        let key = ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519)
            .unwrap();
        let public = key.public_key().to_openssh().unwrap();
        let parsed = Key::parse(&format!("{public} ';$(touch evil)")).unwrap();
        assert!(!parsed.command("DONE").contains("touch evil"));
        assert!(Key::parse(&format!("{public}\n{public}")).is_err());
        assert!(Key::parse(&format!("command=\"bad\" {public}")).is_err());
        assert!(Key::parse(&key.to_openssh(ssh_key::LineEnding::LF).unwrap()).is_err());
    }
}
