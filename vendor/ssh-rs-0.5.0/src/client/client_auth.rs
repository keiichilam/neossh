use std::io::{Read, Write};
use tracing::*;

use crate::{
    algorithm::{compression, hash, Compress, Digest},
    constant::{ssh_connection_code, ssh_str, ssh_transport_code, ssh_user_auth_code},
    error::{SshError, SshResult},
    model::{Data, Packet, SecPacket},
};

use super::Client;

/// Which publickey path produced the outstanding PK_OK.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PubkeyPending {
    None,
    /// Agent identity index and the publickey algorithm string used in the query
    /// (must match the algorithm used when signing after PK_OK).
    Agent {
        blob_index: usize,
        alg: String,
    },
    FileKey,
}

/// Auth method ladder state after a FAILURE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthStep {
    Agent,
    FileKey,
    Password,
    KeyboardInteractive,
    Exhausted,
}

impl Client {
    pub fn do_auth<S>(&mut self, stream: &mut S, digest: &Digest) -> SshResult<()>
    where
        S: Read + Write,
    {
        info!("Auth start");
        let mut data = Data::new();
        data.put_u8(ssh_transport_code::SERVICE_REQUEST)
            .put_str(ssh_str::SSH_USERAUTH);
        data.pack(self).write_stream(stream)?;

        let mut step = self.initial_auth_step();
        let mut agent_blob_index = 0usize;
        let mut agent_alg_index = 0usize;
        let mut pending = PubkeyPending::None;
        let mut started = false;

        loop {
            let mut data = Data::unpack(SecPacket::from_stream(stream, self)?)?;
            let message_code = data.get_u8();
            match message_code {
                ssh_transport_code::SERVICE_ACCEPT => {
                    started = true;
                    self.send_auth_step(
                        stream,
                        step,
                        agent_blob_index,
                        agent_alg_index,
                        &mut pending,
                    )?;
                }
                ssh_user_auth_code::FAILURE => {
                    if !started {
                        return Err(SshError::AuthError);
                    }
                    // Advance past the method that just failed.
                    match pending {
                        PubkeyPending::Agent { blob_index, alg } => {
                            pending = PubkeyPending::None;
                            let next = self.advance_agent_attempt(blob_index, &alg);
                            agent_blob_index = next.0;
                            agent_alg_index = next.1;
                            if agent_blob_index < self.config.auth.agent_blobs.len() {
                                step = AuthStep::Agent;
                            } else {
                                step = self.next_after_agent();
                            }
                        }
                        PubkeyPending::FileKey => {
                            pending = PubkeyPending::None;
                            step = self.next_after_file_key();
                        }
                        PubkeyPending::None => {
                            step = match step {
                                AuthStep::Password => self.next_after_password(),
                                AuthStep::KeyboardInteractive => AuthStep::Exhausted,
                                AuthStep::Agent => {
                                    // Rejected before PK_OK; try next identity/algorithm.
                                    let blob = self.config.auth.agent_blobs.get(agent_blob_index);
                                    if let Some(blob) = blob {
                                        let algs = agent_alg_candidates(blob);
                                        if agent_alg_index + 1 < algs.len() {
                                            agent_alg_index += 1;
                                            AuthStep::Agent
                                        } else {
                                            agent_blob_index = agent_blob_index.saturating_add(1);
                                            agent_alg_index = 0;
                                            if agent_blob_index < self.config.auth.agent_blobs.len()
                                            {
                                                AuthStep::Agent
                                            } else {
                                                self.next_after_agent()
                                            }
                                        }
                                    } else {
                                        self.next_after_agent()
                                    }
                                }
                                AuthStep::FileKey => self.next_after_file_key(),
                                AuthStep::Exhausted => AuthStep::Exhausted,
                            };
                        }
                    }
                    if step == AuthStep::Exhausted {
                        error!("user auth failure: no methods remaining");
                        return Err(SshError::AuthError);
                    }
                    info!("auth failure; trying next method ({:?})", step);
                    self.send_auth_step(
                        stream,
                        step,
                        agent_blob_index,
                        agent_alg_index,
                        &mut pending,
                    )?;
                }
                60 => {
                    // PK_OK or INFO_REQUEST depending on outstanding method.
                    match pending {
                        PubkeyPending::Agent {
                            blob_index,
                            ref alg,
                        } => {
                            match self.agent_public_key_signature(stream, digest, blob_index, alg) {
                                Ok(()) => {
                                    pending = PubkeyPending::None;
                                }
                                Err(error) => {
                                    // Agent could not produce a signature (flags refused,
                                    // user cancelled Pageant, etc.). Do not abort the whole
                                    // ladder — advance to the next alg / identity / method.
                                    info!(
                                        "agent sign failed for identity {blob_index} alg {alg}: {error}; trying next"
                                    );
                                    let next = self.advance_agent_attempt(blob_index, alg);
                                    agent_blob_index = next.0;
                                    agent_alg_index = next.1;
                                    pending = PubkeyPending::None;
                                    if agent_blob_index < self.config.auth.agent_blobs.len() {
                                        step = AuthStep::Agent;
                                    } else {
                                        step = self.next_after_agent();
                                    }
                                    if step == AuthStep::Exhausted {
                                        error!("user auth failure: no methods remaining");
                                        return Err(SshError::AuthError);
                                    }
                                    self.send_auth_step(
                                        stream,
                                        step,
                                        agent_blob_index,
                                        agent_alg_index,
                                        &mut pending,
                                    )?;
                                }
                            }
                        }
                        PubkeyPending::FileKey => {
                            self.public_key_signature(stream, digest)?;
                            pending = PubkeyPending::None;
                        }
                        PubkeyPending::None if step == AuthStep::KeyboardInteractive => {
                            self.keyboard_interactive_response(stream, data)?;
                        }
                        _ => {
                            // Treat as PK_OK for file key if we still have one staged
                            // (legacy servers); otherwise ignore.
                            if self.config.auth.key_pair.is_some() {
                                self.public_key_signature(stream, digest)?;
                            } else if step == AuthStep::KeyboardInteractive {
                                self.keyboard_interactive_response(stream, data)?;
                            }
                        }
                    }
                }
                ssh_user_auth_code::SUCCESS => {
                    info!("user auth successful.");
                    if let Compress::ZlibOpenSsh = self.negotiated.c_compress[0] {
                        let comp = compression::from(&Compress::ZlibOpenSsh);
                        self.compressor = comp;
                    }
                    return Ok(());
                }
                ssh_connection_code::GLOBAL_REQUEST => {
                    let mut data = Data::new();
                    data.put_u8(ssh_connection_code::REQUEST_FAILURE);
                    data.pack(self).write_stream(stream)?;
                }
                ssh_user_auth_code::BANNER => {
                    let _msg = data.get_u8s();
                    let _lang = data.get_u8s();
                }
                _ => {}
            }
        }
    }

    fn initial_auth_step(&self) -> AuthStep {
        if !self.config.auth.agent_blobs.is_empty() {
            AuthStep::Agent
        } else if self.config.auth.key_pair.is_some() {
            AuthStep::FileKey
        } else if !self.config.auth.password.is_empty() {
            AuthStep::Password
        } else if self.config.auth.keyboard_interactive {
            AuthStep::KeyboardInteractive
        } else {
            // Last resort: password method (may be empty; CLI/GUI may re-prompt).
            AuthStep::Password
        }
    }

    fn next_after_agent(&self) -> AuthStep {
        if self.config.auth.key_pair.is_some() {
            AuthStep::FileKey
        } else if !self.config.auth.password.is_empty() {
            AuthStep::Password
        } else if self.config.auth.keyboard_interactive {
            AuthStep::KeyboardInteractive
        } else {
            // Last resort: try password method (may prompt externally via empty)
            AuthStep::Password
        }
    }

    fn next_after_file_key(&self) -> AuthStep {
        if !self.config.auth.password.is_empty() {
            AuthStep::Password
        } else if self.config.auth.keyboard_interactive {
            AuthStep::KeyboardInteractive
        } else {
            AuthStep::Password
        }
    }

    fn next_after_password(&self) -> AuthStep {
        if self.config.auth.keyboard_interactive {
            AuthStep::KeyboardInteractive
        } else {
            AuthStep::Exhausted
        }
    }

    fn send_auth_step<S>(
        &mut self,
        stream: &mut S,
        step: AuthStep,
        agent_blob_index: usize,
        agent_alg_index: usize,
        pending: &mut PubkeyPending,
    ) -> SshResult<()>
    where
        S: Write,
    {
        match step {
            AuthStep::Agent => {
                let blob = self
                    .config
                    .auth
                    .agent_blobs
                    .get(agent_blob_index)
                    .ok_or(SshError::AuthError)?;
                let algs = agent_alg_candidates(blob);
                let alg = algs
                    .get(agent_alg_index)
                    .cloned()
                    .ok_or(SshError::AuthError)?;
                *pending = PubkeyPending::Agent {
                    blob_index: agent_blob_index,
                    alg: alg.clone(),
                };
                self.agent_public_key_query(stream, agent_blob_index, &alg)
            }
            AuthStep::FileKey => {
                *pending = PubkeyPending::FileKey;
                self.public_key_authentication(stream)
            }
            AuthStep::Password => {
                *pending = PubkeyPending::None;
                self.password_authentication(stream)
            }
            AuthStep::KeyboardInteractive => {
                *pending = PubkeyPending::None;
                self.keyboard_interactive_request(stream)
            }
            AuthStep::Exhausted => Err(SshError::AuthError),
        }
    }

    /// After an agent attempt fails, return the next `(blob_index, alg_index)`.
    fn advance_agent_attempt(&self, blob_index: usize, alg: &str) -> (usize, usize) {
        let Some(blob) = self.config.auth.agent_blobs.get(blob_index) else {
            return (self.config.auth.agent_blobs.len(), 0);
        };
        let algs = agent_alg_candidates(blob);
        if let Some(i) = algs.iter().position(|a| a == alg) {
            if i + 1 < algs.len() {
                return (blob_index, i + 1);
            }
        }
        (blob_index + 1, 0)
    }

    fn password_authentication<S>(&mut self, stream: &mut S) -> SshResult<()>
    where
        S: Write,
    {
        info!("password authentication.");
        let mut data = Data::new();
        data.put_u8(ssh_user_auth_code::REQUEST)
            .put_str(self.config.auth.username.as_str())
            .put_str(ssh_str::SSH_CONNECTION)
            .put_str(ssh_str::PASSWORD)
            .put_u8(false as u8)
            .put_str(self.config.auth.password.as_str());

        data.pack(self).write_stream(stream)
    }

    fn public_key_authentication<S>(&mut self, stream: &mut S) -> SshResult<()>
    where
        S: Write,
    {
        let data = {
            let pubkey_alg = &self
                .config
                .auth
                .key_pair
                .as_ref()
                .unwrap()
                .authentication_algorithm();
            info!(
                "public key authentication. algorithm: {}",
                pubkey_alg.as_ref()
            );
            let mut data = Data::new();
            data.put_u8(ssh_user_auth_code::REQUEST)
                .put_str(self.config.auth.username.as_str())
                .put_str(ssh_str::SSH_CONNECTION)
                .put_str(ssh_str::PUBLIC_KEY)
                .put_u8(false as u8)
                .put_str(pubkey_alg.as_ref())
                .put_u8s(
                    &self
                        .config
                        .auth
                        .key_pair
                        .as_ref()
                        .unwrap()
                        .get_blob(pubkey_alg),
                );
            data
        };
        data.pack(self).write_stream(stream)
    }

    fn agent_public_key_query<S>(
        &mut self,
        stream: &mut S,
        index: usize,
        alg: &str,
    ) -> SshResult<()>
    where
        S: Write,
    {
        let blob = self
            .config
            .auth
            .agent_blobs
            .get(index)
            .ok_or(SshError::AuthError)?
            .clone();
        info!("agent public key authentication. algorithm: {alg} (identity {index})");
        let mut data = Data::new();
        data.put_u8(ssh_user_auth_code::REQUEST)
            .put_str(self.config.auth.username.as_str())
            .put_str(ssh_str::SSH_CONNECTION)
            .put_str(ssh_str::PUBLIC_KEY)
            .put_u8(false as u8)
            .put_str(alg)
            .put_u8s(&blob);
        data.pack(self).write_stream(stream)
    }

    fn agent_public_key_signature<S>(
        &mut self,
        stream: &mut S,
        digest: &Digest,
        index: usize,
        alg: &str,
    ) -> SshResult<()>
    where
        S: Write,
    {
        let blob = self
            .config
            .auth
            .agent_blobs
            .get(index)
            .ok_or(SshError::AuthError)?
            .clone();
        info!("agent public key signature. algorithm: {alg}");

        let mut request = Data::new();
        request
            .put_u8(ssh_user_auth_code::REQUEST)
            .put_str(self.config.auth.username.as_str())
            .put_str(ssh_str::SSH_CONNECTION)
            .put_str(ssh_str::PUBLIC_KEY)
            .put_u8(true as u8)
            .put_str(alg)
            .put_u8s(&blob);

        // Data to sign: string(session_id) || USERAUTH_REQUEST fields
        // (algorithm string above is part of the signed payload).
        let session_id = hash::digest(
            digest.hash_ctx.as_bytes().as_slice(),
            digest.key_exchange.as_ref().unwrap().get_hash_type(),
        );
        let mut to_sign = Data::new();
        to_sign.put_u8s(session_id.as_slice());
        to_sign.extend_from_slice(request.as_slice());

        let flags = agent_sign_flags(alg);
        let signature = crate::auth_hooks::agent_sign(&blob, to_sign.as_slice(), flags)
            .map_err(|e| SshError::GeneralError(format!("agent sign failed: {e}")))?;
        request.put_u8s(&signature);
        request.pack(self).write_stream(stream)
    }

    pub(crate) fn public_key_signature<S>(
        &mut self,
        stream: &mut S,
        digest: &Digest,
    ) -> SshResult<()>
    where
        S: Write,
    {
        let data = {
            let pubkey_alg = &self
                .config
                .auth
                .key_pair
                .as_ref()
                .unwrap()
                .authentication_algorithm();

            let mut data = Data::new();
            data.put_u8(ssh_user_auth_code::REQUEST)
                .put_str(self.config.auth.username.as_str())
                .put_str(ssh_str::SSH_CONNECTION)
                .put_str(ssh_str::PUBLIC_KEY)
                .put_u8(true as u8)
                .put_str(pubkey_alg.as_ref())
                .put_u8s(
                    &self
                        .config
                        .auth
                        .key_pair
                        .as_ref()
                        .unwrap()
                        .get_blob(pubkey_alg),
                );
            let signature = self.config.auth.key_pair.as_ref().unwrap().signature(
                data.as_slice(),
                digest.hash_ctx.clone(),
                digest.key_exchange.as_ref().unwrap().get_hash_type(),
                pubkey_alg,
            )?;
            data.put_u8s(&signature);
            data
        };
        data.pack(self).write_stream(stream)
    }

    fn keyboard_interactive_request<S>(&mut self, stream: &mut S) -> SshResult<()>
    where
        S: Write,
    {
        info!("keyboard-interactive authentication.");
        let mut data = Data::new();
        data.put_u8(ssh_user_auth_code::REQUEST)
            .put_str(self.config.auth.username.as_str())
            .put_str(ssh_str::SSH_CONNECTION)
            .put_str(ssh_str::KEYBOARD_INTERACTIVE)
            .put_str("") // language tag
            .put_str(""); // submethods
        data.pack(self).write_stream(stream)
    }

    fn keyboard_interactive_response<S>(&mut self, stream: &mut S, mut data: Data) -> SshResult<()>
    where
        S: Write,
    {
        // SSH_MSG_USERAUTH_INFO_REQUEST
        let name = String::from_utf8_lossy(&data.get_u8s()).into_owned();
        let instruction = String::from_utf8_lossy(&data.get_u8s()).into_owned();
        let _lang = data.get_u8s();
        let num_prompts = data.get_u32() as usize;
        let mut prompts = Vec::with_capacity(num_prompts);
        for _ in 0..num_prompts {
            let prompt = String::from_utf8_lossy(&data.get_u8s()).into_owned();
            let echo = data.get_u8() != 0;
            prompts.push((prompt, echo));
        }
        info!(
            "keyboard-interactive info request: {} prompt(s)",
            prompts.len()
        );
        let answers = crate::auth_hooks::kbd_prompt(name, instruction, prompts).map_err(|e| {
            error!("keyboard-interactive prompt failed: {e}");
            SshError::AuthError
        })?;
        let mut response = Data::new();
        response
            .put_u8(ssh_user_auth_code::INFO_RESPONSE)
            .put_u32(answers.len() as u32);
        for answer in answers {
            response.put_str(&answer);
        }
        response.pack(self).write_stream(stream)
    }
}

fn algorithm_from_blob(blob: &[u8]) -> Option<String> {
    if blob.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize;
    if blob.len() < 4 + len {
        return None;
    }
    Some(String::from_utf8_lossy(&blob[4..4 + len]).into_owned())
}

/// Publickey algorithm names to try for an agent identity (in preference order).
///
/// RSA blobs try modern `rsa-sha2-*` first, then classic `ssh-rsa`, so the
/// algorithm string in USERAUTH matches a single agent sign flag.
fn agent_alg_candidates(blob: &[u8]) -> Vec<String> {
    let raw = algorithm_from_blob(blob).unwrap_or_else(|| "ssh-ed25519".into());
    match raw.as_str() {
        "ssh-rsa" | "rsa-sha2-256" | "rsa-sha2-512" => vec![
            "rsa-sha2-256".into(),
            "rsa-sha2-512".into(),
            "ssh-rsa".into(),
        ],
        "ssh-rsa-cert-v01@openssh.com"
        | "rsa-sha2-256-cert-v01@openssh.com"
        | "rsa-sha2-512-cert-v01@openssh.com" => vec![
            "rsa-sha2-256-cert-v01@openssh.com".into(),
            "rsa-sha2-512-cert-v01@openssh.com".into(),
            "ssh-rsa-cert-v01@openssh.com".into(),
        ],
        other => vec![other.to_owned()],
    }
}

/// OpenSSH agent sign flags for a chosen publickey algorithm (exactly one).
fn agent_sign_flags(alg: &str) -> u32 {
    // SSH_AGENT_RSA_SHA2_256 = 2, SSH_AGENT_RSA_SHA2_512 = 4
    if alg.contains("rsa-sha2-256") {
        2
    } else if alg.contains("rsa-sha2-512") {
        4
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsa_blob_tries_sha2_then_classic() {
        let mut blob = Vec::new();
        let alg = b"ssh-rsa";
        blob.extend_from_slice(&(alg.len() as u32).to_be_bytes());
        blob.extend_from_slice(alg);
        blob.extend_from_slice(&0u32.to_be_bytes()); // dummy mpint-ish
        let cands = agent_alg_candidates(&blob);
        assert_eq!(
            cands,
            vec![
                "rsa-sha2-256".to_string(),
                "rsa-sha2-512".to_string(),
                "ssh-rsa".to_string()
            ]
        );
        assert_eq!(agent_sign_flags("rsa-sha2-256"), 2);
        assert_eq!(agent_sign_flags("rsa-sha2-512"), 4);
        assert_eq!(agent_sign_flags("ssh-rsa"), 0);
    }

    #[test]
    fn ed25519_blob_single_alg() {
        let mut blob = Vec::new();
        let alg = b"ssh-ed25519";
        blob.extend_from_slice(&(alg.len() as u32).to_be_bytes());
        blob.extend_from_slice(alg);
        blob.extend_from_slice(&[0u8; 32]);
        assert_eq!(agent_alg_candidates(&blob), vec!["ssh-ed25519".to_string()]);
        assert_eq!(agent_sign_flags("ssh-ed25519"), 0);
    }
}
