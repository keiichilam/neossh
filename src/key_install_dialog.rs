use neo_ssh::{
    key_install::{Key, Task},
    session::Session,
};

#[derive(Default)]
pub struct Dialog {
    key: Option<Key>,
    path: String,
    task: Option<Task>,
    result: Option<Result<(), String>>,
}

impl Dialog {
    fn change_path(&mut self, path: String) {
        self.path = path;
        self.key = None;
        self.result = None;
    }

    fn load_key(&mut self) {
        self.result = None;
        match Key::read(std::path::Path::new(&self.path)) {
            Ok(key) => self.key = Some(key),
            Err(error) => {
                self.key = None;
                self.result = Some(Err(error));
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: Option<&Session>,
        connected: bool,
        target: &str,
    ) -> bool {
        if let Some(result) = self.task.as_ref().and_then(Task::poll) {
            self.task = None;
            self.result = Some(result);
        }
        let mut open = true;
        egui::Window::new("Install public key")
            .collapsible(false).resizable(false).default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0,0.0])
            .show(ctx, |ui| {
                ui.label(format!("Connected account: {target}"));
                ui.label("Add this key to ~/.ssh/authorized_keys on this POSIX host. Existing entries are preserved. Folder permissions become 700 and file permissions become 600.");
                ui.add_enabled_ui(self.task.is_none(), |ui| {
                    ui.label("Public key path");
                    if ui.text_edit_singleline(&mut self.path).changed() {
                        self.key = None;
                        self.result = None;
                    }
                    if ui.add_enabled(!self.path.is_empty(), egui::Button::new("Load public key")).clicked() {
                        self.load_key();
                    }
                });
                if ui.add_enabled(self.task.is_none(), egui::Button::new("Choose public key (.pub)")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Public key", &["pub"]).pick_file() {
                        self.change_path(path.display().to_string());
                        self.load_key();
                    }
                }
                if let Some(key) = &self.key { ui.monospace(&key.fingerprint); }
                if self.task.is_some() { ui.spinner(); ui.label("Installing public key…"); }
                if let Some(result) = &self.result {
                    match result {
                        Ok(()) => { ui.label("Public key installed. Reconnect with its private key to verify access."); },
                        Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error); }
                    }
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(connected && self.key.is_some() && self.task.is_none(), egui::Button::new("Install on this account")).clicked() {
                        if let (Some(session),Some(key))=(session,self.key.take()) {
                            self.result = None;
                            match session.install_public_key(key) { Ok(task)=>self.task=Some(task),Err(error)=>self.result=Some(Err(error)) }
                        }
                    }
                    if ui.button(if self.task.is_some() {"Stop waiting"} else {"Close"}).clicked() {open=false;}
                });
                if self.task.is_some() { ui.small("Stopping or disconnecting may leave the key installed; verify the remote file before retrying."); }
            });
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_or_reloading_a_bad_path_cannot_install_the_previous_key() {
        let dir = tempfile::tempdir().unwrap();
        let private =
            ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519)
                .unwrap();
        let public_path = dir.path().join("key.pub");
        std::fs::write(&public_path, private.public_key().to_openssh().unwrap()).unwrap();
        let fingerprint = private
            .public_key()
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string();
        let mut dialog = Dialog::default();
        dialog.change_path(public_path.display().to_string());
        dialog.load_key();
        assert_eq!(dialog.key.as_ref().unwrap().fingerprint, fingerprint);
        dialog.change_path(dir.path().join("missing.pub").display().to_string());
        assert!(dialog.key.is_none());
        dialog.load_key();
        assert!(dialog.key.is_none());
        assert!(matches!(dialog.result, Some(Err(_))));
        dialog.change_path(public_path.display().to_string());
        dialog.load_key();
        std::fs::write(&public_path, "not a public key").unwrap();
        dialog.load_key();
        assert!(dialog.key.is_none());
        assert!(matches!(dialog.result, Some(Err(_))));
        assert!(dialog.task.is_none());
    }
}
