//! "Edit remote file": download a reviewed remote file into a private local
//! folder, open it with the system handler, and replace the remote file each
//! time a local save settles.
//!
//! Sessions are polled by their owner (the Remote files window) instead of a
//! background thread, so they can never outlive the SSH session whose file
//! queue they use: dropping a session cancels its transfer task.
use crate::files::{Action, ActionTask, RemoteFingerprint};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

/// How often the local copy is checked for saves.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Local file size and modification time, compared between polls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalStamp {
    len: u64,
    modified: Option<SystemTime>,
}
impl LocalStamp {
    /// `None` while the file is missing, e.g. during an editor's atomic save.
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// Debounced save detection: a change is reported only after it has been seen
/// unchanged on two consecutive polls, so half-written saves are not sent.
#[derive(Debug)]
pub struct ChangeDetector {
    synced: LocalStamp,
    candidate: Option<LocalStamp>,
}
impl ChangeDetector {
    pub fn new(synced: LocalStamp) -> Self {
        Self {
            synced,
            candidate: None,
        }
    }
    /// Returns the stamp to upload once a change is stable. Keeps returning it
    /// on later polls until [`Self::mark_synced`], so a refused submission is
    /// retried.
    pub fn observe(&mut self, current: Option<LocalStamp>) -> Option<LocalStamp> {
        match current {
            Some(current) if current != self.synced => {
                if self.candidate == Some(current) {
                    return Some(current);
                }
                self.candidate = Some(current);
            }
            _ => self.candidate = None,
        }
        None
    }
    pub fn mark_synced(&mut self, stamp: LocalStamp) {
        self.synced = stamp;
        self.candidate = None;
    }
    /// Require a fresh, stable observation before the next upload.
    pub fn reset(&mut self) {
        self.candidate = None;
    }
    pub fn pending(&self) -> bool {
        self.candidate.is_some()
    }
    /// A missing copy is never synced: an editor's atomic save may be
    /// between removing the old file and moving the new one into place.
    pub fn is_synced(&self, current: Option<LocalStamp>) -> bool {
        current == Some(self.synced)
    }
}

/// A local filename for a remote basename that is valid on every desktop
/// platform and keeps the extension, so the system picks a suitable editor.
pub fn local_file_name(remote_name: &str) -> String {
    let mut name: String = remote_name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows silently drops trailing dots and spaces.
    while name.ends_with(['.', ' ']) {
        name.pop();
    }
    if name.is_empty() {
        name = "remote-file".into();
    }
    if crate::local_ops::windows_device_name(&name) {
        name.insert(0, '_');
    }
    const LIMIT: usize = 200;
    if name.len() > LIMIT {
        let split = name
            .rfind('.')
            .filter(|&dot| dot > 0 && name.len() - dot <= 17)
            .unwrap_or(name.len());
        let extension = name[split..].to_owned();
        let mut stem = name[..split].to_owned();
        while stem.len() + extension.len() > LIMIT {
            stem.pop();
        }
        name = stem + &extension;
    }
    name
}

/// File types whose default "open" action can run code instead of editing.
/// They are never opened automatically (on Windows only through an "edit"
/// verb); saves of a manually opened copy still sync.
pub fn may_run_code(name: &str) -> bool {
    const RISKY: &[&str] = &[
        "accda",
        "accde",
        "accdr",
        "ade",
        "adp",
        "app",
        "appimage",
        "appinstaller",
        "application",
        "appref-ms",
        "appx",
        "appxbundle",
        "bash",
        "bat",
        "chm",
        "cmd",
        "com",
        "command",
        "cpl",
        "csh",
        "deb",
        "desktop",
        "diagcab",
        "dmg",
        "docm",
        "dotm",
        "exe",
        "fileloc",
        "fish",
        "flatpak",
        "flatpakref",
        "flatpakrepo",
        "gadget",
        "hlp",
        "hta",
        "htm",
        "html",
        "ica",
        "img",
        "inetloc",
        "inf",
        "ins",
        "iso",
        "isp",
        "its",
        "jar",
        "jnlp",
        "js",
        "jse",
        "ksh",
        "library-ms",
        "lnk",
        "lua",
        "mad",
        "maf",
        "mag",
        "mam",
        "maq",
        "mar",
        "mas",
        "mat",
        "mav",
        "maw",
        "mcf",
        "mda",
        "mdb",
        "mde",
        "mdt",
        "mdw",
        "mdz",
        "mht",
        "mhtml",
        "mpkg",
        "msc",
        "msh",
        "msh1",
        "msh1xml",
        "msh2",
        "msh2xml",
        "mshxml",
        "msi",
        "msix",
        "msixbundle",
        "msp",
        "mst",
        "msu",
        "ocx",
        "ops",
        "pcd",
        "php",
        "pif",
        "pkg",
        "pl",
        "plg",
        "potm",
        "ppam",
        "ppkg",
        "ppsm",
        "pptm",
        "prefpane",
        "prf",
        "prg",
        "ps1",
        "ps1xml",
        "ps2xml",
        "psc1",
        "psc2",
        "psd1",
        "psm1",
        "pssc",
        "py",
        "pyc",
        "pyw",
        "pyz",
        "qtz",
        "rb",
        "rdp",
        "reg",
        "rpm",
        "run",
        "saver",
        "scf",
        "scr",
        "sct",
        "search-ms",
        "searchconnector-ms",
        "settingcontent-ms",
        "sh",
        "shb",
        "shs",
        "shtml",
        "sldm",
        "snap",
        "svg",
        "svgz",
        "tcl",
        "terminal",
        "tool",
        "url",
        "vb",
        "vbe",
        "vbs",
        "vhd",
        "vhdx",
        "vsmacros",
        "vsw",
        "webloc",
        "website",
        "widget",
        "workflow",
        "ws",
        "wsb",
        "wsc",
        "wsf",
        "wsh",
        "xbap",
        "xht",
        "xhtml",
        "xla",
        "xlam",
        "xll",
        "xlsm",
        "xltm",
        "xnk",
        "zsh",
    ];
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| RISKY.contains(&extension.to_ascii_lowercase().as_str()))
}

/// Content that a Unix opener may launch instead of edit when the name has no
/// recognized extension (`xdg-open` falls back to sniffing the bytes):
/// native executables and desktop launchers. Scripts stay editable.
#[cfg_attr(windows, allow(dead_code))]
fn content_may_run_code(head: &[u8]) -> bool {
    head.starts_with(b"\x7fELF")
        || head.starts_with(b"\xcf\xfa\xed\xfe")
        || head.starts_with(b"\xca\xfe\xba\xbe")
        || head.starts_with(b"MZ")
        || head.starts_with(b"[Desktop Entry]")
}

/// Tag a downloaded copy as coming from another computer (Mark of the Web),
/// so Windows and Office apply their downloaded-file protections to it.
fn mark_downloaded(path: &Path) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        // Best effort: FAT/exFAT and some network drives have no streams.
        let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(not(windows))]
    let _ = path;
}

/// Opener program and arguments on macOS and other Unix desktops. The path is
/// one literal argument: no shell is involved.
#[cfg_attr(windows, allow(dead_code))]
fn opener_command(path: &Path, macos: bool) -> (&'static str, Vec<std::ffi::OsString>) {
    (
        if macos { "open" } else { "xdg-open" },
        vec![path.as_os_str().to_owned()],
    )
}

#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, path::Path, ptr};

    // shell32's ShellExecuteW, the API behind `start`, without cmd.exe: the
    // path is passed as data, so `&`, `%` or quotes in names are inert.
    // Declared here because Cargo.toml does not enable windows-sys
    // `Win32_UI_Shell`; raw-dylib needs no import library.
    #[link(name = "shell32", kind = "raw-dylib")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> *mut c_void;
    }

    pub(super) fn wide(text: &std::ffi::OsStr) -> Result<Vec<u16>, String> {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = text.encode_wide().collect();
        if wide.contains(&0) {
            return Err("Path contains a NUL character.".into());
        }
        wide.push(0);
        Ok(wide)
    }

    pub(super) fn shell_execute(path: &Path, verb: Option<&str>) -> Result<(), String> {
        let file = wide(path.as_os_str())?;
        let verb = verb.map(|verb| wide(verb.as_ref())).transpose()?;
        const SW_SHOWNORMAL: i32 = 1;
        // SAFETY: all strings are NUL-terminated and outlive the call.
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ref().map_or(ptr::null(), |verb| verb.as_ptr()),
                file.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        // Values above 32 indicate success.
        if result > 32 {
            Ok(())
        } else {
            Err(format!(
                "Windows could not open it (ShellExecute error {result})."
            ))
        }
    }
}

fn spawn_opener(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::shell_execute(path, None)
    }
    #[cfg(not(windows))]
    {
        let (program, args) = opener_command(path, cfg!(target_os = "macos"));
        let mut child = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start {program}: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
}

/// Open a downloaded copy with the system's default app (never through a
/// shell command line). Types that may run code are refused, except through
/// the Windows "edit" verb where one is registered.
pub fn open_for_editing(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    #[cfg(not(windows))]
    let risky = may_run_code(&name) || {
        use std::io::Read;
        let mut head = [0u8; 16];
        let read = std::fs::File::open(path)
            .and_then(|mut file| file.read(&mut head))
            .unwrap_or(0);
        content_may_run_code(&head[..read])
    };
    #[cfg(windows)]
    let risky = may_run_code(&name);
    if risky {
        #[cfg(windows)]
        if windows::shell_execute(path, Some("edit")).is_ok() {
            return Ok(());
        }
        return Err(refusal(path));
    }
    spawn_opener(path)
}
fn refusal(path: &Path) -> String {
    format!(
        "Not opened automatically because this file type can run programs. Open {} in your editor; saves still sync.",
        path.display()
    )
}

/// Show a folder in the system file manager.
pub fn open_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("The folder no longer exists.".into());
    }
    spawn_opener(path)
}

/// A new owner-only temporary folder for one edit session.
pub fn private_directory() -> Result<tempfile::TempDir, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("neo-ssh-edit-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder
        .tempdir()
        .map_err(|e| format!("Cannot create a private edit folder: {e}"))?;
    #[cfg(windows)]
    {
        // Replace the inherited ACL with a protected current-user-only DACL that
        // new files inside (the editor's saves) inherit.
        use std::os::windows::fs::OpenOptionsExt;
        let handle = std::fs::OpenOptions::new()
            .access_mode(0x0004_0000 | 0x0002_0000) // WRITE_DAC | READ_CONTROL
            .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS opens a folder
            .open(directory.path())
            .map_err(|e| format!("Cannot protect the edit folder: {e}"))?;
        crate::agent::pageant::restrict_folder(&handle)
            .map_err(|e| format!("Cannot protect the edit folder: {e}"))?;
    }
    Ok(directory)
}

enum State {
    Downloading(ActionTask),
    Watching,
    Uploading { task: ActionTask, stamp: LocalStamp },
    DownloadFailed(String),
    Conflict(String),
    Failed(String),
}

/// What the Remote files window shows for a session.
#[derive(Debug, PartialEq)]
pub enum Status<'a> {
    Downloading,
    /// Watching for saves; the time since the last upload, if any.
    Synced(Option<Duration>),
    /// A save was seen and will be uploaded once it settles.
    Pending,
    Uploading,
    Conflict(&'a str),
    Failed(&'a str),
}
impl Status<'_> {
    pub fn label(&self) -> String {
        match self {
            Self::Downloading => "Downloading…".into(),
            Self::Synced(None) => "Opened · no saves yet".into(),
            Self::Synced(Some(age)) => {
                format!("Synced {}", crate::row_activity::describe_age(*age))
            }
            Self::Pending => "Save detected · uploading soon".into(),
            Self::Uploading => "Uploading…".into(),
            Self::Conflict(_) => "Conflict · syncing stopped".into(),
            Self::Failed(_) => "Error · syncing stopped".into(),
        }
    }
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Conflict(_) | Self::Failed(_))
    }
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Conflict(detail) | Self::Failed(detail) => Some(detail),
            _ => None,
        }
    }
}

/// One remote file being edited locally.
pub struct EditSession {
    remote: String,
    name: String,
    local: PathBuf,
    directory: Option<tempfile::TempDir>,
    fingerprint: RemoteFingerprint,
    scp: bool,
    state: State,
    detector: Option<ChangeDetector>,
    next_check: Instant,
    last_sync: Option<Instant>,
    note: Option<String>,
    opener: fn(&Path) -> Result<(), String>,
}
impl EditSession {
    /// Start downloading `remote`, which was listed with `expected`, into a
    /// new private folder. `submit` queues the transfer on the SSH session.
    pub fn start(
        remote: &str,
        expected: RemoteFingerprint,
        scp: bool,
        submit: impl FnOnce(Action) -> Result<ActionTask, String>,
    ) -> Result<Self, String> {
        Self::start_with(remote, expected, scp, submit, open_for_editing)
    }
    fn start_with(
        remote: &str,
        expected: RemoteFingerprint,
        scp: bool,
        submit: impl FnOnce(Action) -> Result<ActionTask, String>,
        opener: fn(&Path) -> Result<(), String>,
    ) -> Result<Self, String> {
        let name = remote
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !matches!(*name, "." | ".."))
            .ok_or("Choose a remote file to edit.")?
            .to_owned();
        let directory = private_directory()?;
        let local = directory.path().join(local_file_name(&name));
        let task = submit(Action::EditDownload {
            remote: remote.to_owned(),
            local: local.clone(),
            expected,
            scp,
        })?;
        Ok(Self {
            remote: remote.to_owned(),
            name,
            local,
            directory: Some(directory),
            fingerprint: expected,
            scp,
            state: State::Downloading(task),
            detector: None,
            next_check: Instant::now(),
            last_sync: None,
            note: None,
            opener,
        })
    }
    pub fn remote(&self) -> &str {
        &self.remote
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn local(&self) -> &Path {
        &self.local
    }
    /// Non-fatal information, such as why the file was not opened.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn status(&self, now: Instant) -> Status<'_> {
        match &self.state {
            State::Downloading(_) => Status::Downloading,
            State::Uploading { .. } => Status::Uploading,
            State::Watching if self.detector.as_ref().is_some_and(ChangeDetector::pending) => {
                Status::Pending
            }
            State::Watching => {
                Status::Synced(self.last_sync.map(|at| now.saturating_duration_since(at)))
            }
            State::Conflict(detail) => Status::Conflict(detail),
            State::Failed(detail) | State::DownloadFailed(detail) => Status::Failed(detail),
        }
    }
    /// Whether the downloaded copy exists and can be opened again.
    pub fn is_open(&self) -> bool {
        !matches!(self.state, State::Downloading(_) | State::DownloadFailed(_))
    }
    pub fn can_retry(&self) -> bool {
        matches!(self.state, State::Failed(_))
    }
    /// After a non-conflict failure, upload the current local copy again.
    /// Conflicts cannot be retried: that would overwrite the server's change.
    pub fn retry(&mut self, now: Instant) {
        if let (State::Failed(_), Some(detector)) = (&self.state, &mut self.detector) {
            detector.reset();
            self.state = State::Watching;
            self.next_check = now;
            self.note = None;
        }
    }
    pub fn reopen(&mut self) {
        self.note = (self.opener)(&self.local).err();
    }
    /// Whether stopping now keeps the local copy because it holds edits that
    /// are not known to be on the server.
    pub fn keeps_copy(&self) -> bool {
        match &self.state {
            State::Downloading(_) | State::DownloadFailed(_) => false,
            State::Uploading { .. } | State::Conflict(_) | State::Failed(_) => true,
            State::Watching => self
                .detector
                .as_ref()
                .is_some_and(|detector| !detector.is_synced(LocalStamp::read(&self.local))),
        }
    }
    /// Advance the session: finish transfers, check the local copy every
    /// [`POLL_INTERVAL`] and submit a replacement for a settled save. Nothing
    /// is submitted while `connected` is false.
    pub fn poll(
        &mut self,
        now: Instant,
        connected: bool,
        submit: &mut dyn FnMut(Action) -> Result<ActionTask, String>,
    ) {
        match &self.state {
            State::Downloading(task) => {
                let Some(result) = task.poll() else { return };
                let downloaded = result.and_then(|()| {
                    // Before the baseline stamp, in case the stream touches it.
                    mark_downloaded(&self.local);
                    LocalStamp::read(&self.local)
                        .ok_or_else(|| "The downloaded copy is missing.".into())
                });
                self.state = match downloaded {
                    Ok(stamp) => {
                        self.detector = Some(ChangeDetector::new(stamp));
                        self.next_check = now + POLL_INTERVAL;
                        self.note = (self.opener)(&self.local).err();
                        State::Watching
                    }
                    Err(error) => State::DownloadFailed(error),
                };
            }
            State::Uploading { task, stamp } => {
                let Some(result) = task.poll() else { return };
                let stamp = *stamp;
                let conflict = task.remote_conflict();
                let uncertain = task.outcome_uncertain();
                let fingerprint = task.remote_fingerprint();
                let Some(detector) = self.detector.as_mut() else {
                    return;
                };
                self.next_check = now + POLL_INTERVAL;
                let local = self.local.display();
                self.state = match (result, fingerprint) {
                    (Ok(()), Some(fingerprint)) => {
                        self.fingerprint = fingerprint;
                        detector.mark_synced(stamp);
                        self.last_sync = Some(now);
                        State::Watching
                    }
                    (Ok(()), None) => State::Failed(format!(
                        "The server did not confirm the replaced file. Your edits are kept at {local}."
                    )),
                    (Err(_), _) if conflict => State::Conflict(format!(
                        "{} changed on the server; your edits are kept at {local}.",
                        self.remote
                    )),
                    // The server may hold this upload already, so its fingerprint
                    // is unknown: an automatic retry would report a false conflict.
                    (Err(error), _) if uncertain => State::Failed(format!(
                        "{error} Syncing stopped; your edits are kept at {local}."
                    )),
                    (Err(error), _) if LocalStamp::read(&self.local) != Some(stamp) => {
                        // Saved again while uploading: send the newer copy.
                        detector.reset();
                        self.note = Some(format!("Upload interrupted by a newer save: {error}"));
                        State::Watching
                    }
                    (Err(error), _) => State::Failed(format!(
                        "{error} Syncing stopped; your edits are kept at {local}."
                    )),
                };
            }
            State::Watching => {
                if !connected || now < self.next_check {
                    return;
                }
                self.next_check = now + POLL_INTERVAL;
                let Some(detector) = self.detector.as_mut() else {
                    return;
                };
                let Some(stamp) = detector.observe(LocalStamp::read(&self.local)) else {
                    return;
                };
                match submit(Action::EditReplace {
                    local: self.local.clone(),
                    remote: self.remote.clone(),
                    expected: self.fingerprint,
                    scp: self.scp,
                }) {
                    Ok(task) => {
                        self.note = None;
                        self.state = State::Uploading { task, stamp };
                    }
                    Err(error) => self.note = Some(format!("Waiting to upload: {error}")),
                }
            }
            State::DownloadFailed(_) | State::Conflict(_) | State::Failed(_) => {}
        }
    }
    /// Stop watching. Removes the temporary copy unless it holds edits that
    /// are not known to be on the server, and describes what happened.
    pub fn stop(mut self) -> String {
        let keep = self.keeps_copy();
        let uploading = matches!(self.state, State::Uploading { .. });
        let local = self.local.display().to_string();
        let name = self.name.clone();
        let Some(directory) = self.directory.take() else {
            return format!("Stopped editing {name}.");
        };
        let downloading = self.cancel_download();
        if keep {
            let _ = directory.keep();
            return if uploading {
                format!("Stopped editing {name} during an upload; the server copy may or may not have been replaced. Your edits are kept at {local}.")
            } else {
                format!("Stopped editing {name}. Your unsynced edits are kept at {local}.")
            };
        }
        match remove_folder(directory, downloading) {
            Ok(()) => format!("Stopped editing {name}; the temporary copy was removed."),
            Err(error) if downloading => format!(
                "Stopped editing {name} during its download; the partial copy will be removed once the transfer ends ({error})."
            ),
            Err(error) => format!(
                "Stopped editing {name}, but the temporary copy at {local} could not be removed: {error}"
            ),
        }
    }
    /// Drop an in-flight download task, which tells the worker to cancel it.
    /// Returns whether one was running.
    fn cancel_download(&mut self) -> bool {
        if matches!(self.state, State::Downloading(_)) {
            self.state = State::DownloadFailed("Stopped.".into());
            true
        } else {
            false
        }
    }
}
impl Drop for EditSession {
    fn drop(&mut self) {
        // Owner closed without stop(): never delete edits that may be unsynced.
        if let Some(directory) = self.directory.take() {
            if self.keeps_copy() {
                let _ = directory.keep();
            } else {
                let downloading = self.cancel_download();
                let _ = remove_folder(directory, downloading);
            }
        }
    }
}

/// Remove a session folder. A just-cancelled download may still hold its
/// partial file open (Windows refuses to delete open files), so a failed
/// removal is retried in the background for a few seconds.
fn remove_folder(directory: tempfile::TempDir, retry: bool) -> Result<(), String> {
    let path = directory.keep();
    match std::fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(_) if !path.exists() => Ok(()),
        Err(error) => {
            if retry {
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        std::thread::sleep(Duration::from_millis(100));
                        if std::fs::remove_dir_all(&path).is_ok() || !path.exists() {
                            return;
                        }
                    }
                });
            }
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{submit_action, Request};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    fn stamp(len: u64, seconds: u64) -> LocalStamp {
        LocalStamp {
            len,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
        }
    }

    #[test]
    fn saves_are_uploaded_only_after_two_identical_polls() {
        let mut detector = ChangeDetector::new(stamp(1, 1));
        assert_eq!(detector.observe(Some(stamp(1, 1))), None);
        assert!(!detector.pending());
        // Half-written save: still growing on the next poll.
        assert_eq!(detector.observe(Some(stamp(5, 2))), None);
        assert!(detector.pending());
        assert_eq!(detector.observe(Some(stamp(9, 2))), None);
        assert_eq!(detector.observe(Some(stamp(9, 2))), Some(stamp(9, 2)));
        // A refused submission is retried on the next poll.
        assert_eq!(detector.observe(Some(stamp(9, 2))), Some(stamp(9, 2)));
        // Missing during an atomic save: wait for a stable file again.
        assert_eq!(detector.observe(None), None);
        assert_eq!(detector.observe(Some(stamp(9, 2))), None);
        detector.mark_synced(stamp(9, 2));
        assert!(detector.is_synced(Some(stamp(9, 2))));
        assert!(!detector.is_synced(Some(stamp(9, 3))));
        assert_eq!(detector.observe(Some(stamp(9, 2))), None);
        // Reverting to the synced contents cancels a pending change.
        assert_eq!(detector.observe(Some(stamp(9, 3))), None);
        assert_eq!(detector.observe(Some(stamp(9, 2))), None);
        assert!(!detector.pending());
        assert_eq!(detector.observe(Some(stamp(9, 3))), None);
        detector.reset();
        assert_eq!(detector.observe(Some(stamp(9, 3))), None);
        assert_eq!(detector.observe(Some(stamp(9, 3))), Some(stamp(9, 3)));
    }

    #[test]
    fn local_names_are_portable_and_keep_their_extension() {
        for (remote, local) in [
            ("nginx.conf", "nginx.conf"),
            ("a:b*c?.txt", "a_b_c_.txt"),
            ("line\nbreak\u{1b}.md", "line_break_.md"),
            ("quote\"|<>\\.yml", "quote_____.yml"),
            ("trailing. . ", "trailing"),
            ("...", "remote-file"),
            ("CON", "_CON"),
            ("nul.txt", "_nul.txt"),
            ("com7.log", "_com7.log"),
            ("lpt1", "_lpt1"),
            ("COM\u{b9}.txt", "_COM\u{b9}.txt"),
            ("lpt\u{b3}", "_lpt\u{b3}"),
            ("COM10.txt", "COM10.txt"),
            ("computer.txt", "computer.txt"),
            ("conx.txt", "conx.txt"),
            ("文件 ' ; $(x) & %PATH%.txt", "文件 ' ; $(x) & %PATH%.txt"),
            (".bashrc", ".bashrc"),
        ] {
            assert_eq!(local_file_name(remote), local, "{remote:?}");
        }
        let long = local_file_name(&format!("{}.conf", "界".repeat(100)));
        assert!(long.len() <= 200 && long.ends_with("界.conf"), "{long}");
        assert!(local_file_name(&"x".repeat(300)).len() <= 200);
    }

    #[test]
    fn executable_types_are_never_opened_with_their_default_action() {
        for name in [
            "deploy.sh",
            "x.PS1",
            "run.bat",
            "a.txt.exe",
            "tool.py",
            "link.lnk",
            "s.command",
            "app.desktop",
            "r.vbs",
            "budget.XLSM",
            "launch.jnlp",
            "disk.iso",
            "remote.rdp",
            "index.html",
            "page.XHTML",
            "logo.svg",
            "saved.mht",
        ] {
            assert!(may_run_code(name), "{name}");
        }
        for head in [
            &b"\x7fELF\x02\x01"[..],
            b"MZ\x90\x00",
            b"[Desktop Entry]\nExec=x",
        ] {
            assert!(content_may_run_code(head), "{head:?}");
        }
        for head in [&b"#!/bin/sh\n"[..], b"server {", b""] {
            assert!(!content_may_run_code(head), "{head:?}");
        }
        for name in [
            "nginx.conf",
            "notes.txt",
            "Makefile",
            ".bashrc",
            "data.json",
            "sh",
        ] {
            assert!(!may_run_code(name), "{name}");
        }
        let refused = refusal(Path::new("/tmp/folder/deploy.sh"));
        assert!(refused.contains("deploy.sh"), "{refused}");
    }

    #[test]
    fn opener_receives_the_path_as_one_literal_argument() {
        let path = Path::new("/tmp/neo-ssh-edit-x/a b'; rm -rf ~ $(x) & y.txt");
        for (macos, program) in [(true, "open"), (false, "xdg-open")] {
            let (actual, args) = opener_command(path, macos);
            assert_eq!(actual, program);
            assert_eq!(args, vec![path.as_os_str().to_owned()]);
        }
        #[cfg(windows)]
        {
            let wide = windows::wide("C:\\Temp\\a & b %PATH% \"q\".txt".as_ref()).unwrap();
            assert_eq!(wide.last(), Some(&0));
            assert_eq!(wide.iter().filter(|&&unit| unit == 0).count(), 1);
            assert_eq!(
                String::from_utf16(&wide[..wide.len() - 1]).unwrap(),
                "C:\\Temp\\a & b %PATH% \"q\".txt"
            );
        }
        assert!(open_folder(Path::new("/nonexistent/neo-ssh-folder")).is_err());
    }

    #[test]
    fn private_edit_folders_are_owner_only_usable_and_removable() {
        let directory = private_directory().unwrap();
        let path = directory.path().to_owned();
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("neo-ssh-edit-"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let file = path.join("edited.txt");
        std::fs::write(&file, b"contents").unwrap();
        std::fs::write(&file, b"saved again").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"saved again");
        directory.close().unwrap();
        assert!(!path.exists());
    }

    static OPENED: AtomicUsize = AtomicUsize::new(0);
    fn record_open(_: &Path) -> Result<(), String> {
        OPENED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn refuse_open(_: &Path) -> Result<(), String> {
        Err("no default app".into())
    }

    struct Worker {
        queue: mpsc::SyncSender<Request>,
        requests: mpsc::Receiver<Request>,
    }
    impl Worker {
        fn new() -> Self {
            let (queue, requests) = mpsc::sync_channel(1);
            Self { queue, requests }
        }
        fn submit(&self) -> impl FnMut(Action) -> Result<ActionTask, String> + '_ {
            |action| submit_action(&self.queue, action)
        }
        fn next(&self) -> Request {
            self.requests.try_recv().expect("a submitted request")
        }
        fn idle(&self) -> bool {
            self.requests.try_recv().is_err()
        }
    }
    const LISTED: RemoteFingerprint = RemoteFingerprint {
        size: 2,
        modified: 100,
    };

    fn downloaded(worker: &Worker, opener: fn(&Path) -> Result<(), String>) -> EditSession {
        let mut edit = EditSession::start_with(
            "/srv/app/config.yml",
            LISTED,
            false,
            worker.submit(),
            opener,
        )
        .unwrap();
        let request = worker.next();
        let Some(Action::EditDownload {
            remote,
            local,
            expected,
            scp,
        }) = request.action().cloned()
        else {
            panic!("expected an edit download")
        };
        assert_eq!(remote, "/srv/app/config.yml");
        assert_eq!(local, edit.local());
        assert_eq!(expected, LISTED);
        assert!(!scp);
        assert_eq!(local.file_name().unwrap(), "config.yml");
        assert_eq!(edit.status(Instant::now()), Status::Downloading);
        std::fs::write(&local, b"v1").unwrap();
        request.complete(Ok(()), None, false);
        edit.poll(Instant::now(), true, &mut worker.submit());
        edit
    }

    /// Save new contents and poll twice, two seconds apart.
    fn save_and_settle(edit: &mut EditSession, worker: &Worker, at: &mut Instant, bytes: &[u8]) {
        std::fs::write(edit.local(), bytes).unwrap();
        *at += POLL_INTERVAL;
        edit.poll(*at, true, &mut worker.submit());
        assert!(worker.idle(), "uploaded before the save settled");
        assert_eq!(edit.status(*at), Status::Pending);
        *at += POLL_INTERVAL;
        edit.poll(*at, true, &mut worker.submit());
    }

    #[test]
    fn session_downloads_opens_syncs_and_stops_on_conflict() {
        let worker = Worker::new();
        let before = OPENED.load(Ordering::SeqCst);
        let mut edit = downloaded(&worker, record_open);
        assert!(OPENED.load(Ordering::SeqCst) > before);
        assert_eq!(edit.note(), None);
        let mut at = Instant::now();
        assert_eq!(edit.status(at), Status::Synced(None));
        assert!(!edit.keeps_copy());
        // Unchanged file: nothing is uploaded.
        at += POLL_INTERVAL * 2;
        edit.poll(at, true, &mut worker.submit());
        assert!(worker.idle());

        save_and_settle(&mut edit, &worker, &mut at, b"version two");
        let request = worker.next();
        let Some(Action::EditReplace {
            local,
            remote,
            expected,
            ..
        }) = request.action().cloned()
        else {
            panic!("expected an edit replacement")
        };
        assert_eq!(
            (local.as_path(), remote.as_str()),
            (edit.local(), "/srv/app/config.yml")
        );
        assert_eq!(expected, LISTED);
        assert_eq!(edit.status(at), Status::Uploading);
        assert!(edit.keeps_copy());
        let synced = RemoteFingerprint {
            size: 11,
            modified: 200,
        };
        request.complete(Ok(()), Some(synced), false);
        edit.poll(at, true, &mut worker.submit());
        assert_eq!(
            edit.status(at + Duration::from_secs(3)),
            Status::Synced(Some(Duration::from_secs(3)))
        );
        assert!(!edit.keeps_copy());

        // The next save must match the fingerprint left by the last sync;
        // the server reports that someone else changed the file.
        save_and_settle(&mut edit, &worker, &mut at, b"version three");
        let request = worker.next();
        let Some(Action::EditReplace { expected, .. }) = request.action().cloned() else {
            panic!("expected an edit replacement")
        };
        assert_eq!(expected, synced);
        request.complete(Err("changed".into()), None, true);
        edit.poll(at, true, &mut worker.submit());
        let status = edit.status(at);
        assert!(status.is_error());
        let detail = status.detail().unwrap().to_owned();
        assert!(detail.contains("changed on the server"), "{detail}");
        assert!(
            detail.contains(&edit.local().display().to_string()),
            "{detail}"
        );
        assert!(!edit.can_retry());
        // Conflicts stop syncing for good.
        std::fs::write(edit.local(), b"version four!").unwrap();
        for _ in 0..3 {
            at += POLL_INTERVAL;
            edit.poll(at, true, &mut worker.submit());
        }
        assert!(worker.idle());
        let folder = edit.local().parent().unwrap().to_owned();
        let local = edit.local().to_owned();
        let message = edit.stop();
        assert!(message.contains(&local.display().to_string()), "{message}");
        assert_eq!(std::fs::read(&local).unwrap(), b"version four!");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn busy_queue_disconnects_and_failures_are_retried_only_on_request() {
        let worker = Worker::new();
        let mut edit = downloaded(&worker, refuse_open);
        assert_eq!(edit.note(), Some("no default app"));
        let folder = edit.local().parent().unwrap().to_owned();
        let mut at = Instant::now();
        std::fs::write(edit.local(), b"edited").unwrap();
        // Disconnected: never submits.
        for _ in 0..3 {
            at += POLL_INTERVAL;
            edit.poll(at, false, &mut worker.submit());
        }
        assert!(worker.idle());
        // Busy file tools: the settled save waits and is retried.
        for _ in 0..2 {
            at += POLL_INTERVAL;
            edit.poll(at, true, &mut |_| {
                Err("File tools are busy or disconnected.".into())
            });
        }
        assert!(edit.note().unwrap().contains("busy"));
        at += POLL_INTERVAL;
        edit.poll(at, true, &mut worker.submit());
        let request = worker.next();
        assert!(matches!(request.action(), Some(Action::EditReplace { .. })));
        // A transport failure with an unchanged local copy stops syncing.
        request.complete(Err("Connection lost.".into()), None, false);
        edit.poll(at, true, &mut worker.submit());
        assert!(edit
            .status(at)
            .detail()
            .unwrap()
            .starts_with("Connection lost."));
        assert!(edit.can_retry());
        at += POLL_INTERVAL;
        edit.poll(at, true, &mut worker.submit());
        assert!(worker.idle());
        edit.retry(at);
        edit.poll(at, true, &mut worker.submit());
        assert!(worker.idle());
        at += POLL_INTERVAL;
        edit.poll(at, true, &mut worker.submit());
        let request = worker.next();
        // A failure caused by a newer local save goes back to watching.
        std::fs::write(edit.local(), b"edited again").unwrap();
        request.complete(Err("Local file changed during upload.".into()), None, false);
        edit.poll(at, true, &mut worker.submit());
        assert!(!edit.status(at).is_error());
        assert!(edit.note().unwrap().contains("newer save"));
        // Dropping a session with unsynced edits keeps them.
        let local = edit.local().to_owned();
        drop(edit);
        assert_eq!(std::fs::read(&local).unwrap(), b"edited again");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_upload_that_may_have_replaced_the_file_stops_instead_of_retrying() {
        let worker = Worker::new();
        let mut edit = downloaded(&worker, refuse_open);
        let folder = edit.local().parent().unwrap().to_owned();
        let mut at = Instant::now();
        std::fs::write(edit.local(), b"edited").unwrap();
        for _ in 0..2 {
            at += POLL_INTERVAL;
            edit.poll(at, true, &mut worker.submit());
        }
        let request = worker.next();
        assert!(matches!(request.action(), Some(Action::EditReplace { .. })));
        // Saved again while an upload whose outcome is unknown failed: a retry
        // against the old server fingerprint would report a false conflict.
        std::fs::write(edit.local(), b"edited again").unwrap();
        request.complete_uncertain("Upload publication was not confirmed.".into());
        edit.poll(at, true, &mut worker.submit());
        assert!(edit.status(at).is_error());
        assert!(edit.can_retry());
        for _ in 0..3 {
            at += POLL_INTERVAL;
            edit.poll(at, true, &mut worker.submit());
        }
        assert!(worker.idle(), "uploaded against a stale fingerprint");
        drop(edit);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn clean_sessions_and_failed_downloads_remove_their_folder() {
        let worker = Worker::new();
        let edit = downloaded(&worker, record_open);
        let folder = edit.local().parent().unwrap().to_owned();
        assert!(folder.is_dir());
        assert!(edit.stop().contains("removed"));
        assert!(!folder.exists());

        let mut edit =
            EditSession::start_with("/srv/x.txt", LISTED, true, worker.submit(), record_open)
                .unwrap();
        let request = worker.next();
        assert!(matches!(
            request.action(),
            Some(Action::EditDownload { scp: true, .. })
        ));
        let folder = edit.local().parent().unwrap().to_owned();
        request.complete(
            Err("The remote file changed after the folder was listed.".into()),
            None,
            false,
        );
        edit.poll(Instant::now(), true, &mut worker.submit());
        assert!(edit.status(Instant::now()).is_error());
        assert!(!edit.is_open());
        assert!(!edit.can_retry());
        drop(edit);
        assert!(!folder.exists());

        for remote in ["/srv/", "/srv/..", ""] {
            assert!(
                EditSession::start_with(remote, LISTED, false, worker.submit(), record_open)
                    .is_err()
            );
        }
        // A refused submission leaves no folder behind.
        assert!(EditSession::start_with(
            "/srv/y.txt",
            LISTED,
            false,
            |_| Err("busy".into()),
            record_open
        )
        .is_err());
    }

    #[test]
    fn a_copy_missing_mid_save_is_kept_and_stopping_a_download_cancels_it() {
        let worker = Worker::new();
        let edit = downloaded(&worker, record_open);
        let folder = edit.local().parent().unwrap().to_owned();
        // An editor's atomic save has moved the original aside and not yet
        // put the new file in place: stopping must not delete the folder.
        let aside = folder.join("config.yml.tmp");
        std::fs::rename(edit.local(), &aside).unwrap();
        assert!(edit.keeps_copy());
        let message = edit.stop();
        assert!(aside.exists(), "{message}");
        std::fs::remove_dir_all(&folder).unwrap();

        for stop in [true, false] {
            let edit = EditSession::start_with(
                "/srv/big.log",
                LISTED,
                false,
                worker.submit(),
                record_open,
            )
            .unwrap();
            let request = worker.next();
            let folder = edit.local().parent().unwrap().to_owned();
            std::fs::write(folder.join(".partial"), b"part").unwrap();
            if stop {
                let message = edit.stop();
                assert!(message.contains("removed"), "{message}");
            } else {
                drop(edit);
            }
            assert!(request.cancelled());
            assert!(!folder.exists());
        }
    }

    #[test]
    fn status_labels_are_short_and_errors_carry_details() {
        assert_eq!(
            Status::Synced(Some(Duration::from_secs(75))).label(),
            "Synced 1 min ago"
        );
        assert_eq!(
            Status::Synced(Some(Duration::from_secs(7300))).label(),
            "Synced 2 h 1 min ago"
        );
        assert_eq!(
            Status::Synced(Some(Duration::from_secs(5))).label(),
            "Synced 5 s ago"
        );
        assert!(!Status::Uploading.is_error());
        assert_eq!(Status::Conflict("why").detail(), Some("why"));
        assert_eq!(Status::Pending.detail(), None);
    }
}
