use super::{super::hash::HashType, KeyExchange};
use crate::error::SshError;
use crate::SshResult;
use ring::agreement::{EphemeralPrivateKey, PublicKey, UnparsedPublicKey, X25519};

pub(super) struct CURVE25519 {
    pub private_key: std::sync::Mutex<Option<EphemeralPrivateKey>>,
    pub public_key: PublicKey,
}

impl KeyExchange for CURVE25519 {
    fn new() -> SshResult<Self> {
        let rng = ring::rand::SystemRandom::new();
        let private_key = match EphemeralPrivateKey::generate(&X25519, &rng) {
            Ok(v) => v,
            Err(e) => return Err(SshError::KexError(e.to_string())),
        };
        match private_key.compute_public_key() {
            Ok(public_key) => Ok(CURVE25519 {
                private_key: std::sync::Mutex::new(Some(private_key)),
                public_key,
            }),
            Err(e) => Err(SshError::KexError(e.to_string())),
        }
    }

    fn get_public_key(&self) -> &[u8] {
        self.public_key.as_ref()
    }

    fn get_shared_secret(&self, puk: Vec<u8>) -> SshResult<Vec<u8>> {
        let mut public_key = [0u8; 32];
        if puk.len() != public_key.len() {
            return Err(SshError::KexError(
                "Invalid X25519 public key length.".into(),
            ));
        }
        public_key.copy_from_slice(&puk);

        let server_pub = UnparsedPublicKey::new(&X25519, public_key);
        let private_key = self
            .private_key
            .lock()
            .map_err(|_| SshError::KexError("Ephemeral key unavailable.".into()))?
            .take()
            .ok_or_else(|| SshError::KexError("Ephemeral key already consumed.".into()))?;
        crate::algorithm::key_exchange::agree_ephemeral(private_key, &server_pub)
    }

    fn get_hash_type(&self) -> HashType {
        HashType::SHA256
    }
}
