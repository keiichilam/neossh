use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Clone, Debug)]
pub struct HostKey {
    pub peer: String,
    pub algorithm: String,
    pub encoded: String,
    pub fingerprint: String,
    /// Set for prompts when known_hosts has this host only with other key
    /// types ([`Status::NewKeyType`]), so the prompt can warn.
    pub other_types_known: bool,
}

impl HostKey {
    /// The prompt heading and advice for a key that is not yet trusted.
    pub fn trust_warning(&self) -> (String, &'static str) {
        if self.other_types_known {
            (
                format!(
                    "{} is already known, but not with a {} key",
                    self.peer, self.algorithm
                ),
                "Trust it only if the server administrator added this key type. \
                 Otherwise someone may be intercepting the connection.",
            )
        } else {
            (
                format!("First connection to {}", self.peer),
                "Compare this fingerprint with the server administrator's record.",
            )
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Known,
    Unknown,
    /// The host is known only by keys of other types. Servers can add key
    /// types, but so can someone intercepting the connection, so this key is
    /// never accepted without asking.
    NewKeyType,
    Changed,
    Revoked,
}

impl HostKey {
    pub fn new(host: &str, port: u16, blob: &[u8]) -> Result<Self, String> {
        let header: [u8; 4] = blob
            .get(..4)
            .ok_or("Truncated host key.")?
            .try_into()
            .unwrap();
        let length = u32::from_be_bytes(header) as usize;
        let algorithm = std::str::from_utf8(
            blob.get(4..4usize.checked_add(length).ok_or("Invalid host key.")?)
                .ok_or("Truncated host-key algorithm.")?,
        )
        .map_err(|_| "Invalid host-key algorithm.")?;
        if algorithm.is_empty()
            || !algorithm
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-@._".contains(&b))
        {
            return Err("Invalid host-key algorithm.".into());
        }
        let host = host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| {
                if let Some((address, zone)) = host.split_once('%') {
                    if let Ok(address) = address.parse::<std::net::Ipv6Addr>() {
                        return format!("{address}%{zone}");
                    }
                }
                host.to_ascii_lowercase()
            });
        Ok(Self {
            peer: if port == 22 {
                host
            } else {
                format!("[{host}]:{port}")
            },
            algorithm: algorithm.into(),
            encoded: STANDARD.encode(blob),
            fingerprint: format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(blob))),
            other_types_known: false,
        })
    }

    pub fn check(&self, text: &str) -> Status {
        let mut status = Status::Unknown;
        let mut other_type = false;
        for line in text.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.is_empty() || fields[0].starts_with('#') {
                continue;
            }
            let (marker, fields) = if fields[0].starts_with('@') {
                (Some(fields[0]), &fields[1..])
            } else {
                (None, fields.as_slice())
            };
            if fields.len() < 3 || !matches_peer(fields[0], &self.peer) {
                continue;
            }
            if fields[1] != self.algorithm {
                other_type |= marker.is_none();
                continue;
            }
            if marker == Some("@revoked") && fields[2] == self.encoded {
                return Status::Revoked;
            }
            if marker.is_some() {
                continue;
            }
            if fields[2] == self.encoded {
                status = Status::Known;
            } else if status != Status::Known {
                status = Status::Changed;
            }
        }
        if status == Status::Unknown && other_type {
            Status::NewKeyType
        } else {
            status
        }
    }

    pub fn check_file(&self, path: &Path) -> Result<Status, String> {
        Ok(self.check(&read_file(path)?))
    }

    /// Call only after the transport confirms the KEX signature proof.
    pub fn persist_verified(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or("Known-hosts file needs a parent directory.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("ssh4-lock"))
            .map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let text = read_file(path)?;
        match self.check(&text) {
            Status::Known => return Ok(()),
            Status::Revoked | Status::Changed => {
                return Err(
                    "Host trust changed while connecting. Review known_hosts before reconnecting."
                        .into(),
                )
            }
            Status::Unknown | Status::NewKeyType => {}
        }
        let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staging
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        staging
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        if !text.is_empty() && !text.ends_with('\n') {
            staging.write_all(b"\n").map_err(|e| e.to_string())?;
        }
        writeln!(staging, "{} {} {}", self.peer, self.algorithm, self.encoded)
            .map_err(|e| e.to_string())?;
        staging.as_file().sync_all().map_err(|e| e.to_string())?;
        staging.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn read_file(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("Cannot read known_hosts: {e}")),
    }
}

fn matches_peer(patterns: &str, peer: &str) -> bool {
    if patterns.starts_with('|') {
        return matches_hashed_peer(patterns, peer);
    }
    let mut matched = false;
    for pattern in patterns.split(',') {
        if pattern.starts_with('|') {
            continue;
        }
        let (negative, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern), |p| (true, p));
        if glob(pattern.as_bytes(), peer.as_bytes()) {
            if negative {
                return false;
            }
            matched = true;
        }
    }
    matched
}

fn matches_hashed_peer(pattern: &str, peer: &str) -> bool {
    // OpenSSH's legacy hostname obfuscation is HMAC-SHA1 with a 20-byte salt.
    // Hashed entries represent one complete host token, never a pattern list.
    let Some(value) = pattern.strip_prefix("|1|") else {
        return false;
    };
    let Some((salt, digest)) = value.split_once('|') else {
        return false;
    };
    if salt.len() != 28 || digest.len() != 28 {
        return false;
    }
    let (Ok(salt), Ok(digest)) = (STANDARD.decode(salt), STANDARD.decode(digest)) else {
        return false;
    };
    if salt.len() != 20 || digest.len() != 20 {
        return false;
    }
    let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.update(peer.as_bytes());
    mac.verify_slice(&digest).is_ok()
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t, mut star, mut retry) = (0, 0, None, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = t;
        } else if let Some(index) = star {
            retry += 1;
            t = retry;
            p = index + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    const HASHED_SERVER: &str = "|1|AAECAwQFBgcICQoLDA0ODxAREhM=|EK886lDLij/nyfj1Ye1/UNgJlTY=";
    fn key(extra: u8) -> HostKey {
        HostKey::new(
            "server",
            22,
            &[0, 0, 0, 7, b's', b's', b'h', b'-', b'r', b's', b'a', extra],
        )
        .unwrap()
    }
    #[test]
    fn hashed_host_entries_preserve_known_changed_and_revoked_policy() {
        let changed = key(2);
        let key = key(1);
        let line = format!("{HASHED_SERVER} {} {}", key.algorithm, key.encoded);
        assert_eq!(key.check(&line), Status::Known);
        assert_eq!(changed.check(&line), Status::Changed);
        assert_eq!(
            key.check(&format!("{line}\n@revoked {line}")),
            Status::Revoked
        );
        assert_eq!(
            key.check(&format!("@cert-authority {line}")),
            Status::Unknown
        );
        assert!(!matches_peer(HASHED_SERVER, "other"));
        assert!(!matches_peer(HASHED_SERVER, "[server]:2222"));
        let encoded = STANDARD.decode(&key.encoded).unwrap();
        assert_eq!(
            HostKey::new("FE80:0:0:0:0:0:0:1%Ethernet", 22, &encoded)
                .unwrap()
                .peer,
            "fe80::1%Ethernet"
        );
        assert_eq!(
            HostKey::new("SERVER", 22, &encoded).unwrap().check(&line),
            Status::Known
        );
        assert_eq!(
            HostKey::new("2001:0DB8:0:0:0:0:0:1", 2222, &encoded)
                .unwrap()
                .peer,
            "[2001:db8::1]:2222"
        );
    }

    #[test]
    fn malformed_hashes_never_match_or_override_a_valid_entry() {
        let changed = key(2);
        let key = key(1);
        for invalid in [
            "|1|",
            "|2|salt|digest",
            "|1|bad|bad",
            "|1|AAECAwQFBgcICQoLDA0ODxAREhM=|!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
        ] {
            assert!(!matches_peer(invalid, "server"));
            assert_eq!(
                key.check(&format!("{invalid} {} {}", key.algorithm, key.encoded)),
                Status::Unknown
            );
        }
        assert!(!matches_peer(&format!("{HASHED_SERVER},server"), "server"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let line = format!("{HASHED_SERVER} {} {}\n", key.algorithm, key.encoded);
        fs::write(&path, &line).unwrap();
        key.persist_verified(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), line);
        assert!(changed.persist_verified(&path).is_err());
    }

    #[test]
    fn host_policy_distinguishes_change_revocation_and_excluded_patterns() {
        let key = key(1);
        let line = format!("*.example,server {} {}", key.algorithm, key.encoded);
        assert_eq!(key.check(&line), Status::Known);
        assert_eq!(
            key.check(&line.replace(&key.encoded, "other")),
            Status::Changed
        );
        assert_eq!(key.check(&format!("@revoked {line}")), Status::Revoked);
        assert_eq!(key.check(&format!("!server,{line}")), Status::Unknown);
        assert_eq!(
            key.check(&format!("@cert-authority {line}")),
            Status::Unknown
        );
    }
    #[test]
    fn a_known_host_presenting_another_key_type_is_flagged() {
        let rsa = key(1);
        let mut blob = vec![0, 0, 0, 11];
        blob.extend_from_slice(b"ssh-ed25519");
        blob.push(7);
        let other = HostKey::new("server", 22, &blob).unwrap();
        let line = format!("server {} {}", rsa.algorithm, rsa.encoded);
        assert_eq!(other.check(&line), Status::NewKeyType);
        // Other hosts' keys and marked lines do not make the host known.
        for text in [
            format!("elsewhere {} {}", rsa.algorithm, rsa.encoded),
            format!("@revoked {line}"),
            format!("@cert-authority {line}"),
        ] {
            assert_eq!(other.check(&text), Status::Unknown, "{text}");
        }
        // Its own type decides once recorded.
        let both = format!("{line}\nserver {} {}", other.algorithm, other.encoded);
        assert_eq!(other.check(&both), Status::Known);
        let changed = format!("{line}\nserver {} AAAA", other.algorithm);
        assert_eq!(other.check(&changed), Status::Changed);
        // Accepting it records the new type beside the old one.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("{line}\n")).unwrap();
        other.persist_verified(&path).unwrap();
        assert_eq!(other.check_file(&path).unwrap(), Status::Known);
        assert_eq!(rsa.check_file(&path).unwrap(), Status::Known);
    }

    #[test]
    fn concurrent_trust_change_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        key(1).persist_verified(&path).unwrap();
        assert!(key(2).persist_verified(&path).is_err());
        assert_eq!(key(1).check_file(&path).unwrap(), Status::Known);
        key(1).persist_verified(&path).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 1);
    }
    #[test]
    fn untrusted_key_blob_is_bounded() {
        assert!(HostKey::new("h", 22, &[255, 255, 255, 255]).is_err());
        assert!(HostKey::new("h", 22, &[]).is_err());
    }
}
