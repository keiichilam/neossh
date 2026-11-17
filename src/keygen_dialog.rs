use neo_ssh::keygen::Generated;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use zeroize::{Zeroize, Zeroizing};
static BUSY: AtomicBool = AtomicBool::new(false);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
pub struct Dialog {
    path: String,
    job: Option<mpsc::Receiver<Result<Generated, String>>>,
    saved: Option<Generated>,
    error: Option<String>,
    protect: bool,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
}
impl Default for Dialog {
    fn default() -> Self {
        Self {
            path: String::new(),
            job: None,
            saved: None,
            error: None,
            protect: true,
            password: Zeroizing::new(String::new()),
            confirmation: Zeroizing::new(String::new()),
        }
    }
}
impl Dialog {
    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<PathBuf>) {
        if let Some(result) = self.job.as_ref().and_then(|job| match job.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Key generator ended unexpectedly.".into())),
        }) {
            self.job = None;
            match result {
                Ok(saved) => self.saved = Some(saved),
                Err(error) => self.error = Some(error),
            }
        }
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Generate SSH key").open(&mut open).default_width(520.0).show(ctx, |ui| {
            ui.label("Ed25519 · OpenSSH format");
            ui.label("Private-file access is restricted to your account. Existing key files are preserved.");
            ui.add_enabled_ui(self.job.is_none() && self.saved.is_none(), |ui| {
                ui.label("Private-key file (parent folder must exist)");
                ui.text_edit_singleline(&mut self.path);
                if ui.checkbox(&mut self.protect, "Protect private key with a passphrase").changed() { self.password.zeroize(); self.confirmation.zeroize(); }
                if self.protect {
                    ui.label("New passphrase"); ui.add(egui::TextEdit::singleline(&mut *self.password).password(true));
                    ui.label("Confirm passphrase"); ui.add(egui::TextEdit::singleline(&mut *self.confirmation).password(true));
                    if self.password.as_str() != self.confirmation.as_str() { ui.label("Passphrases must match."); }
                } else { ui.label("The private key will be saved without a passphrase."); }
                if ui.button("Choose key location").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name("id_ed25519").save_file() {
                        if let Some(path) = path.to_str() { self.path = path.into(); } else { self.error = Some("Choose a Unicode key path.".into()); }
                    }
                }
                ui.label(format!("Private: {:?}\nPublic: {:?}", self.path, format!("{}.pub", self.path)));
                if ui.add_enabled(PathBuf::from(&self.path).is_absolute() && (!self.protect || !self.password.is_empty() && self.password.len() <= 1024 && self.password.as_str() == self.confirmation.as_str()), egui::Button::new("Generate and save keypair")).clicked() {
                    if BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                        self.error = Some("Another keypair is still being saved.".into());
                    } else {
                        let guard = Guard;
                        let path = PathBuf::from(&self.path);
                        let password = if self.protect { Some(std::mem::take(&mut self.password)) } else { None };
                        self.confirmation.zeroize();
                        let (sender, result) = mpsc::sync_channel(1);
                        match std::thread::Builder::new().name("key-generator".into()).spawn(move || { let _guard = guard; let _ = sender.send(neo_ssh::keygen::generate_with_passphrase(&path, password.as_ref().map(|password| password.as_str()))); }) {
                            Ok(_) => { self.job = Some(result); self.error = None; }, Err(error) => self.error = Some(error.to_string())
                        }
                    }
                }
            });
            if self.job.is_some() { ui.spinner(); ui.label("Saving keypair…"); }
            if let Some(saved) = &self.saved {
                ui.label(format!("Saved private key: {}", saved.private_path.display()));
                ui.monospace(&saved.fingerprint);
                if let Some(warning) = &saved.warning { ui.label(warning); }
                ui.horizontal(|ui| {
                    if ui.button("Copy public key").clicked() { ui.output_mut(|o| o.copied_text = saved.public_key.clone()); }
                    if ui.button("Use key for this connection").clicked() { chosen = Some(saved.private_path.clone()); }
                });
            }
            if let Some(error) = &self.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
        });
        if chosen.is_some() {
            open = false;
        }
        (open || self.job.is_some(), chosen)
    }
}
