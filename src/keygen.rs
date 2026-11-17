//! Generate an OpenSSH Ed25519 pair without replacing existing files.
use rand::RngCore;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
pub struct Generated {
    pub private_path: PathBuf,
    pub public_path: PathBuf,
    pub public_key: String,
    pub fingerprint: String,
    pub warning: Option<String>,
}
fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0xc000_0000 | 0x0004_0000);
    } // read/write + WRITE_DAC
    options.open(path)
}
pub fn generate(path: &Path) -> Result<Generated, String> {
    generate_with_passphrase(path, None)
}
pub fn generate_with_passphrase(path: &Path, password: Option<&str>) -> Result<Generated, String> {
    if password.is_some_and(|password| password.is_empty() || password.len() > 1024) {
        return Err("Key passphrase must contain 1–1024 bytes.".into());
    }
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("Choose an absolute private-key file path.".into());
    }
    let mut public_name = path.as_os_str().to_owned();
    public_name.push(".pub");
    let public_path = PathBuf::from(public_name);
    for target in [path, public_path.as_path()] {
        match std::fs::symlink_metadata(target) {
            Ok(_) => return Err(format!("Key file already exists: {}", target.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let parent = path.parent().ok_or("Choose a key folder.")?;
    let mut private = tempfile::Builder::new()
        .prefix(".ssh4-key-")
        .make_in(parent, private_file)
        .map_err(|e| format!("Cannot stage private key: {e}"))?;
    #[cfg(windows)]
    crate::agent::pageant::restrict_key_file(private.as_file())
        .map_err(|e| format!("Cannot protect private key: {e}"))?;
    let mut public = tempfile::Builder::new()
        .prefix(".ssh4-public-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng
        .try_fill_bytes(&mut *seed)
        .map_err(|e| e.to_string())?;
    let key = ssh_key::PrivateKey::new(
        ssh_key::private::KeypairData::Ed25519(ssh_key::private::Ed25519Keypair::from_seed(&seed)),
        "neo-ssh",
    )
    .map_err(|e| e.to_string())?;
    let public_key = key.public_key().to_openssh().map_err(|e| e.to_string())?;
    let fingerprint = key
        .public_key()
        .fingerprint(ssh_key::HashAlg::Sha256)
        .to_string();
    let key = if let Some(password) = password {
        key.encrypt(&mut rand::rngs::OsRng, password)
            .map_err(|e| e.to_string())?
    } else {
        key
    };
    let encoded = key
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| e.to_string())?;
    private
        .write_all(encoded.as_bytes())
        .map_err(|e| e.to_string())?;
    writeln!(public, "{public_key}").map_err(|e| e.to_string())?;
    private.as_file().sync_all().map_err(|e| e.to_string())?;
    public.as_file().sync_all().map_err(|e| e.to_string())?;
    private
        .persist_noclobber(path)
        .map_err(|e| format!("Cannot publish private key: {e}"))?;
    let warning = public.persist_noclobber(&public_path).err().map(|e| format!("Private key saved, but public file could not be published: {e}. Save the displayed public key separately."));
    Ok(Generated {
        private_path: path.into(),
        public_path,
        public_key,
        fingerprint,
        warning,
    })
}

/// Public key of an existing private key (ssh4gui "Copy public key"): the
/// public half stored in an OpenSSH private key (readable without the
/// passphrase, even for encrypted keys), so a stale `.pub` file can never be
/// copied instead. Legacy PEM keys fall back to their adjacent `.pub` file.
pub fn public_key_for(private_path: &Path) -> Result<String, String> {
    let text = crate::keyfile::read(private_path)?;
    if let Ok(key) = ssh_key::PrivateKey::from_openssh(text.as_bytes()) {
        return key
            .public_key()
            .to_openssh()
            .map_err(|error| error.to_string());
    }
    let mut public_path = private_path.as_os_str().to_owned();
    public_path.push(".pub");
    std::fs::read_to_string(&public_path)
        .ok()
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .filter(|line| {
                    line.len() <= 16 * 1024 && ssh_key::PublicKey::from_openssh(line).is_ok()
                })
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            "Cannot read a public key from this file. Choose an OpenSSH private key or keep its .pub file next to it.".to_owned()
        })
}

/// One-line command that adds `public_key` to `~/.ssh/authorized_keys` with
/// private permissions; paste it into a shell on the server (ssh4gui). It runs
/// in its own `sh`, so the umask does not stick to the user's shell and it
/// works from csh or fish too (it contains no `!`). The key's type and data
/// (`$1`) and the full line (`$2`) are arguments, never script text. A last
/// line without a newline is ended first (so two keys are never joined), and
/// a key that is already present (with any comment or options, which a bare
/// copy would lift) is not added again; commented-out lines do not count.
pub fn authorized_keys_command(public_key: &str) -> Result<String, String> {
    let key = public_key.trim();
    if key.is_empty() || key.contains(['\n', '\r', '\0']) {
        return Err("The public key must be a single line.".into());
    }
    let parsed =
        ssh_key::PublicKey::from_openssh(key).map_err(|_| "This is not an OpenSSH public key.")?;
    let encoded = parsed.to_openssh().map_err(|e| e.to_string())?;
    let identity = encoded
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let quote = |text: &str| format!("'{}'", text.replace('\'', r"'\''"));
    let (identity, line) = (quote(&identity), quote(key));
    Ok(format!(
        r#"sh -c 'umask 077; mkdir -p ~/.ssh && cd ~/.ssh && if test -s authorized_keys && test -n "$(tail -c 1 authorized_keys)"; then echo >> authorized_keys; fi && {{ awk -v k="$1" "/^[ \t]*#/ {{ next }} {{ for (i = 1; i < NF; i++) if (\$i \" \" \$(i + 1) == k) f = 1 }} END {{ exit (f ? 0 : 1) }}" authorized_keys 2>/dev/null || printf "%s\n" "$2" >> authorized_keys; }}' sh {identity} {line}"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_generation_writes_only_an_encrypted_private_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        assert!(generate_with_passphrase(&path, Some("")).is_err());
        assert!(!path.exists());
        let saved = generate_with_passphrase(&path, Some("test passphrase")).unwrap();
        let text = std::fs::read(&path).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(text).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("wrong").is_err());
        let unlocked = key.decrypt("test passphrase").unwrap();
        assert_eq!(
            unlocked.public_key().to_openssh().unwrap(),
            saved.public_key
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn generated_pair_matches_and_existing_keys_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        let generated = generate(&path).unwrap();
        assert!(generated.warning.is_none());
        let original = std::fs::read(&path).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&original).unwrap();
        assert_eq!(key.public_key().to_openssh().unwrap(), generated.public_key);
        assert_eq!(
            std::fs::read_to_string(&generated.public_path).unwrap(),
            format!("{}\n", generated.public_key)
        );
        assert!(generate(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn public_key_comes_from_pub_file_or_private_key_and_builds_a_quoted_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_test");
        let generated = generate(&path).unwrap();
        assert_eq!(public_key_for(&path).unwrap(), generated.public_key);
        std::fs::remove_file(&generated.public_path).unwrap();
        assert_eq!(public_key_for(&path).unwrap(), generated.public_key);
        std::fs::write(dir.path().join("not_a_key"), "hello").unwrap();
        assert!(public_key_for(&dir.path().join("not_a_key")).is_err());
        let key = format!(
            "{} it's mine",
            generated.public_key.rsplit_once(' ').unwrap().0
        );
        let command = authorized_keys_command(&key).unwrap();
        assert!(command.starts_with("sh -c 'umask 077; mkdir -p ~/.ssh && "));
        assert!(
            command.contains("' sh 'ssh-ed25519 "),
            "key is one sh argument"
        );
        assert!(command.ends_with(r"it'\''s mine'"));
        assert!(!command.contains('\n'), "must paste as one line");
        assert!(authorized_keys_command("ssh-ed25519 AAAA\nrm -rf /").is_err());
        assert!(authorized_keys_command("not a key").is_err());
        // A stale .pub file never wins over the private key's own public half.
        std::fs::write(&generated.public_path, &key).unwrap();
        assert_eq!(public_key_for(&path).unwrap(), generated.public_key);
    }

    #[test]
    fn install_command_ends_the_last_line_and_adds_a_key_once() {
        let shell = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/sh"
        };
        if !Path::new(shell).is_file() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let generated = generate(&dir.path().join("id_new")).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        let file = home.join(".ssh").join("authorized_keys");
        let old = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOld old@host";
        std::fs::write(&file, old).unwrap(); // no trailing newline
        let command = authorized_keys_command(&generated.public_key).unwrap();
        for _ in 0..2 {
            let output = std::process::Command::new(shell)
                .args(["-c", &command])
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("{old}\n{}\n", generated.public_key)
        );
        // A fresh account gets the folder and the file.
        std::fs::remove_dir_all(home.join(".ssh")).unwrap();
        let output = std::process::Command::new(shell)
            .args(["-c", &command])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("{}\n", generated.public_key)
        );
        // The same key with options or another comment is already installed:
        // a bare copy would lift its restrictions. A commented-out one is not.
        let identity = generated
            .public_key
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        for (existing, added) in [
            (
                format!("command=\"backup --x\",restrict {identity} a\n"),
                false,
            ),
            (format!("{identity} other@comment\n"), false),
            (format!("# {identity}\n"), true),
        ] {
            std::fs::write(&file, &existing).unwrap();
            let output = std::process::Command::new(shell)
                .args(["-c", &command])
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(output.status.success());
            let expected = if added {
                format!("{existing}{}\n", generated.public_key)
            } else {
                existing.clone()
            };
            assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
        }
    }
}
