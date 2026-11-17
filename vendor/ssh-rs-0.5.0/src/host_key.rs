//! Optional host-key verification hook (ssh4 local addition).
//!
//! The application sets a verifier and pending peer **on the connecting thread**
//! before handshake. During KEX, when the server host key (K_S) is received, the
//! verifier is invoked. Installation is thread-local; each connection captures
//! its own policy and proof state so nested transports and later rekeys cannot
//! interleave peers or policies.
//!
//! The verifier should only **check** policy during KEX. Persistence of newly
//! accepted keys should happen after signature verification succeeds (see
//! `mark_host_key_verified` / application-side commit).

use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

type Verifier = Arc<dyn Fn(&str, &[u8]) -> Result<(), String> + Send + Sync>;

thread_local! {
    static VERIFIER: RefCell<Option<Verifier>> = const { RefCell::new(None) };
    static PENDING_PEER: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Set true only after KEX signature verification succeeds.
    static HOST_KEY_PROOF_OK: RefCell<Arc<AtomicBool>> = RefCell::new(Arc::new(AtomicBool::new(false)));
}

/// Register (or clear with `None`) the host-key verifier for the current thread.
///
/// Installing a **new** verifier resets the signature-proof flag. Clearing with
/// `None` leaves the proof flag intact so the caller can still commit a staged
/// known_hosts write after a successful connect.
pub fn set_host_key_verifier(
    verifier: Option<Box<dyn Fn(&str, &[u8]) -> Result<(), String> + Send + Sync>>,
) {
    if verifier.is_some() {
        HOST_KEY_PROOF_OK.with(|cell| {
            *cell.borrow_mut() = Arc::new(AtomicBool::new(false));
        });
    }
    VERIFIER.with(|cell| {
        *cell.borrow_mut() = verifier.map(Arc::from);
    });
}

/// Set the peer address label used for the next handshake on this thread
/// (e.g. `host:22`).
pub fn set_pending_peer(peer: Option<String>) {
    PENDING_PEER.with(|cell| {
        *cell.borrow_mut() = peer;
    });
}

/// Called by KEX after the host key signature verifies successfully.
pub fn mark_host_key_verified() {
    HOST_KEY_PROOF_OK.with(|cell| {
        cell.borrow().store(true, Ordering::Release);
    });
}

/// Whether KEX completed signature verification for the staged host key.
pub fn host_key_proof_ok() -> bool {
    HOST_KEY_PROOF_OK.with(|cell| cell.borrow().load(Ordering::Acquire))
}

/// Reset proof flag (failure paths / cleanup).
pub fn clear_host_key_proof() {
    HOST_KEY_PROOF_OK.with(|cell| {
        cell.borrow().store(false, Ordering::Release);
    });
}

/// Verification policy and proof state captured when a connection is created.
/// Nested transports must never borrow another connection's thread-local policy.
pub(crate) struct Context {
    verifier: Option<Verifier>,
    peer: String,
    proof: Arc<AtomicBool>,
}
impl Context {
    pub(crate) fn capture() -> Self {
        Self {
            verifier: VERIFIER.with(|cell| cell.borrow().clone()),
            peer: PENDING_PEER
                .with(|cell| cell.borrow().clone().unwrap_or_else(|| "unknown".into())),
            proof: HOST_KEY_PROOF_OK.with(|cell| {
                let proof = Arc::new(AtomicBool::new(false));
                *cell.borrow_mut() = proof.clone();
                proof
            }),
        }
    }
    pub(crate) fn verify(&self, key_blob: &[u8]) -> Result<(), String> {
        self.proof.store(false, Ordering::Release);
        if let Some(verifier) = &self.verifier {
            verifier(&self.peer, key_blob)
        } else {
            Ok(())
        }
    }
    pub(crate) fn mark_verified(&self) {
        self.proof.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_connections_keep_their_own_peer_policy_and_signature_proof() {
        set_pending_peer(Some("jump".into()));
        set_host_key_verifier(Some(Box::new(|peer, blob| {
            if peer == "jump" && blob == b"jump-key" {
                Ok(())
            } else {
                Err("wrong jump key".into())
            }
        })));
        let jump = Context::capture();
        set_pending_peer(Some("destination".into()));
        set_host_key_verifier(Some(Box::new(|peer, blob| {
            if peer == "destination" && blob == b"destination-key" {
                Ok(())
            } else {
                Err("wrong destination key".into())
            }
        })));
        let destination = Context::capture();
        jump.verify(b"jump-key").unwrap();
        jump.mark_verified();
        assert!(
            !host_key_proof_ok(),
            "Upstream proof must not authorize downstream persistence"
        );
        assert!(destination.verify(b"jump-key").is_err());
        destination.verify(b"destination-key").unwrap();
        assert!(
            !host_key_proof_ok(),
            "Policy acceptance is not signature proof"
        );
        destination.mark_verified();
        assert!(host_key_proof_ok());
        assert!(jump.verify(b"destination-key").is_err());
        assert!(
            host_key_proof_ok(),
            "Upstream rekey must not reset downstream proof"
        );
        set_host_key_verifier(None);
        set_pending_peer(None);
        assert!(
            destination.verify(b"jump-key").is_err(),
            "Clearing thread hooks must not disable existing connection checks"
        );
        let first = Context::capture();
        let _second = Context::capture();
        first.mark_verified();
        assert!(
            !host_key_proof_ok(),
            "Each connection needs independent proof even with the same policy"
        );
        clear_host_key_proof();
    }
}
