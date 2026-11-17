//! Local navigation with bounded background enumeration, upload selection and
//! reviewed file actions (create, rename, copy, move, delete) that never replace
//! existing entries. File actions run on a background thread.
use neo_ssh::local_ops::{self, Progress};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
static READERS: AtomicUsize = AtomicUsize::new(0);
struct ReaderGuard;
impl Drop for ReaderGuard {
    fn drop(&mut self) {
        READERS.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
    file: bool,
    size: Option<u64>,
}
struct Listing {
    path: PathBuf,
    entries: Vec<Entry>,
}
struct Job {
    result: mpsc::Receiver<Result<Listing, String>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Job {
    fn start(path: PathBuf) -> Result<Self, String> {
        READERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .map_err(|_| "Local folders are still being read. Try again shortly.")?;
        let guard = ReaderGuard;
        let (sender, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::Builder::new()
            .name("local-files".into())
            .spawn(move || {
                let _guard = guard;
                let _ = sender.send(read_directory(path, &stop));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { result, cancel })
    }
    fn poll(&self) -> Option<Result<Listing, String>> {
        match self.result.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Local directory reader ended.".into())),
        }
    }
}
fn read_directory(path: PathBuf, cancel: &AtomicBool) -> Result<Listing, String> {
    if !path.is_absolute() {
        return Err("Enter an absolute local folder path.".into());
    }
    if cancel.load(Ordering::Acquire) {
        return Err("Local folder request cancelled.".into());
    }
    let path = path
        .canonicalize()
        .map_err(|e| format!("Cannot open local folder: {e}"))?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(|e| format!("Cannot read local folder: {e}"))? {
        if cancel.load(Ordering::Acquire) {
            return Err("Local folder request cancelled.".into());
        }
        if entries.len() >= 10_000 {
            return Err("Local folder exceeds the 10,000-entry browsing limit.".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        let metadata = std::fs::metadata(&entry_path).ok();
        entries.push(Entry {
            path: entry_path,
            name: entry.file_name().to_string_lossy().into_owned(),
            directory: metadata.as_ref().is_some_and(|m| m.is_dir()),
            file: metadata.as_ref().is_some_and(|m| m.is_file()),
            size: metadata.map(|m| m.len()),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Listing { path, entries })
}

/// A file action running on its own thread. Dropping it (for example by hiding
/// the pane) cancels a copy in progress, which removes its temporary copy.
struct Operation {
    result: mpsc::Receiver<Result<String, String>>,
    progress: Arc<Progress>,
    label: &'static str,
    /// Folder that was shown when the action started.
    folder: PathBuf,
    /// Entry to select in `folder` after success.
    select: Option<String>,
    cancellable: bool,
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}
impl Operation {
    fn poll(&self) -> Option<Result<String, String>> {
        match self.result.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Local file action ended unexpectedly. Check the folder before retrying.".into(),
            )),
        }
    }
}

/// Selection to restore, by name, when the refreshed folder arrives.
struct Reselect {
    folder: PathBuf,
    current: Option<String>,
    checked: Vec<String>,
}

enum Pending {
    NewFolder,
    NewFile,
    Rename {
        source: PathBuf,
    },
    Copy {
        source: PathBuf,
        destination: PathBuf,
    },
    Move {
        source: PathBuf,
        destination: PathBuf,
    },
    Delete {
        path: PathBuf,
    },
}
/// An action awaiting confirmation; shown inline below the listing.
struct Review {
    pending: Pending,
    folder: PathBuf,
    name: String,
    /// Precomputed description for actions without a name field.
    summary: Result<String, String>,
    focus: bool,
}
impl Review {
    fn named(pending: Pending, folder: PathBuf, name: String) -> Self {
        Self {
            pending,
            folder,
            name,
            summary: Ok(String::new()),
            focus: true,
        }
    }
    fn fixed(pending: Pending, folder: PathBuf, summary: Result<String, String>) -> Self {
        Self {
            pending,
            folder,
            name: String::new(),
            summary,
            focus: false,
        }
    }
    fn takes_name(&self) -> bool {
        matches!(
            self.pending,
            Pending::NewFolder | Pending::NewFile | Pending::Rename { .. }
        )
    }
    fn title(&self) -> &'static str {
        match self.pending {
            Pending::NewFolder => "Create local folder",
            Pending::NewFile => "Create empty local file",
            Pending::Rename { .. } => "Rename local entry",
            Pending::Copy { .. } => "Copy local entry",
            Pending::Move { .. } => "Move local entry",
            Pending::Delete { .. } => "Delete local entry",
        }
    }
    fn confirm_label(&self) -> &'static str {
        match self.pending {
            Pending::NewFolder => "Create folder",
            Pending::NewFile => "Create file",
            Pending::Rename { .. } => "Confirm rename",
            Pending::Copy { .. } => "Start copy",
            Pending::Move { .. } => "Confirm move",
            Pending::Delete { .. } => "Confirm delete",
        }
    }
    /// What confirming will do, or why it cannot. Name checks here are pure;
    /// existence is enforced atomically when the action runs.
    fn plan(&self) -> Result<String, String> {
        let shown = local_ops::display_path;
        match &self.pending {
            Pending::NewFolder => local_ops::child_path(&self.folder, &self.name).map(|path| {
                format!(
                    "Create folder: {}\nAn existing entry is never replaced.",
                    shown(&path)
                )
            }),
            Pending::NewFile => local_ops::child_path(&self.folder, &self.name).map(|path| {
                format!(
                    "Create empty file: {}\nAn existing entry is never replaced.",
                    shown(&path)
                )
            }),
            Pending::Rename { source } => {
                let parent = source.parent().unwrap_or(&self.folder);
                let target = local_ops::child_path(parent, &self.name)?;
                if source.file_name() == Some(OsStr::new(&self.name)) {
                    return Err("Enter a different name.".into());
                }
                Ok(format!(
                    "From: {}\nTo: {}\nAn existing entry is never replaced.",
                    shown(source),
                    shown(&target)
                ))
            }
            _ => self.summary.clone(),
        }
    }
}

fn delete_summary(path: &Path) -> Result<String, String> {
    let shown = local_ops::display_path(path);
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(format!(
            "Delete folder: {shown}\nOnly empty folders can be deleted. This cannot be undone."
        )),
        Ok(meta) if meta.file_type().is_symlink() => Ok(format!(
            "Delete link: {shown}\nOnly the link is removed; its target is kept. This cannot be undone."
        )),
        Ok(_) => Ok(format!("Delete file: {shown}\nThis cannot be undone.")),
        Err(error) => Err(format!("Cannot read {shown}: {error}")),
    }
}

/// Asks for a destination folder and reviews the copy or move into it.
/// Returns `Ok(None)` when the picker is cancelled.
fn transfer_review(source: &Path, folder: &Path, moving: bool) -> Result<Option<Review>, String> {
    let verb = if moving { "Move" } else { "Copy" };
    let name = local_ops::display_path(Path::new(source.file_name().unwrap_or_default()));
    let Some(destination) = rfd::FileDialog::new()
        .set_title(format!("{verb} {name} to folder"))
        .pick_folder()
    else {
        return Ok(None);
    };
    let target = local_ops::transfer_target(source, &destination)?;
    let note = if moving {
        "On the same volume this is a rename. Across volumes the entry is copied completely before the original is removed."
    } else {
        "Folders are copied with all their contents."
    };
    let summary = format!(
        "From: {}\nTo: {}\n{note} Existing entries are never replaced.",
        local_ops::display_path(source),
        local_ops::display_path(&target)
    );
    let source = source.to_owned();
    let pending = if moving {
        Pending::Move {
            source,
            destination,
        }
    } else {
        Pending::Copy {
            source,
            destination,
        }
    };
    Ok(Some(Review::fixed(pending, folder.to_owned(), Ok(summary))))
}

pub struct Browser {
    path: String,
    listing: Option<Listing>,
    job: Option<Job>,
    error: Option<String>,
    /// Files checked for upload.
    selected: BTreeSet<usize>,
    /// Entry that file actions apply to (clicked name).
    current: Option<usize>,
    review: Option<Review>,
    op: Option<Operation>,
    notice: Option<String>,
    reselect: Option<Reselect>,
}
impl Browser {
    pub fn new() -> Self {
        let path = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        Self::open(path)
    }
    fn open(path: PathBuf) -> Self {
        let mut browser = Self {
            path: path.to_string_lossy().into_owned(),
            listing: None,
            job: None,
            error: None,
            selected: BTreeSet::new(),
            current: None,
            review: None,
            op: None,
            notice: None,
            reselect: None,
        };
        browser.load(path);
        browser
    }
    fn load(&mut self, path: PathBuf) {
        match Job::start(path) {
            Ok(job) => {
                self.job = Some(job);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }
    fn entry_name(&self, index: usize) -> Option<String> {
        let listing = self.listing.as_ref()?;
        listing.entries.get(index).map(|entry| entry.name.clone())
    }
    fn apply_listing(&mut self, listing: Listing) {
        let keep = self
            .reselect
            .take()
            .filter(|keep| keep.folder == listing.path);
        let position = |name: &str| listing.entries.iter().position(|e| e.name == name);
        match keep {
            Some(keep) => {
                self.current = keep.current.as_deref().and_then(position);
                self.selected = keep
                    .checked
                    .iter()
                    .filter_map(|name| position(name))
                    .filter(|index| listing.entries[*index].file)
                    .collect();
            }
            None => {
                self.current = None;
                self.selected.clear();
            }
        }
        self.path = listing.path.to_string_lossy().into_owned();
        self.listing = Some(listing);
    }
    fn poll(&mut self) {
        if let Some(result) = self.job.as_ref().and_then(Job::poll) {
            self.job = None;
            match result {
                Ok(listing) => self.apply_listing(listing),
                Err(error) => self.error = Some(error),
            }
        }
        let Some(result) = self.op.as_ref().and_then(Operation::poll) else {
            return;
        };
        let Some(op) = self.op.take() else {
            return;
        };
        let shown = self.listing.as_ref().map(|listing| listing.path.clone());
        if shown.as_ref() == Some(&op.folder) {
            let current = match &result {
                Ok(_) => op.select.clone(),
                Err(_) => self.current.and_then(|index| self.entry_name(index)),
            };
            let checked = self
                .selected
                .iter()
                .filter_map(|index| self.entry_name(*index))
                .collect();
            self.reselect = Some(Reselect {
                folder: op.folder.clone(),
                current,
                checked,
            });
        }
        // Refresh after failures too: a failed action may have changed something.
        // A pending navigation will show fresh contents anyway.
        if let (None, Some(path)) = (&self.job, shown) {
            self.load(path);
        }
        match result {
            Ok(notice) => self.notice = Some(notice),
            Err(error) => {
                self.notice = None;
                self.error = Some(error);
            }
        }
    }
    fn start(
        &mut self,
        label: &'static str,
        cancellable: bool,
        folder: PathBuf,
        select: Option<String>,
        work: impl FnOnce(&Progress) -> Result<String, String> + Send + 'static,
    ) {
        let progress = Arc::new(Progress::default());
        let worker = progress.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("local-file-action".into())
            .spawn(move || {
                let _ = sender.send(work(&worker));
            });
        match spawned {
            Ok(_) => {
                self.op = Some(Operation {
                    result,
                    progress,
                    label,
                    folder,
                    select,
                    cancellable,
                });
                self.error = None;
                self.notice = None;
            }
            Err(error) => self.error = Some(format!("Cannot start local file action: {error}")),
        }
    }
    fn submit(&mut self, review: Review) {
        let Review {
            pending,
            folder,
            name,
            ..
        } = review;
        let shown = local_ops::display_path;
        let parent = folder.clone();
        let select = Some(name.clone());
        match pending {
            Pending::NewFolder => self.start("Creating folder", false, folder, select, move |_| {
                local_ops::create_folder(&parent, &name)
                    .map(|_| format!("Created folder {name:?}."))
            }),
            Pending::NewFile => self.start("Creating file", false, folder, select, move |_| {
                local_ops::create_file(&parent, &name)
                    .map(|_| format!("Created empty file {name:?}."))
            }),
            Pending::Rename { source } => {
                let old = source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.start("Renaming", false, folder, select, move |_| {
                    local_ops::rename(&source, &name)
                        .map(|_| format!("Renamed {old:?} to {name:?}."))
                })
            }
            Pending::Copy {
                source,
                destination,
            } => {
                let keep = source.file_name().map(|n| n.to_string_lossy().into_owned());
                self.start("Copying", true, folder, keep, move |progress| {
                    local_ops::copy_into(&source, &destination, progress)
                        .map(|target| format!("Copied to {}.", shown(&target)))
                })
            }
            Pending::Move {
                source,
                destination,
            } => self.start("Moving", true, folder, None, move |progress| {
                local_ops::move_into(&source, &destination, progress)
                    .map(|target| format!("Moved to {}.", shown(&target)))
            }),
            Pending::Delete { path } => self.start("Deleting", false, folder, None, move |_| {
                local_ops::delete(&path).map(|()| format!("Deleted {}.", shown(&path)))
            }),
        }
    }
    fn operation_status(&self, ui: &mut egui::Ui) {
        let Some(op) = &self.op else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            let (files, bytes) = (op.progress.files(), op.progress.bytes());
            if files > 0 {
                ui.label(format!(
                    "{}… {files} file(s), {bytes} bytes copied",
                    op.label
                ));
            } else {
                ui.label(format!("{}…", op.label));
            }
            if op.cancellable {
                let cancelling = op.progress.is_cancelled();
                if ui
                    .add_enabled(!cancelling, egui::Button::new("Cancel"))
                    .clicked()
                {
                    op.progress.cancel();
                }
                if cancelling {
                    ui.label("Cancelling; the temporary copy will be removed.");
                }
            }
        });
    }
    fn review_ui(&mut self, ui: &mut egui::Ui) {
        let Some(review) = &mut self.review else {
            return;
        };
        let mut submit = false;
        let mut dismiss = false;
        ui.separator();
        ui.group(|ui| {
            ui.strong(review.title());
            let mut edit = None;
            if review.takes_name() {
                ui.label("New name");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut review.name).desired_width(320.0));
                if std::mem::take(&mut review.focus) {
                    response.request_focus();
                }
                edit = Some(response);
            }
            let entered = edit.as_ref().is_some_and(|edit| {
                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            });
            let plan = review.plan();
            match &plan {
                Ok(text) | Err(text) => ui.label(text),
            };
            if let (true, Err(_), Some(edit)) = (entered, &plan, &edit) {
                edit.request_focus();
            }
            ui.horizontal(|ui| {
                let confirm =
                    ui.add_enabled(plan.is_ok(), egui::Button::new(review.confirm_label()));
                if confirm.has_focus() {
                    confirm.scroll_to_me(Some(egui::Align::Center));
                }
                let cancel = ui.button("Cancel");
                if cancel.has_focus() {
                    cancel.scroll_to_me(Some(egui::Align::Center));
                }
                submit = plan.is_ok() && (confirm.clicked() || entered);
                dismiss = cancel.clicked();
            });
        });
        if dismiss {
            self.review = None;
        } else if submit {
            if let Some(review) = self.review.take() {
                self.submit(review);
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, can_upload: bool) -> Vec<PathBuf> {
        self.poll();
        let mut navigate = None;
        let mut upload = Vec::new();
        let mut review = None;
        let browsing = self.job.is_none() && self.review.is_none();
        let idle = browsing && self.op.is_none();
        ui.group(|ui| {
            ui.strong("Local files");
            ui.add_enabled_ui(browsing, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Parent folder").clicked() {
                        navigate = self
                            .listing
                            .as_ref()
                            .and_then(|listing| listing.path.parent().map(PathBuf::from));
                    }
                    let path =
                        ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(360.0));
                    if ui.button("Read folder").clicked()
                        || path.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        navigate = Some(PathBuf::from(&self.path));
                    }
                });
            });
            if self.job.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading local folder…");
                    if ui.button("Cancel local read").clicked() {
                        self.job = None;
                    }
                });
            }
            self.operation_status(ui);
            if let Some(notice) = &self.notice {
                ui.label(notice);
            }
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            if let Some(listing) = &self.listing {
                ui.label(format!(
                    "{} entries · {} selected",
                    listing.entries.len(),
                    self.selected.len()
                ));
                egui::ScrollArea::vertical()
                    .id_source("local-files-list")
                    .max_height(210.0)
                    .show_rows(ui, 22.0, listing.entries.len(), |ui, rows| {
                        for index in rows {
                            let entry = &listing.entries[index];
                            ui.add_enabled_ui(browsing, |ui| {
                                ui.horizontal(|ui| {
                                    if entry.file {
                                        let mut selected = self.selected.contains(&index);
                                        if ui.checkbox(&mut selected, "").changed() {
                                            if selected {
                                                if self.selected.len() < 64 {
                                                    self.selected.insert(index);
                                                } else {
                                                    self.error = Some(
                                                        "Select up to 64 files at a time.".into(),
                                                    );
                                                }
                                            } else {
                                                self.selected.remove(&index);
                                            }
                                        }
                                    } else {
                                        ui.add_space(20.0);
                                    }
                                    let name = format!(
                                        "{}{}",
                                        entry.name.escape_debug(),
                                        if entry.directory { "/" } else { "" }
                                    );
                                    let response =
                                        ui.selectable_label(self.current == Some(index), name);
                                    if response.clicked() {
                                        self.current = Some(index);
                                    }
                                    if response.double_clicked() && entry.directory {
                                        navigate = Some(entry.path.clone());
                                    }
                                    if entry.file {
                                        ui.weak(format!("{} bytes", entry.size.unwrap_or(0)));
                                    }
                                });
                            });
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_upload && self.job.is_none() && !self.selected.is_empty(),
                            egui::Button::new("Queue selected uploads"),
                        )
                        .clicked()
                    {
                        upload = self
                            .selected
                            .iter()
                            .filter_map(|index| listing.entries.get(*index))
                            .map(|entry| entry.path.clone())
                            .collect();
                    }
                    if ui.button("Clear local selection").clicked() {
                        self.selected.clear();
                    }
                });
                ui.add_enabled_ui(idle, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let folder = || listing.path.clone();
                        if ui.button("New folder…").clicked() {
                            review =
                                Some(Review::named(Pending::NewFolder, folder(), String::new()));
                        }
                        if ui.button("New file…").clicked() {
                            review = Some(Review::named(Pending::NewFile, folder(), String::new()));
                        }
                        let entry = self.current.and_then(|index| listing.entries.get(index));
                        let mut button = |label: &str| {
                            ui.add_enabled(entry.is_some(), egui::Button::new(label))
                                .clicked()
                        };
                        let (rename, copy, moving, delete) = (
                            button("Rename…"),
                            button("Copy to…"),
                            button("Move to…"),
                            button("Delete…"),
                        );
                        let Some(entry) = entry else {
                            return;
                        };
                        if rename {
                            review = Some(Review::named(
                                Pending::Rename {
                                    source: entry.path.clone(),
                                },
                                folder(),
                                entry.name.clone(),
                            ));
                        }
                        if copy || moving {
                            match transfer_review(&entry.path, &listing.path, moving) {
                                Ok(picked) => review = picked,
                                Err(error) => {
                                    self.notice = None;
                                    self.error = Some(error);
                                }
                            }
                        }
                        if delete {
                            review = Some(Review::fixed(
                                Pending::Delete {
                                    path: entry.path.clone(),
                                },
                                folder(),
                                delete_summary(&entry.path),
                            ));
                        }
                    });
                });
                ui.label(
                    "Double-click folders to navigate. Click a name to select it for file actions. \
                     Queued files are reviewed before upload.",
                );
            }
            self.review_ui(ui);
        });
        if review.is_some() {
            self.review = review;
        }
        if let Some(path) = navigate {
            self.notice = None;
            self.load(path);
        }
        upload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_listing_keeps_paths_sizes_directories_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        std::fs::write(dir.path().join("文件 ' .bin"), b"hello").unwrap();
        let stop = AtomicBool::new(false);
        let listing = read_directory(dir.path().to_owned(), &stop).unwrap();
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries[0].directory);
        assert!(listing.entries[1].file);
        assert_eq!(listing.entries[1].size, Some(5));
        assert_eq!(std::fs::read(&listing.entries[1].path).unwrap(), b"hello");
        assert!(read_directory("relative".into(), &stop).is_err());
        stop.store(true, Ordering::Release);
        assert!(read_directory(dir.path().into(), &stop).is_err());
    }

    fn settle(browser: &mut Browser) {
        for _ in 0..500 {
            browser.poll();
            if browser.job.is_none() && browser.op.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("local browser did not settle");
    }

    fn render(browser: &mut Browser) {
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(browser.show(ui, false).is_empty());
            });
        });
    }

    #[test]
    fn file_actions_run_in_background_refresh_and_keep_selection_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"c").unwrap();
        let mut browser = Browser::open(dir.path().to_owned());
        settle(&mut browser);
        let folder = browser.listing.as_ref().unwrap().path.clone();
        browser.selected.insert(1); // c.txt checked for upload
        browser.current = Some(0);
        render(&mut browser);
        // Create a folder: the new entry is selected and the checked file stays checked.
        browser.review = Some(Review::named(
            Pending::NewFolder,
            folder.clone(),
            "a folder".into(),
        ));
        render(&mut browser);
        let review = browser.review.take().unwrap();
        assert!(review.plan().is_ok());
        browser.submit(review);
        render(&mut browser);
        settle(&mut browser);
        let listing = browser.listing.as_ref().unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a folder", "b.txt", "c.txt"]);
        assert_eq!(browser.current, Some(0));
        assert_eq!(browser.selected, BTreeSet::from([2]));
        assert!(browser.notice.as_deref().unwrap().contains("a folder"));
        // A refused action reports inline and keeps the current entry.
        browser.current = Some(1);
        browser.submit(Review::named(
            Pending::Rename {
                source: folder.join("b.txt"),
            },
            folder.clone(),
            "c.txt".into(),
        ));
        settle(&mut browser);
        assert!(browser.error.as_deref().unwrap().contains("already exists"));
        assert_eq!(
            browser.entry_name(browser.current.unwrap()).unwrap(),
            "b.txt"
        );
        assert_eq!(std::fs::read(dir.path().join("c.txt")).unwrap(), b"c");
        // Deleting a non-empty folder is refused with a clear message.
        std::fs::write(dir.path().join("a folder").join("inner"), b"x").unwrap();
        browser.review = Some(Review::fixed(
            Pending::Delete {
                path: folder.join("a folder"),
            },
            folder.clone(),
            delete_summary(&folder.join("a folder")),
        ));
        render(&mut browser);
        let review = browser.review.take().unwrap();
        browser.submit(review);
        settle(&mut browser);
        assert!(browser.error.as_deref().unwrap().contains("not empty"));
        assert!(dir.path().join("a folder").join("inner").exists());
        // Name reviews reject separators and dot names before anything runs.
        for name in ["", ".", "..", "x/y", "x\\y"] {
            let review = Review::named(Pending::NewFile, folder.clone(), name.into());
            assert!(review.plan().is_err(), "{name:?}");
        }
        render(&mut browser);
    }
}
