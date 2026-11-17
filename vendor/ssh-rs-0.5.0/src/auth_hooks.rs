//! Application hooks for agent signing and keyboard-interactive prompts.
//!
//! Thread-local so concurrent connects do not share callbacks.
//! Callbacks only need `Send` (invoked on the connecting worker thread).

use std::cell::RefCell;

/// Sign SSH userauth data with an agent key blob.
/// Args: (key_blob, data_to_sign, agent_flags) -> signature blob.
///
/// `agent_flags` are OpenSSH agent sign flags (e.g. `SSH_AGENT_RSA_SHA2_256 = 2`,
/// `SSH_AGENT_RSA_SHA2_512 = 4`). Must match the publickey algorithm string
/// placed in the USERAUTH request that produced `data_to_sign`.
type AgentSigner = Box<dyn Fn(&[u8], &[u8], u32) -> Result<Vec<u8>, String> + Send>;

/// Answer keyboard-interactive prompts.
/// Args: (name, instruction, prompts: Vec<(prompt, echo)>) -> answers
type KbdPrompter =
    Box<dyn Fn(String, String, Vec<(String, bool)>) -> Result<Vec<String>, String> + Send>;

thread_local! {
    static AGENT_SIGNER: RefCell<Option<AgentSigner>> = const { RefCell::new(None) };
    static KBD_PROMPTER: RefCell<Option<KbdPrompter>> = const { RefCell::new(None) };
}

pub fn set_agent_signer(
    signer: Option<Box<dyn Fn(&[u8], &[u8], u32) -> Result<Vec<u8>, String> + Send>>,
) {
    AGENT_SIGNER.with(|cell| {
        *cell.borrow_mut() = signer;
    });
}

pub fn set_kbd_prompter(
    prompter: Option<
        Box<dyn Fn(String, String, Vec<(String, bool)>) -> Result<Vec<String>, String> + Send>,
    >,
) {
    KBD_PROMPTER.with(|cell| {
        *cell.borrow_mut() = prompter;
    });
}

pub(crate) fn agent_sign(key_blob: &[u8], data: &[u8], flags: u32) -> Result<Vec<u8>, String> {
    AGENT_SIGNER.with(|cell| {
        let guard = cell.borrow();
        match guard.as_ref() {
            Some(signer) => signer(key_blob, data, flags),
            None => Err("No SSH agent signer registered.".into()),
        }
    })
}

pub(crate) fn kbd_prompt(
    name: String,
    instruction: String,
    prompts: Vec<(String, bool)>,
) -> Result<Vec<String>, String> {
    KBD_PROMPTER.with(|cell| {
        let guard = cell.borrow();
        match guard.as_ref() {
            Some(prompter) => prompter(name, instruction, prompts),
            None => Err("No keyboard-interactive prompter registered.".into()),
        }
    })
}
