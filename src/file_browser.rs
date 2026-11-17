use neo_ssh::{
    files::{Action, ActionTask, DirectoryTask, Listing, RemoteFingerprint},
    remote_edit::EditSession,
    session::Session,
};

pub struct Browser {
    path: String,
    listing: Option<Listing>,
    task: Option<DirectoryTask>,
    error: Option<String>,
    selected: Option<usize>,
    select_name: Option<String>,
    reveal_selection: bool,
    editor: Option<Editor>,
    action_task: Option<ActionTask>,
    downloading: bool,
    uploading: bool,
    use_scp: bool,
    cancelling: bool,
    notice: Option<String>,
    download_destination: String,
    upload_queue: std::collections::VecDeque<std::path::PathBuf>,
    upload_destination: Option<String>,
    completed_upload: Option<String>,
    insert_path: Option<String>,
    clipboard_job: Option<(crate::clipboard_image::Job, String)>,
    clipboard_file: Option<tempfile::NamedTempFile>,
    clipboard_preview: Option<egui::TextureHandle>,
    clipboard_dimensions: String,
    local_browser: Option<crate::local_browser::Browser>,
    /// Remote files open in a local editor. They live as long as this window:
    /// dropping the Browser cancels their transfers, and copies with unsynced
    /// edits are kept on disk.
    edits: Vec<EditSession>,
    confirm_close: bool,
}

/// Window frame and group margins kept free below the last widget.
const FRAME_ALLOWANCE: f32 = 24.0;
/// Room the file rows give up while a review is open: about a clipboard-image review.
const REVIEW_ROOM: f32 = 440.0;

/// Height between the next widget and the bottom of the screen. The Remote files
/// window does not scroll, so anything placed lower is cut off (for example on
/// 1920x1080 at 150% scaling, which leaves about 670 points).
fn room_below(ui: &egui::Ui) -> f32 {
    ui.ctx().screen_rect().bottom() - ui.cursor().top()
}

/// What an open review's buttons confirm, decided while its details are shown.
enum Review {
    Edit(RemoteFingerprint, Result<String, String>),
    Action(Result<Action, String>),
}

fn remote_rows(
    ui: &mut egui::Ui,
    listing: &Listing,
    selected: &mut Option<usize>,
    enabled: bool,
    height: f32,
    reveal: bool,
) -> (Option<String>, usize) {
    let name_width = (ui.available_width() - 200.0 - 2.0 * ui.spacing().item_spacing.x).max(80.0);
    ui.horizontal(|ui| {
        for (width, text) in [
            (name_width, "Name"),
            (100.0, "Bytes"),
            (100.0, "Permissions"),
        ] {
            ui.add_sized(
                [width, 22.0],
                egui::Label::new(egui::RichText::new(text).strong()).truncate(),
            );
        }
    });
    let mut navigate = None;
    let mut rendered = 0;
    let mut scroll = egui::ScrollArea::vertical()
        .id_source(("remote-file-list", &listing.path))
        .max_height(height);
    if let Some(index) = selected.filter(|_| reveal) {
        scroll = scroll.vertical_scroll_offset(index as f32 * (22.0 + ui.spacing().item_spacing.y));
    }
    scroll.show_rows(ui, 22.0, listing.entries.len(), |ui, rows| {
        for index in rows {
            rendered += 1;
            let entry = &listing.entries[index];
            let name = format!(
                "{}{}",
                entry.name.escape_debug(),
                if entry.is_directory() { "/" } else { "" }
            );
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    let response = ui
                        .add_enabled_ui(enabled, |ui| {
                            ui.add_sized(
                                [name_width, 22.0],
                                egui::Button::new(&name)
                                    .truncate()
                                    .frame(false)
                                    .selected(*selected == Some(index)),
                            )
                        })
                        .inner
                        .on_hover_text(&name);
                    if response.clicked() {
                        *selected = Some(index);
                    }
                    if response.double_clicked() && entry.is_directory() {
                        navigate = Some(format!(
                            "{}/{}",
                            listing.path.trim_end_matches('/'),
                            entry.name
                        ));
                    }
                    let size = entry
                        .size
                        .map_or_else(|| "—".into(), |value| value.to_string());
                    ui.add_sized([100.0, 22.0], egui::Label::new(&size).truncate())
                        .on_hover_text(size);
                    let mode = entry
                        .permissions
                        .map_or_else(|| "—".into(), |value| format!("{:04o}", value & 0o7777));
                    ui.add_sized([100.0, 22.0], egui::Label::new(mode).truncate());
                });
            });
        }
    });
    (navigate, rendered)
}
impl Browser {
    pub fn new(session: &Session) -> Self {
        Self::open(session, ".".into(), None)
    }
    pub fn locate(session: &Session, path: &str) -> Result<Self, String> {
        let (parent, name) = locate_target(path)?;
        Ok(Self::open(session, parent, name))
    }
    fn open(session: &Session, path: String, select_name: Option<String>) -> Self {
        let mut browser = Self::blank(path, select_name);
        browser.load(session);
        browser
    }
    fn blank(path: String, select_name: Option<String>) -> Self {
        Self {
            path,
            listing: None,
            task: None,
            error: None,
            selected: None,
            select_name,
            reveal_selection: false,
            editor: None,
            action_task: None,
            downloading: false,
            uploading: false,
            use_scp: false,
            cancelling: false,
            notice: None,
            download_destination: String::new(),
            upload_queue: Default::default(),
            upload_destination: None,
            completed_upload: None,
            insert_path: None,
            clipboard_job: None,
            clipboard_file: None,
            clipboard_preview: None,
            clipboard_dimensions: String::new(),
            local_browser: None,
            edits: Vec::new(),
            confirm_close: false,
        }
    }
    pub fn queue_uploads(&mut self, paths: Vec<std::path::PathBuf>) -> Result<(), String> {
        enqueue_uploads(&mut self.upload_queue, paths)
    }
    pub fn take_terminal_insert(&mut self) -> Option<String> {
        self.insert_path.take()
    }
    /// Ctrl+Shift+I / palette (Ctrl+P in ssh4, ssh4gui): capture the clipboard image; its upload
    /// is then reviewed into the current remote folder like any other upload.
    pub fn capture_clipboard_image(&mut self) -> Result<(), String> {
        if self.clipboard_job.is_some() {
            return Ok(());
        }
        if self.editor.is_some() || self.action_task.is_some() {
            return Err("Finish the open file action before uploading a clipboard image.".into());
        }
        let job = crate::clipboard_image::Job::start()?;
        self.clipboard_job = Some((job, self.path.clone()));
        self.error = None;
        Ok(())
    }
    /// Hand edit sessions to the tab when the window closes with
    /// "Keep syncing in background" (or when another window replaces this one).
    pub fn take_edits(&mut self) -> Vec<neo_ssh::remote_edit::EditSession> {
        std::mem::take(&mut self.edits)
    }
    /// Show edit sessions that were syncing in the background in this window.
    pub fn adopt_edits(&mut self, edits: Vec<neo_ssh::remote_edit::EditSession>) {
        self.edits.extend(edits);
    }
    /// Advance edit sessions: finish their transfers and upload settled saves.
    /// `show` calls this; an owner may also call it for windows not shown.
    pub fn poll_edits(&mut self, session: Option<&Session>, connected: bool) {
        let now = std::time::Instant::now();
        let mut submit = |action| match session {
            Some(session) => session.file_action(action),
            None => Err("The session has ended.".to_owned()),
        };
        for edit in &mut self.edits {
            edit.poll(now, connected && session.is_some(), &mut submit);
        }
    }
    fn load(&mut self, session: &Session) {
        match session.list_directory(self.path.clone()) {
            Ok(task) => {
                self.task = Some(task);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: Option<&Session>,
        connected: bool,
        title: &str,
    ) -> bool {
        self.poll_edits(session, connected);
        if !self.edits.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if let Some(result) = self.clipboard_job.as_ref().and_then(|(job, _)| job.poll()) {
            let (_, parent) = self.clipboard_job.take().unwrap();
            match result {
                Ok(capture) => {
                    self.clipboard_dimensions =
                        format!("PNG image: {} × {} pixels", capture.width, capture.height);
                    self.clipboard_preview = Some(ctx.load_texture(
                        "clipboard-preview",
                        capture.preview,
                        egui::TextureOptions::LINEAR,
                    ));
                    self.editor = Some(Editor {
                        kind: EditKind::Upload,
                        parent,
                        source: None,
                        name: "clipboard.png".into(),
                        local: capture.file.path().to_string_lossy().into_owned(),
                    });
                    self.clipboard_file = Some(capture.file);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(result) = self.action_task.as_ref().and_then(ActionTask::poll) {
            self.action_task = None;
            self.clipboard_file = None;
            self.clipboard_preview = None;
            match result {
                Ok(()) => {
                    if self.downloading {
                        self.notice = Some(format!(
                            "Saved download to {:?}.",
                            self.download_destination
                        ));
                    } else {
                        if self.uploading {
                            self.notice = Some("Upload complete.".into());
                            self.completed_upload = self.upload_destination.take();
                        }
                        if let (Some(session), Some(listing)) = (session, &self.listing) {
                            self.path = listing.path.clone();
                            self.load(session);
                        }
                    }
                }
                Err(error) => {
                    self.error = Some(if self.downloading {
                        error
                    } else {
                        format!(
                            "{error} Refresh to check the current server state before retrying."
                        )
                    })
                }
            }
            self.downloading = false;
            self.upload_destination = None;
            self.uploading = false;
            self.cancelling = false;
        }
        if let Some(result) = self.task.as_ref().and_then(DirectoryTask::poll) {
            self.task = None;
            match result {
                Ok(listing) => {
                    self.path = listing.path.clone();
                    if let Some(name) = self.select_name.take() {
                        self.selected = listing.entries.iter().position(|entry| entry.name == name);
                        self.reveal_selection = self.selected.is_some();
                        if self.selected.is_none() {
                            self.notice = Some(format!(
                                "The requested entry {name:?} was not found in this folder."
                            ));
                        }
                    } else {
                        self.selected = None;
                    }
                    self.listing = Some(listing);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        let mut open = true;
        let mut navigate = None;
        let mut close_now = false;
        let mut close_keep = false;
        egui::Window::new(format!("Remote files — {title}"))
            .id(egui::Id::new("remote-files"))
            .open(&mut open)
            .default_width(650.0)
            .show(ctx, |ui| {
                if !connected {
                    ui.label("Session disconnected. Reconnect to browse remote files.");
                }
                if self.confirm_close {
                    ui.group(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(245, 176, 140), format!("You are editing {} file(s). Keep syncing them in the background, or stop editing.", self.edits.len()));
                        for edit in self.edits.iter().filter(|edit| edit.keeps_copy()) {
                            ui.label(format!("{}: unsynced edits will be kept at {}", edit.name().escape_debug(), edit.local().display()));
                        }
                        ui.label("Stopping removes temporary copies without unsynced edits. Background edits appear as EDITING in the tab header; reopen Remote files to manage them.");
                        ui.horizontal(|ui| {
                            if ui.button("Close, keep syncing").clicked() { close_keep = true; }
                            if ui.button("Stop editing and close").clicked() { close_now = true; }
                            if ui.button("Keep editing").clicked() { self.confirm_close = false; }
                        });
                    });
                }
                ui.add_enabled(self.action_task.is_none() && self.editor.is_none() && self.clipboard_job.is_none(), egui::Checkbox::new(&mut self.use_scp, "Use SCP for uploads"));
                if self.use_scp && self.listing.is_none() {
                    ui.label("Enter the remote folder below. SCP uploads do not require an SFTP listing.");
                    if ui.add_enabled(connected && self.task.is_none() && self.action_task.is_none() && self.editor.is_none() && self.clipboard_job.is_none(), egui::Button::new("Clipboard image via SCP")).clicked() {
                        match crate::clipboard_image::Job::start() { Ok(job) => { self.clipboard_job = Some((job, self.path.clone())); self.error = None; }, Err(error) => self.error = Some(error) }
                    }
                }
                if ui.add_enabled(connected && self.task.is_none() && self.action_task.is_none() && self.editor.is_none(), egui::Button::new("Upload with SCP…")).clicked() {
                    self.use_scp = true;
                    self.editor = Some(Editor { kind: EditKind::Upload, parent: self.path.clone(), source: None, name: String::new(), local: String::new() });
                }
                if ui.button(if self.local_browser.is_some() { "Hide local files" } else { "Browse local files" }).clicked() {
                    self.local_browser = if self.local_browser.is_some() { None } else { Some(crate::local_browser::Browser::new()) };
                }
                if ui.add_enabled(connected && self.task.is_none() && self.action_task.is_none() && self.editor.is_none(), egui::Button::new("Download with SCP...")).clicked() {
                    self.use_scp = true;
                    self.editor = Some(Editor { kind: EditKind::Download, parent: self.path.clone(), source: None, name: String::new(), local: String::new() });
                }
                let selected_uploads = self.local_browser.as_mut().map(|browser| browser.show(ui, connected)).unwrap_or_default();
                if !selected_uploads.is_empty() {
                    match self.queue_uploads(selected_uploads) {
                        Ok(()) => { if let Some(browser) = &mut self.local_browser { browser.clear_selection(); } },
                        Err(error) => self.error = Some(error),
                    }
                }
                if self.clipboard_job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner(); ui.label("Reading clipboard image…");
                        if ui.button("Cancel capture").clicked() { self.clipboard_job = None; }
                    });
                }
                if let Some(path) = &self.completed_upload {
                    ui.horizontal(|ui| {
                        ui.label(format!("Uploaded: {path:?}"));
                        if ui.button("Copy remote path").clicked() { ui.output_mut(|output| output.copied_text = path.clone()); }
                        let quoted = quote_terminal_path(path);
                        if ui.add_enabled(connected && quoted.is_ok(), egui::Button::new("Insert quoted path")).on_hover_text("Insert a POSIX shell-quoted path into this terminal without pressing Enter.").clicked() { self.insert_path = quoted.ok(); }
                    });
                }
                if !self.upload_queue.is_empty() {
                    ui.group(|ui| {
                        ui.label(format!("{} file(s) waiting for review", self.upload_queue.len()));
                        if let Some(path) = self.upload_queue.front() { ui.label(format!("Next: {path:?}")); }
                        ui.horizontal(|ui| {
                            let ready = connected && (self.use_scp || self.listing.is_some()) && self.task.is_none() && self.action_task.is_none() && self.editor.is_none() && self.clipboard_job.is_none();
                            if ui.add_enabled(ready, egui::Button::new("Review next upload")).clicked() {
                                if let Some(path) = self.upload_queue.pop_front() {
                                    let parent = if self.use_scp { self.path.clone() } else { self.listing.as_ref().unwrap().path.clone() };
                                    self.editor = Some(Editor { kind: EditKind::Upload, parent, source: None, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), local: path.to_string_lossy().into_owned() });
                                }
                            }
                            if ui.button("Clear waiting files").clicked() { self.upload_queue.clear(); }
                        });
                        ui.label("Choose a remote folder, then review each file. Closing this window discards waiting files.");
                    });
                }
                ui.add_enabled_ui(connected && self.task.is_none() && self.action_task.is_none() && self.editor.is_none() && self.clipboard_job.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Up").clicked() {
                            if let Some(listing) = &self.listing {
                                navigate =
                                    Some(format!("{}/..", listing.path.trim_end_matches('/')));
                            }
                        }
                        let path =
                            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(430.0));
                        if ui.button("Open / refresh").clicked()
                            || path.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            navigate = Some(self.path.clone());
                        }
                    });
                });
                if self.task.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading directory…");
                        if ui.button("Cancel").clicked() {
                            self.task = None;
                            self.error = Some("Directory request cancelled.".into());
                        }
                    });
                }
                if self.action_task.is_some() && !self.downloading && !self.uploading {
                    ui.spinner(); ui.label("Waiting for server confirmation… Closing this window will not undo a submitted action.");
                }
                if self.downloading || self.uploading {
                    if let Some(task) = &self.action_task {
                        if task.used_scp_fallback() { ui.label("SFTP is unavailable; transferring with SCP."); }
                        let bytes = task.bytes_transferred();
                        if let Some(total) = task.total_bytes() {
                            ui.add(egui::ProgressBar::new(if total == 0 { 0.0 } else { (bytes as f64 / total as f64).min(1.0) as f32 }).text(format!("{bytes} / {total} bytes")));
                        } else { ui.label(format!("Transferred {bytes} bytes")); }
                        if ui.add_enabled(!self.cancelling, egui::Button::new("Cancel transfer")).clicked() { task.cancel(); self.cancelling = true; }
                        if self.cancelling { ui.label("Cancelling transfer and waiting for cleanup…"); }
                    }
                }
                if let Some(notice) = &self.notice { ui.label(notice); }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if !self.edits.is_empty() {
                    ui.group(|ui| {
                        ui.strong("Editing — each save replaces the remote file");
                        let now = std::time::Instant::now();
                        let mut stop = None;
                        for (index, edit) in self.edits.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                let status = edit.status(now);
                                let (label, failed, detail) = (status.label(), status.is_error(), status.detail().map(str::to_owned));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(edit.name().escape_debug().to_string()).strong()).on_hover_text(format!("Remote: {}\nLocal: {}", edit.remote().escape_debug(), edit.local().display()));
                                    if failed { ui.colored_label(egui::Color32::LIGHT_RED, label); } else { ui.label(label); }
                                    if ui.add_enabled(edit.is_open(), egui::Button::new("Open again")).clicked() { edit.reopen(); }
                                    if ui.add_enabled(edit.is_open(), egui::Button::new("Show folder")).clicked() {
                                        if let Err(error) = edit.local().parent().map_or(Err("No folder.".to_owned()), neo_ssh::remote_edit::open_folder) { self.error = Some(error); }
                                    }
                                    if ui.button("Copy local path").clicked() { ui.output_mut(|output| output.copied_text = edit.local().display().to_string()); }
                                    if edit.can_retry() && ui.add_enabled(connected, egui::Button::new("Retry upload")).on_hover_text("Upload the local copy again if the server copy is still unchanged.").clicked() { edit.retry(now); }
                                    if ui.button("Stop editing").on_hover_text("Stop syncing. The temporary copy is removed unless it holds unsynced edits.").clicked() { stop = Some(index); }
                                });
                                if let Some(detail) = detail { ui.colored_label(egui::Color32::LIGHT_RED, detail); }
                                if let Some(note) = edit.note() { ui.label(note); }
                            });
                        }
                        if let Some(index) = stop { self.notice = Some(self.edits.remove(index).stop()); }
                    });
                }
                if let Some(listing) = &self.listing {
                    ui.label(format!(
                        "{} · {} entries",
                        listing.path,
                        listing.entries.len()
                    ));
                    ui.separator();
                    let enabled = connected && self.task.is_none() && self.editor.is_none() && self.clipboard_job.is_none() && self.action_task.is_none();
                    let list_height = {
                        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                        let preferred = if self.local_browser.is_some() { 230.0 } else { 420.0 };
                        // Keep the column header, the hint and up to two lines of buttons on
                        // screen; while a review is open, the (disabled) rows also make room for it.
                        let reviewing = self.editor.is_some();
                        let below = 5.0 * row + FRAME_ALLOWANCE + if reviewing { REVIEW_ROOM } else { 0.0 };
                        (room_below(ui) - below).clamp(if reviewing { 0.0 } else { 3.0 * row }, preferred)
                    };
                    let (folder, _) = remote_rows(ui, listing, &mut self.selected, enabled, list_height, std::mem::take(&mut self.reveal_selection));
                    if folder.is_some() { navigate = folder; }
                    ui.label("Double-click a folder to open it.");
                    ui.add_enabled_ui(connected && self.task.is_none() && self.action_task.is_none() && self.editor.is_none() && self.clipboard_job.is_none(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Clipboard image").clicked() {
                                match crate::clipboard_image::Job::start() { Ok(job) => { self.clipboard_job = Some((job, listing.path.clone())); self.error = None; }, Err(error) => self.error = Some(error) }
                            }
                            if ui.button("Upload…").clicked() { self.editor = Some(Editor { kind: EditKind::Upload, parent: listing.path.clone(), source: None, name: String::new(), local: String::new() }); }
                            if ui.button("New file").clicked() { self.editor = Some(Editor { kind: EditKind::CreateFile, parent: listing.path.clone(), source: None, name: String::new(), local: String::new() }); }
                            if ui.button("New folder").clicked() { self.editor = Some(Editor { kind: EditKind::Create, parent: listing.path.clone(), source: None, name: String::new(), local: String::new() }); }
                            if let Some(entry) = self.selected.and_then(|index| listing.entries.get(index)) {
                                if !entry.is_directory() && ui.button("Download…").clicked() { self.editor = Some(Editor { kind: EditKind::Download, parent: listing.path.clone(), source: Some((entry.name.clone(), false)), name: String::new(), local: String::new() }); }
                                if !entry.is_directory() {
                                    let fingerprint = RemoteFingerprint::of(entry);
                                    let editing = child_path(&listing.path, &entry.name).is_ok_and(|path| self.edits.iter().any(|edit| edit.remote() == path));
                                    let edit = ui.add_enabled(fingerprint.is_some() && !editing, egui::Button::new("Edit…"))
                                        .on_hover_text("Open in your default app; each save replaces the remote file.")
                                        .on_disabled_hover_text(if editing { "This file is already being edited." } else { "Only regular files with a reported size and modification time can be edited." });
                                    if let (true, Some(fingerprint)) = (edit.clicked(), fingerprint) { self.editor = Some(Editor { kind: EditKind::Edit(fingerprint), parent: listing.path.clone(), source: Some((entry.name.clone(), false)), name: String::new(), local: String::new() }); }
                                }
                                if entry.permissions.is_some_and(|mode| matches!(mode & 0o170000, 0o040000 | 0o100000)) && ui.button("Permissions...").clicked() { self.editor = Some(Editor { kind: EditKind::Permissions, parent: listing.path.clone(), source: Some((entry.name.clone(), entry.is_directory())), name: format!("{:04o}", entry.permissions.unwrap() & 0o7777), local: String::new() }); }
                                if ui.button("Rename…").clicked() { self.editor = Some(Editor { kind: EditKind::Rename, parent: listing.path.clone(), source: Some((entry.name.clone(), entry.is_directory())), name: entry.name.clone(), local: String::new() }); }
                                if ui.button("Delete…").clicked() { self.editor = Some(Editor { kind: EditKind::Delete, parent: listing.path.clone(), source: Some((entry.name.clone(), entry.is_directory())), name: String::new(), local: String::new() }); }
                            }
                        });
                    });
                }
                let mut dismiss = false;
                if let Some(editor) = &mut self.editor {
                    ui.separator();
                    // The window does not scroll: the review's details scroll in the room left
                    // on screen and its buttons stay below them, always visible.
                    let buttons = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                    let details_height = (room_below(ui) - buttons - FRAME_ALLOWANCE).max(2.0 * buttons);
                    ui.group(|ui| {
                    let review = egui::ScrollArea::vertical().id_source("remote-file-review").min_scrolled_height(details_height).max_height(details_height).show(ui, |ui| {
                        ui.strong(match editor.kind { EditKind::Permissions => "Change remote permissions", EditKind::CreateFile => "Create empty remote file", EditKind::Create => "Create remote folder", EditKind::Rename => "Rename or move remote entry", EditKind::Delete => "Delete remote entry", EditKind::Download => "Download remote file", EditKind::Upload => "Upload local file", EditKind::Edit(_) => "Edit remote file" });
                        if let EditKind::Edit(fingerprint) = editor.kind {
                            let remote = editor.source.as_ref().ok_or_else(|| "Select a file first.".to_owned()).and_then(|source| child_path(&editor.parent, &source.0));
                            match &remote {
                                Ok(path) => { ui.label(format!("Remote: {path:?}\nListed size: {} bytes", fingerprint.size)); }
                                Err(error) => { ui.label(error); }
                            }
                            ui.label("Downloads the file to a private temporary folder and opens it with your default app. File types that can run programs are not opened automatically.");
                            ui.colored_label(egui::Color32::from_rgb(245, 176, 140), "Each save in the local editor REPLACES the remote file. The saved copy is written next to it and renamed over it, keeping its permission bits (not setuid/setgid).");
                            ui.label("If the file changes on the server meanwhile, syncing stops and your edits stay in the local copy. Stop editing to remove the temporary copy.");
                            ui.label("If the server refuses SFTP, SCP is used automatically; replacing then needs a POSIX shell with stat and GNU mv.");
                            ui.checkbox(&mut self.use_scp, "Use SCP instead of SFTP");
                            return Review::Edit(fingerprint, remote);
                        }
                        if !matches!(editor.kind, EditKind::Delete) { ui.label(if matches!(editor.kind, EditKind::Download) { "Local file path" } else if matches!(editor.kind, EditKind::Permissions) { "Octal permissions (0000-7777)" } else if matches!(editor.kind, EditKind::Rename) { "Destination name or path (relative to current folder, or absolute)" } else { "New name" }); ui.text_edit_singleline(&mut editor.name); }
                        if matches!(editor.kind, EditKind::Download) && ui.button("Choose destination…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_title("Download to a new local file").save_file() { editor.name = path.to_string_lossy().into_owned(); }
                        }
                        if let Some(preview) = &self.clipboard_preview {
                            ui.label(&self.clipboard_dimensions);
                            ui.image(preview);
                        }
                        if matches!(editor.kind, EditKind::Upload) && self.clipboard_file.is_none() {
                            ui.label("Local source file"); ui.text_edit_singleline(&mut editor.local);
                            if ui.button("Choose source…").clicked() {
                                if let Some(path) = rfd::FileDialog::new().set_title("Choose file to upload").pick_file() {
                                    if editor.name.is_empty() { editor.name = path.file_name().unwrap_or_default().to_string_lossy().into_owned(); }
                                    editor.local = path.to_string_lossy().into_owned();
                                }
                            }
                        }
                        if matches!(editor.kind, EditKind::Download) && editor.source.is_none() {
                            ui.label("Remote filename in the current folder"); ui.text_edit_singleline(&mut editor.local);
                        }
                        if matches!(editor.kind, EditKind::Upload | EditKind::Download) {
                            ui.label("If the server refuses SFTP, this transfer uses SCP automatically.");
                            ui.checkbox(&mut self.use_scp, "Use SCP instead of SFTP");
                            if self.use_scp {
                                ui.label(if matches!(editor.kind, EditKind::Download) { "Requires a POSIX shell with scp. Downloads one regular file." } else { "Requires a POSIX shell with scp, mktemp and ln -T." });
                            }
                        }
                        let action = editor.action().map(|action| match action {
                            Action::Upload { local, remote } if self.use_scp => Action::UploadScp { local, remote },
                            Action::Download { local, remote } if self.use_scp => Action::DownloadScp { local, remote },
                            action => action,
                        });
                        match &action {
                            Ok(Action::SetPermissions { path, mode }) => { ui.label(format!("Path: {path:?}\nNew permissions: {mode:04o}")); ui.label("Applies to this entry only. Special permission bits are included; this does not change ownership."); }
                            Ok(Action::Upload { local, remote } | Action::UploadScp { local, remote }) => { if self.clipboard_file.is_some() { ui.label(format!("Clipboard PNG to: {remote:?}")); } else { ui.label(format!("Local: {local:?}\nRemote: {remote:?}")); } ui.label("Uploads create a new remote file with private permissions. Existing destinations are not replaced."); }
                            Ok(Action::Download { remote, local } | Action::DownloadScp { remote, local }) => { ui.label(format!("Remote: {remote:?}\nLocal: {local:?}")); ui.label("Choose a new filename. Existing local files are never replaced."); }
                            Ok(Action::CreateFile { path }) => { ui.label(format!("Create empty file: {path:?}")); ui.label("Uses private permissions (0600). An existing file will not be replaced."); }
                            Ok(Action::CreateDirectory { path }) => { ui.label(format!("Create: {path:?}")); }
                            Ok(Action::Rename { from, to }) => { ui.label(format!("From: {from:?}\nTo: {to:?}")); ui.label("An existing destination will not be replaced."); }
                            Ok(Action::RemoveFile { path }) => { ui.label(format!("Delete file: {path:?}")); ui.label("This cannot be undone here."); }
                            Ok(Action::RemoveDirectory { path }) => { ui.label(format!("Delete folder: {path:?}")); ui.label("Only empty folders can be deleted. This cannot be undone here."); }
                            Ok(Action::EditDownload { .. } | Action::EditReplace { .. }) => {}
                            Err(error) => { ui.label(error); }
                        }
                        Review::Action(action)
                    }).inner;
                    ui.horizontal(|ui| {
                        match review {
                            Review::Edit(fingerprint, remote) => {
                                if ui.add_enabled(connected && remote.is_ok(), egui::Button::new("Start editing")).clicked() {
                                    if let (Some(session), Ok(remote)) = (session, &remote) {
                                        match EditSession::start(remote, fingerprint, self.use_scp, |action| session.file_action(action)) {
                                            Ok(edit) => { self.notice = Some(format!("Editing {:?}. Save in your editor to replace the remote file.", edit.name())); self.edits.push(edit); self.error = None; dismiss = true; }
                                            Err(error) => self.error = Some(error),
                                        }
                                    }
                                }
                            }
                            Review::Action(action) => {
                                let confirm = ui.add_enabled(connected && action.is_ok(), egui::Button::new(match editor.kind { EditKind::Permissions => "Apply permissions", EditKind::CreateFile => "Create file", EditKind::Create => "Create folder", EditKind::Rename => "Confirm rename/move", EditKind::Delete => "Confirm delete", EditKind::Download => "Start download", EditKind::Upload => "Start upload", EditKind::Edit(_) => "Start editing" }));
                                if confirm.clicked() {
                                    if let (Some(session), Ok(action)) = (session, action) {
                                        let uploading = matches!(action, Action::Upload { .. } | Action::UploadScp { .. });
                                        let upload_destination = if let Action::Upload { remote, .. } | Action::UploadScp { remote, .. } = &action { Some(remote.clone()) } else { None };
                                        let downloading = matches!(action, Action::Download { .. } | Action::DownloadScp { .. });
                                        match session.file_action(action) { Ok(task) => { self.action_task = Some(task); self.downloading = downloading; self.uploading = uploading; self.upload_destination = upload_destination; self.cancelling = false; self.download_destination = editor.name.clone(); self.notice = None; self.error = None; dismiss = true; }, Err(error) => self.error = Some(error) }
                                    }
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() { dismiss = true; }
                    });
                    });
                }
                if dismiss { self.editor = None; self.clipboard_preview = None; if self.action_task.is_none() { self.clipboard_file = None; } }
            });
        if let (Some(path), Some(session)) = (navigate, session) {
            self.path = path;
            self.load(session);
        }
        if close_now {
            for edit in self.edits.drain(..) {
                edit.stop();
            }
            return false;
        }
        if close_keep {
            // The owner takes the remaining sessions with take_edits().
            self.confirm_close = false;
            return false;
        }
        if !open && !self.edits.is_empty() {
            // Closing drops this Browser and with it every edit session, so
            // ask first and show where unsynced edits will stay.
            self.confirm_close = true;
            open = true;
        }
        open
    }
}

fn locate_target(path: &str) -> Result<(String, Option<String>), String> {
    neo_ssh::remote_directory::validate(path)?;
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Ok(("/".into(), None));
    }
    let (parent, name) = path
        .rsplit_once('/')
        .ok_or("Expected absolute remote path")?;
    Ok((
        if parent.is_empty() {
            "/".into()
        } else {
            parent.into()
        },
        Some(name.into()),
    ))
}

fn enqueue_uploads(
    queue: &mut std::collections::VecDeque<std::path::PathBuf>,
    paths: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    if paths.len() > 64usize.saturating_sub(queue.len()) {
        return Err(
            "The upload review queue holds up to 64 files. Finish or clear waiting files first."
                .into(),
        );
    }
    for path in &paths {
        if !path.is_absolute() || path.file_name().is_none() || path.to_str().is_none() {
            return Err("Dropped files need absolute Unicode file paths.".into());
        }
    }
    queue.extend(paths);
    Ok(())
}

fn quote_terminal_path(path: &str) -> Result<String, String> {
    if path.chars().any(char::is_control) {
        return Err("Use Copy remote path for names containing control characters.".into());
    }
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

enum EditKind {
    /// Edit a listed regular file in a local app; carries its listed fingerprint.
    Edit(RemoteFingerprint),
    CreateFile,
    Permissions,
    Upload,
    Download,
    Create,
    Rename,
    Delete,
}
struct Editor {
    local: String,
    kind: EditKind,
    parent: String,
    source: Option<(String, bool)>,
    name: String,
}
fn child_path(parent: &str, name: &str) -> Result<String, String> {
    if parent.is_empty() || parent.contains('\0') {
        return Err("Enter a remote folder before reviewing the upload.".into());
    }
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err("Enter one filename without a slash or NUL character.".into());
    }
    let path = format!("{}/{}", parent.trim_end_matches('/'), name);
    if path.len() > 4096 {
        return Err("Remote path exceeds 4096 bytes.".into());
    }
    Ok(path)
}
fn move_destination(parent: &str, destination: &str) -> Result<String, String> {
    if parent.is_empty() || parent.contains('\0') {
        return Err("Select a remote folder first.".into());
    }
    let name = destination.rsplit('/').next().unwrap_or_default();
    if name.is_empty() || name == "." || name == ".." || destination.contains('\0') {
        return Err("Enter a destination path ending in the new entry name.".into());
    }
    // Preserve .. and literal whitespace: lexical normalization can change
    // the meaning of paths that traverse remote symbolic links.
    let path = if destination.starts_with('/') {
        destination.to_owned()
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), destination)
    };
    if path.len() > 4096 {
        return Err("Remote path exceeds 4096 bytes.".into());
    }
    Ok(path)
}
impl Editor {
    fn action(&self) -> Result<Action, String> {
        match self.kind {
            EditKind::Edit(_) => Err("Use Start editing to begin.".into()),
            EditKind::Permissions => {
                let source = self.source.as_ref().ok_or("Select an entry first.")?;
                if !(3..=4).contains(&self.name.len())
                    || !self.name.bytes().all(|b| (b'0'..=b'7').contains(&b))
                {
                    return Err("Enter three or four octal digits (0-7).".into());
                }
                Ok(Action::SetPermissions {
                    path: child_path(&self.parent, &source.0)?,
                    mode: u32::from_str_radix(&self.name, 8).map_err(|e| e.to_string())?,
                })
            }
            EditKind::Upload => {
                let local = std::path::PathBuf::from(&self.local);
                if !local.is_absolute() || local.file_name().is_none() {
                    return Err("Choose an absolute local source file path.".into());
                }
                Ok(Action::Upload {
                    local,
                    remote: child_path(&self.parent, &self.name)?,
                })
            }
            EditKind::Download => {
                let source = self
                    .source
                    .as_ref()
                    .map(|source| source.0.as_str())
                    .unwrap_or(&self.local);
                let local = std::path::PathBuf::from(&self.name);
                if !local.is_absolute() || local.file_name().is_none() {
                    return Err("Choose an absolute local file path.".into());
                }
                Ok(Action::Download {
                    remote: child_path(&self.parent, source)?,
                    local,
                })
            }
            EditKind::CreateFile => Ok(Action::CreateFile {
                path: child_path(&self.parent, &self.name)?,
            }),
            EditKind::Create => Ok(Action::CreateDirectory {
                path: child_path(&self.parent, &self.name)?,
            }),
            EditKind::Rename => {
                let source = self.source.as_ref().ok_or("Select an entry first.")?;
                let from = child_path(&self.parent, &source.0)?;
                let to = move_destination(&self.parent, &self.name)?;
                if from == to {
                    return Err("Choose a different destination.".into());
                }
                Ok(Action::Rename { from, to })
            }
            EditKind::Delete => {
                let source = self.source.as_ref().ok_or("Select an entry first.")?;
                let path = child_path(&self.parent, &source.0)?;
                Ok(if source.1 {
                    Action::RemoveDirectory { path }
                } else {
                    Action::RemoveFile { path }
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clicked_paths_preserve_literal_names_and_locate_the_containing_folder() {
        assert_eq!(
            super::locate_target("/tmp/a b;$(literal) 界").unwrap(),
            ("/tmp".into(), Some("a b;$(literal) 界".into()))
        );
        assert_eq!(
            super::locate_target("/tmp/folder/").unwrap(),
            ("/tmp".into(), Some("folder".into()))
        );
        assert_eq!(super::locate_target("/").unwrap(), ("/".into(), None));
        for path in ["relative", "C:\\local", "/tmp/bad\nname"] {
            assert!(super::locate_target(path).is_err());
        }
    }
    use super::*;
    #[test]
    fn large_remote_listing_renders_only_visible_rows_and_keeps_selection() {
        let context = egui::Context::default();
        let listing = Listing {
            path: "/fixture".into(),
            entries: (0..10_000)
                .map(|index| neo_ssh::files::Entry {
                    name: if index == 0 {
                        "界".repeat(1000)
                    } else {
                        format!("file-{index:05}")
                    },
                    size: Some(u64::MAX),
                    permissions: Some(0o100600),
                    modified: None,
                })
                .collect(),
        };
        let mut selected = Some(9999);
        for reveal in [false, false, true, false] {
            let mut rendered = 0;
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(650.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (navigate, count) =
                            remote_rows(ui, &listing, &mut selected, true, 230.0, reveal);
                        rendered = count;
                        assert!(navigate.is_none());
                        assert!(
                            ui.min_rect().width() <= 650.0,
                            "Long names expanded the table"
                        );
                    });
                },
            );
            if reveal {
                fn includes_entry(shape: &egui::Shape) -> bool {
                    match shape {
                        egui::Shape::Text(text) => text.galley.job.text.contains("file-09999"),
                        egui::Shape::Vec(shapes) => shapes.iter().any(includes_entry),
                        _ => false,
                    }
                }
                assert!(
                    output
                        .shapes
                        .iter()
                        .any(|shape| includes_entry(&shape.shape)),
                    "Requested entry was not brought into view"
                );
            }
            assert!(
                rendered > 0 && rendered < 20,
                "Rendered {rendered} of 10000 entries"
            );
            assert_eq!(selected, Some(9999));
        }
    }
    /// Where `text` was painted and the rectangle it was clipped to.
    fn painted_text(output: &egui::FullOutput, text: &str) -> Vec<(egui::Rect, egui::Rect)> {
        fn visit(
            shape: &egui::Shape,
            clip: egui::Rect,
            text: &str,
            found: &mut Vec<(egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Text(shape) if shape.galley.job.text == text => found.push((
                    egui::Rect::from_min_size(shape.pos, shape.galley.size()),
                    clip,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        visit(shape, clip, text, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &output.shapes {
            visit(&clipped.shape, clipped.clip_rect, text, &mut found);
        }
        found
    }
    #[test]
    fn clipboard_upload_review_buttons_are_visible_on_short_screens() {
        // 1920x1080 at 150% Windows scaling leaves about 1280x670 points; also
        // check a shorter window. A top panel stands in for the tab header.
        for height in [670.0, 560.0] {
            let context = egui::Context::default();
            let mut browser = Browser::blank("/home/user".into(), None);
            browser.listing = Some(Listing {
                path: "/home/user".into(),
                entries: (0..200)
                    .map(|index| neo_ssh::files::Entry {
                        name: format!("file-{index:03}"),
                        size: Some(1),
                        permissions: Some(0o100600),
                        modified: None,
                    })
                    .collect(),
            });
            let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
            browser.clipboard_preview = Some(context.load_texture(
                "clipboard-preview",
                egui::ColorImage::new([320, 200], egui::Color32::GRAY),
                egui::TextureOptions::LINEAR,
            ));
            browser.clipboard_dimensions = "PNG image: 1920 × 1080 pixels".into();
            browser.editor = Some(Editor {
                kind: EditKind::Upload,
                parent: "/home/user".into(),
                source: None,
                name: "clipboard.png".into(),
                local: file.path().to_string_lossy().into_owned(),
            });
            browser.clipboard_file = Some(file);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, height));
            let mut output = None;
            // Windows settle their size over the first frames.
            for _ in 0..4 {
                output = Some(context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::TopBottomPanel::top("tabs")
                            .exact_height(60.0)
                            .show(ctx, |_| {});
                        assert!(browser.show(ctx, None, true, "user@host"));
                    },
                ));
            }
            let visible = |output: &egui::FullOutput, label: &str| {
                let found = painted_text(output, label);
                assert!(
                    found.iter().any(
                        |(rect, clip)| clip.contains_rect(*rect) && screen.contains_rect(*rect)
                    ),
                    "{label:?} is not visible on a 1280x{height} screen: painted at {found:?}"
                );
            };
            let output = output.unwrap();
            for label in ["Start upload", "Cancel", "New name"] {
                visible(&output, label);
            }
            // Without a review, the folder's own buttons stay below the list.
            browser.editor = None;
            browser.clipboard_preview = None;
            browser.clipboard_file = None;
            let mut output = None;
            for _ in 0..4 {
                output = Some(context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::TopBottomPanel::top("tabs")
                            .exact_height(60.0)
                            .show(ctx, |_| {});
                        assert!(browser.show(ctx, None, true, "user@host"));
                    },
                ));
            }
            let output = output.unwrap();
            for label in ["Clipboard image", "Upload…", "New folder"] {
                visible(&output, label);
            }
        }
    }
    #[test]
    fn dropped_upload_queue_is_bounded_atomic_and_preserves_literal_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file ' ; $(test).txt");
        let mut queue = std::collections::VecDeque::new();
        enqueue_uploads(&mut queue, vec![path.clone()]).unwrap();
        assert!(enqueue_uploads(&mut queue, vec![path.clone(), "relative.txt".into()]).is_err());
        assert_eq!(queue.len(), 1);
        assert!(enqueue_uploads(&mut queue, vec![path.clone(); 64]).is_err());
        assert_eq!(queue.pop_front(), Some(path.clone()));
        let editor = Editor {
            kind: EditKind::Upload,
            parent: "/chosen".into(),
            source: None,
            name: "file ' ; $(test).txt".into(),
            local: path.to_str().unwrap().into(),
        };
        let Action::Upload { local, remote } = editor.action().unwrap() else {
            panic!()
        };
        assert_eq!(local, path);
        assert_eq!(remote, "/chosen/file ' ; $(test).txt");
        assert_eq!(
            quote_terminal_path(&remote).unwrap(),
            "'/chosen/file '\\'' ; $(test).txt'"
        );
        assert!(quote_terminal_path("/line\nbreak").is_err());
        assert!(quote_terminal_path("/escape\x1b").is_err());
    }
    #[test]
    fn permissions_review_accepts_only_octal_bits_and_keeps_the_selected_path() {
        let mut editor = Editor {
            local: String::new(),
            kind: EditKind::Permissions,
            parent: "/selected".into(),
            source: Some(("file.txt".into(), false)),
            name: String::new(),
        };
        for text in ["", "77", "888", "10000", "-600", "+600", " 600", "6/0"] {
            editor.name = text.into();
            assert!(editor.action().is_err(), "{text}");
        }
        for (text, expected) in [
            ("000", 0),
            ("640", 0o640),
            ("2750", 0o2750),
            ("7777", 0o7777),
        ] {
            editor.name = text.into();
            let Action::SetPermissions { path, mode } = editor.action().unwrap() else {
                panic!()
            };
            assert_eq!(path, "/selected/file.txt");
            assert_eq!(mode, expected);
        }
    }
    #[test]
    fn move_review_resolves_paths_and_rejects_ambiguous_destinations() {
        let mut editor = Editor {
            local: String::new(),
            kind: EditKind::Rename,
            parent: "/selected".into(),
            source: Some(("before.txt".into(), false)),
            name: String::new(),
        };
        for (input, expected) in [
            ("after.txt", "/selected/after.txt"),
            ("sub/file.txt", "/selected/sub/file.txt"),
            ("../other/file.txt", "/selected/../other/file.txt"),
            ("/other/file.txt", "/other/file.txt"),
            (
                "sub/../literal ' ; $(echo x) ",
                "/selected/sub/../literal ' ; $(echo x) ",
            ),
        ] {
            editor.name = input.into();
            let Action::Rename { from, to } = editor.action().unwrap() else {
                panic!()
            };
            assert_eq!(from, "/selected/before.txt");
            assert_eq!(to, expected);
        }
        for input in [
            "",
            "/",
            "sub/",
            ".",
            "..",
            "sub/..",
            "sub/.",
            "a\0b",
            "before.txt",
            "/selected/before.txt",
        ] {
            editor.name = input.into();
            assert!(editor.action().is_err(), "accepted {input:?}");
        }
        editor.name = "x".repeat(4096);
        assert!(editor.action().is_err());
        assert_eq!(move_destination("/", "file").unwrap(), "/file");
    }
    #[test]
    fn reviewed_actions_preserve_literal_names_and_creation_stays_in_selected_folder() {
        for name in ["", ".", "..", "../outside", "/outside", "a\0b"] {
            assert!(child_path("/selected", name).is_err());
        }
        let mut editor = Editor {
            local: String::new(),
            kind: EditKind::Rename,
            parent: "/selected".into(),
            source: Some(("before.txt".into(), false)),
            name: "文件 ' ; $(echo x).txt".into(),
        };
        let Action::Rename { from, to } = editor.action().unwrap() else {
            panic!()
        };
        assert_eq!(from, "/selected/before.txt");
        assert_eq!(to, "/selected/文件 ' ; $(echo x).txt");
        editor.kind = EditKind::Delete;
        assert!(matches!(
            editor.action().unwrap(),
            Action::RemoveFile { .. }
        ));
        editor.source.as_mut().unwrap().1 = true;
        assert!(matches!(
            editor.action().unwrap(),
            Action::RemoveDirectory { .. }
        ));
        assert_eq!(child_path("/", "folder").unwrap(), "/folder");
    }
}
