//! Bounded private-key reads and cancellable waiting for passphrase decryption.
use std::{
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use zeroize::Zeroizing;
static DECRYPTORS: AtomicUsize = AtomicUsize::new(0);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        DECRYPTORS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) fn read(path: &Path) -> Result<Zeroizing<String>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot read private key: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Private key must be a regular file.".into());
    }
    let mut text = Zeroizing::new(String::new());
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "Private key is not valid UTF-8 text.")?;
    if text.len() > 1024 * 1024 {
        return Err("Private key exceeds the 1 MiB limit.".into());
    }
    Ok(text)
}
pub(crate) fn encrypted(text: &str) -> Option<ssh_key::PrivateKey> {
    const BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
    const END: &str = "-----END OPENSSH PRIVATE KEY-----";
    let body = text.trim().strip_prefix(BEGIN)?.strip_suffix(END)?;
    let body = Zeroizing::new(body.lines().map(str::trim).collect::<String>());
    if !body.is_ascii() {
        return None;
    }
    let mut normalized = Zeroizing::new(format!("{BEGIN}\n"));
    for line in body.as_bytes().chunks(70) {
        normalized.push_str(std::str::from_utf8(line).ok()?);
        normalized.push('\n');
    }
    normalized.push_str(END);
    normalized.push('\n');
    ssh_key::PrivateKey::from_openssh(normalized.as_str())
        .ok()
        .filter(|key| key.is_encrypted())
}
pub(crate) fn decrypt(
    key: ssh_key::PrivateKey,
    password: Zeroizing<String>,
    stop: &AtomicBool,
) -> Result<Zeroizing<String>, String> {
    if stop.load(Ordering::Acquire) {
        return Err("Key decryption cancelled.".into());
    }
    match key.kdf() {
        ssh_key::Kdf::Bcrypt { salt, rounds } if (1..=256).contains(rounds) && salt.len() <= 1024 => {},
        _ => return Err("Encrypted key KDF settings exceed the supported work limit; unlock this key with an SSH agent.".into()),
    }
    DECRYPTORS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 2).then_some(n + 1)
        })
        .map_err(|_| "Previous key decryption is still finishing. Try again shortly.")?;
    let guard = Guard;
    let (sender, result) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("key-decrypt".into())
        .spawn(move || {
            let _guard = guard;
            let decoded = key
                .decrypt(password.as_bytes())
                .and_then(|key| key.to_openssh(ssh_key::LineEnding::LF))
                .map_err(|_| "Incorrect passphrase or damaged encrypted key.".to_owned());
            let _ = sender.send(decoded);
        })
        .map_err(|e| e.to_string())?;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Key decryption cancelled.".into());
        }
        match result.recv_timeout(Duration::from_millis(20)) {
            Ok(value) => return value,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("Private-key decryptor ended unexpectedly.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_keys_unlock_without_changing_the_source_and_reject_wrong_passphrases() {
        let key = ssh_key::PrivateKey::new(
            ssh_key::private::KeypairData::Ed25519(ssh_key::private::Ed25519Keypair::from_seed(
                &[42; 32],
            )),
            "test",
        )
        .unwrap();
        let locked = key
            .encrypt(&mut rand::rngs::OsRng, "test passphrase")
            .unwrap();
        let text = locked.to_openssh(ssh_key::LineEnding::LF).unwrap();
        let body: String = text
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let mut wide = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
        for line in body.as_bytes().chunks(76) {
            wide.push_str(std::str::from_utf8(line).unwrap());
            wide.push('\n');
        }
        wide.push_str("-----END OPENSSH PRIVATE KEY-----\n");
        assert!(encrypted(&wide).is_some());
        let stop = AtomicBool::new(false);
        let unlocked = decrypt(
            encrypted(&text).unwrap(),
            Zeroizing::new("test passphrase".into()),
            &stop,
        )
        .unwrap();
        assert_eq!(
            ssh_key::PrivateKey::from_openssh(&unlocked)
                .unwrap()
                .public_key(),
            key.public_key()
        );
        assert!(decrypt(
            encrypted(&text).unwrap(),
            Zeroizing::new("wrong".into()),
            &stop
        )
        .is_err());
        assert!(encrypted(&text).unwrap().is_encrypted());
        stop.store(true, Ordering::Release);
        assert!(decrypt(locked, Zeroizing::new("test passphrase".into()), &stop).is_err());
    }
}
