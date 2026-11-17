use crate::algorithm::{
    hash::{self, HashCtx, HashType},
    PubKey,
};
use crate::model::Data;
use crate::{SshError, SshResult};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::Pkcs1v15Sign;
use rsa::traits::PublicKeyParts;
use std::fmt::Debug;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const KEY_FILE_MAGIC_START: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const KEY_FILE_MAGIC_END: &str = "-----END OPENSSH PRIVATE KEY-----";

// ssh-key's PEM decoder expects OpenSSH's 70-column wrapping. Other valid
// writers (including cryptography) use 76 columns. Normalize only the armor;
// the decoder still validates the base64 and the complete key payload.
fn normalize_openssh_pem(key: &str) -> String {
    let Some(body) = key
        .strip_prefix(KEY_FILE_MAGIC_START)
        .and_then(|body| body.strip_suffix(KEY_FILE_MAGIC_END))
    else {
        return key.to_owned();
    };
    let encoded: String = body.lines().map(str::trim).collect();
    if !encoded.is_ascii() {
        return key.to_owned();
    }
    let mut pem = format!("{KEY_FILE_MAGIC_START}\n");
    for line in encoded.as_bytes().chunks(70) {
        pem.push_str(std::str::from_utf8(line).unwrap());
        pem.push('\n');
    }
    pem.push_str(KEY_FILE_MAGIC_END);
    pem.push('\n');
    pem
}

#[derive(Clone, Default)]
pub struct KeyPair {
    pub(super) private_key: String,
    pub(super) key_type: KeyType,
}

impl KeyPair {
    pub fn from_str(key_str: &str) -> SshResult<Self> {
        // first validate the key
        let key_str = key_str.trim().to_owned();

        let (key_type, private_key) = if rsa::RsaPrivateKey::from_pkcs1_pem(&key_str).is_ok() {
            (KeyType::PemRsa, key_str)
        } else if key_str.starts_with(KEY_FILE_MAGIC_START) {
            let key_str = normalize_openssh_pem(&key_str);
            match ssh_key::PrivateKey::from_openssh(&key_str) {
                Ok(prk) if prk.is_encrypted() => {
                    return Err(SshError::SshPubKeyError(
                        "Encrypted private key must be unlocked before authentication".into(),
                    ))
                }
                Ok(prk) => match prk.algorithm() {
                    ssh_key::Algorithm::Rsa { hash: _hash } => (KeyType::SshRsa, key_str),
                    ssh_key::Algorithm::Ed25519 => (KeyType::SshEd25519, key_str),
                    x => {
                        return Err(SshError::SshPubKeyError(format!(
                            "Currently don't support the key file type {}",
                            x
                        )))
                    }
                },
                Err(e) => return Err(SshError::SshPubKeyError(e.to_string())),
            }
        } else {
            return Err(SshError::SshPubKeyError(
                "Unable to detect the pulic key type".to_owned(),
            ));
        };

        // then store it
        let pair = KeyPair {
            private_key,
            key_type,
        };
        Ok(pair)
    }

    /// User authentication is independent of the server's host-key algorithm.
    pub(crate) fn authentication_algorithm(&self) -> PubKey {
        match self.key_type {
            KeyType::PemRsa | KeyType::SshRsa => PubKey::RsaSha2_512,
            KeyType::SshEd25519 => PubKey::SshEd25519,
        }
    }

    pub fn get_blob(&self, _alg: &PubKey) -> Vec<u8> {
        match self.key_type {
            KeyType::PemRsa => {
                // already valid key string, just unwrap it.
                let rprk = rsa::RsaPrivateKey::from_pkcs1_pem(&self.private_key).unwrap();
                let rpuk = rprk.to_public_key();
                let es = rpuk.e().to_bytes_be();
                let ns = rpuk.n().to_bytes_be();
                let mut blob = Data::new();
                // RFC 8332 keeps ssh-rsa in the key blob for both SHA-2 signatures.
                blob.put_str("ssh-rsa");
                blob.put_mpint(&es);
                blob.put_mpint(&ns);
                blob.to_vec()
            }
            KeyType::SshRsa => {
                let prk = ssh_key::PrivateKey::from_openssh(&self.private_key).unwrap();
                let rsa = prk.key_data().rsa().unwrap();
                let es = rsa.public.e.as_bytes();
                let ns = rsa.public.n.as_bytes();
                let mut blob = Data::new();
                blob.put_str("ssh-rsa");
                blob.put_mpint(es);
                blob.put_mpint(ns);
                blob.to_vec()
            }
            KeyType::SshEd25519 => {
                let prk = ssh_key::PrivateKey::from_openssh(&self.private_key).unwrap();
                let ed25519 = prk.key_data().ed25519().unwrap();
                let mut blob = Data::new();
                blob.put_str("ssh-ed25519");
                blob.put_u8s(ed25519.public.as_ref());
                blob.to_vec()
            }
        }
    }

    fn sign(&self, sd: &[u8], alg: &PubKey) -> SshResult<Vec<u8>> {
        match self.key_type {
            KeyType::PemRsa | KeyType::SshRsa => {
                let (scheme, digest) = match alg {
                    PubKey::RsaSha2_512 => (
                        Pkcs1v15Sign::new::<sha2::Sha512>(),
                        ring::digest::digest(&ring::digest::SHA512, sd),
                    ),
                    PubKey::RsaSha2_256 => (
                        Pkcs1v15Sign::new::<sha2::Sha256>(),
                        ring::digest::digest(&ring::digest::SHA256, sd),
                    ),
                    #[cfg(feature = "deprecated-rsa-sha1")]
                    PubKey::SshRsa => (
                        Pkcs1v15Sign::new::<sha1::Sha1>(),
                        ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, sd),
                    ),
                    _ => {
                        return Err(SshError::SshPubKeyError(
                            "Signature algorithm does not match RSA key".into(),
                        ))
                    }
                };

                let msg = digest.as_ref();
                let rprk = match self.key_type {
                    KeyType::PemRsa => {
                        rsa::RsaPrivateKey::from_pkcs1_pem(self.private_key.as_str()).unwrap()
                    }
                    KeyType::SshRsa => {
                        // the sign method in ssh_key itself can only work with sha2-512
                        // so we convert it to the raw rsa key
                        let prk = ssh_key::PrivateKey::from_openssh(&self.private_key).unwrap();
                        let rsa = prk.key_data().rsa().unwrap();
                        // ssh-key 0.6.7's conversion mistakenly uses p twice.
                        // Construct from both primes and validate before signing.
                        let integer = |value: &ssh_key::Mpint| {
                            rsa::BigUint::try_from(value)
                                .map_err(|e| SshError::SshPubKeyError(e.to_string()))
                        };
                        let key = rsa::RsaPrivateKey::from_components(
                            integer(&rsa.public.n)?,
                            integer(&rsa.public.e)?,
                            integer(&rsa.private.d)?,
                            vec![integer(&rsa.private.p)?, integer(&rsa.private.q)?],
                        )
                        .map_err(|e| SshError::SshPubKeyError(e.to_string()))?;
                        key.validate()
                            .map_err(|e| SshError::SshPubKeyError(e.to_string()))?;
                        key
                    }
                    _ => unreachable!(),
                };

                rprk.sign(scheme, msg)
                    .map_err(|e| SshError::SshPubKeyError(e.to_string()))
            }
            KeyType::SshEd25519 => {
                use signature::Signer;
                let prk = ssh_key::PrivateKey::from_openssh(&self.private_key).unwrap();
                let sign = prk
                    .try_sign(sd)
                    .map_err(|e| SshError::SshPubKeyError(e.to_string()))?;
                Ok(sign.as_bytes().to_vec())
            }
        }
    }

    pub(crate) fn signature(
        &self,
        buf: &[u8],
        hash_ctx: HashCtx,
        hash_type: HashType,
        alg: &PubKey,
    ) -> SshResult<Vec<u8>> {
        let session_id = hash::digest(hash_ctx.as_bytes().as_slice(), hash_type);
        let mut sd = Data::new();
        sd.put_u8s(session_id.as_slice());
        sd.extend_from_slice(buf);
        let sign = self.sign(&sd, alg)?;
        let mut ss = Data::new();
        ss.put_str(alg.as_ref());
        ss.put_u8s(&sign);
        Ok(ss.to_vec())
    }
}

#[derive(Clone, Default)]
pub(super) enum KeyType {
    #[default]
    PemRsa,
    SshRsa,
    SshEd25519,
}

#[derive(Clone, Default)]
pub(crate) struct AuthInfo {
    pub username: String,
    pub password: String,
    pub key_pair: Option<KeyPair>,
    /// Public key blobs from an SSH agent (tried before file key / password).
    pub agent_blobs: Vec<Vec<u8>>,
    /// When true, attempt keyboard-interactive after password (or if password empty).
    pub keyboard_interactive: bool,
}

impl Debug for AuthInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "username: {}", self.username)?;
        Ok(())
    }
}

impl AuthInfo {
    pub fn username<U>(&mut self, u: U) -> SshResult<()>
    where
        U: ToString,
    {
        self.username = u.to_string();
        Ok(())
    }

    pub fn password<P>(&mut self, p: P) -> SshResult<()>
    where
        P: ToString,
    {
        self.password = p.to_string();
        Ok(())
    }

    pub fn private_key<K>(&mut self, k: K) -> SshResult<()>
    where
        K: ToString,
    {
        self.key_pair = Some((KeyPair::from_str(&k.to_string()))?);
        Ok(())
    }

    pub fn private_key_path<P>(&mut self, p: P) -> SshResult<()>
    where
        P: AsRef<Path>,
    {
        let mut file = File::open(p)?;
        let mut prks = String::new();
        file.read_to_string(&mut prks)?;

        self.key_pair = Some((KeyPair::from_str(&prks))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;

    #[test]
    fn rsa_formats_use_canonical_blob_and_valid_sha2_signatures() {
        let private = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
        let pem = private.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let openssh = ssh_key::PrivateKey::new(
            ssh_key::private::KeypairData::Rsa((&private).try_into().unwrap()),
            "fixture",
        )
        .unwrap()
        .to_openssh(ssh_key::LineEnding::LF)
        .unwrap();
        let body: String = openssh
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let mut rewrapped = format!("{KEY_FILE_MAGIC_START}\r\n");
        for line in body.as_bytes().chunks(76) {
            rewrapped.push_str(std::str::from_utf8(line).unwrap());
            rewrapped.push_str("\r\n");
        }
        rewrapped.push_str(KEY_FILE_MAGIC_END);
        let mut expected_blob = None;
        for encoded in [pem.as_str(), openssh.as_str(), rewrapped.as_str()] {
            let key = KeyPair::from_str(encoded).unwrap();
            assert_eq!(key.authentication_algorithm().as_ref(), "rsa-sha2-512");
            // Even an unrelated server host-key algorithm cannot change the blob.
            let blob = key.get_blob(&PubKey::SshEd25519);
            assert_eq!(&blob[4..11], b"ssh-rsa");
            if let Some(expected) = &expected_blob {
                assert_eq!(&blob, expected);
            } else {
                expected_blob = Some(blob);
            }
            for alg in [PubKey::RsaSha2_256, PubKey::RsaSha2_512] {
                let signature = key.sign(b"signed fixture", &alg).unwrap();
                let (scheme, digest) = match alg {
                    PubKey::RsaSha2_256 => (
                        Pkcs1v15Sign::new::<sha2::Sha256>(),
                        ring::digest::digest(&ring::digest::SHA256, b"signed fixture"),
                    ),
                    _ => (
                        Pkcs1v15Sign::new::<sha2::Sha512>(),
                        ring::digest::digest(&ring::digest::SHA512, b"signed fixture"),
                    ),
                };
                private
                    .to_public_key()
                    .verify(scheme, digest.as_ref(), &signature)
                    .unwrap();
            }
            assert!(key.sign(b"fixture", &PubKey::SshEd25519).is_err());
        }
    }

    #[test]
    fn encrypted_keys_are_rejected_before_public_blob_extraction() {
        let encoded = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABD1
xAtUxQvxOZTWOQmvW/ERAAAAEAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAIBl/
ayPhbIUyxqvIOPrNXqeJvgx2spIDNAOb+os9No1hAAAAkNrtN+uv7YkqfmTqQezQ
9ubSLyY6JqfxqtdJ2YWh8/HALRJZ/Xah6xIb0B2BnDLa1j0guIcY3ftMbllDAADR
RuYaBth01dsax9UW4oTUHupvBcHOzxzXHqre+xi4owKCsyoVmYFHalXr+pyX/L90
fqeNN3kjRu0QABnOi+aMmz4VbhryTHXZrpLLHMtEIKT8ng==
-----END OPENSSH PRIVATE KEY-----
"#;
        assert!(KeyPair::from_str(encoded)
            .err()
            .unwrap()
            .to_string()
            .contains("must be unlocked"));
    }

    #[test]
    fn ed25519_auth_is_independent_of_server_key() {
        let private = ssh_key::PrivateKey::new(
            ssh_key::private::KeypairData::Ed25519(ssh_key::private::Ed25519Keypair::from_seed(
                &[42; 32],
            )),
            "fixture",
        )
        .unwrap();
        let encoded = private.to_openssh(ssh_key::LineEnding::LF).unwrap();
        let key = KeyPair::from_str(&encoded).unwrap();
        assert_eq!(key.authentication_algorithm().as_ref(), "ssh-ed25519");
        assert_eq!(&key.get_blob(&PubKey::RsaSha2_512)[4..15], b"ssh-ed25519");
        let signature = key
            .sign(b"fixture", &key.authentication_algorithm())
            .unwrap();
        assert_eq!(signature.len(), 64);
    }
}
