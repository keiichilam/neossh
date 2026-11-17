//! Session-owned, incremental SFTP v3 directory requests.
mod edit;
mod scp;
mod scp_download;
mod shell;
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub modified: Option<u32>,
}
impl Entry {
    pub fn is_directory(&self) -> bool {
        self.permissions
            .is_some_and(|mode| mode & 0o170000 == 0o040000)
    }
}
/// Size and whole-second modification time of a remote regular file. Edit
/// sessions replace a remote file only while it still matches the fingerprint
/// recorded when it was listed for download or last synced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteFingerprint {
    pub size: u64,
    pub modified: u32,
}
impl RemoteFingerprint {
    /// Fingerprint of a listed regular file; `None` when the server omitted
    /// the type, size or modification time needed to detect later changes.
    pub fn of(entry: &Entry) -> Option<Self> {
        let regular = entry
            .permissions
            .is_some_and(|mode| mode & 0o170000 == 0o100000);
        Some(Self {
            size: entry.size?,
            modified: entry.modified?,
        })
        .filter(|_| regular)
    }
    fn matches(&self, entry: &Entry) -> bool {
        entry.size == Some(self.size) && entry.modified == Some(self.modified)
    }
}
#[derive(Debug)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}
pub struct Task<T> {
    result: mpsc::Receiver<Result<T, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<Progress>,
}
struct Progress {
    bytes: AtomicU64,
    total: AtomicU64,
    scp_fallback: AtomicBool,
    /// Edit replacement refused because the server copy changed.
    conflict: AtomicBool,
    /// The task failed after the destination may already have been replaced
    /// (its confirmation was lost, or it was stopped mid-replacement).
    uncertain: AtomicBool,
    /// Edit replacement: fingerprint of the file now at the destination.
    fingerprint: Mutex<Option<RemoteFingerprint>>,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            bytes: AtomicU64::new(0),
            total: AtomicU64::new(u64::MAX),
            scp_fallback: AtomicBool::new(false),
            conflict: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
            fingerprint: Mutex::new(None),
        }
    }
}
pub type DirectoryTask = Task<Listing>;
pub type ActionTask = Task<()>;
impl<T> Task<T> {
    pub fn used_scp_fallback(&self) -> bool {
        self.progress.scp_fallback.load(Ordering::Acquire)
    }
    /// For [`Action::EditReplace`]: the server copy no longer matched the
    /// expected fingerprint, so nothing was replaced.
    pub fn remote_conflict(&self) -> bool {
        self.progress.conflict.load(Ordering::Acquire)
    }
    /// A failed task may still have replaced its destination, so the server's
    /// current state is unknown until it is checked.
    pub fn outcome_uncertain(&self) -> bool {
        self.progress.uncertain.load(Ordering::Acquire)
    }
    /// For a successful [`Action::EditReplace`]: the replaced file's fingerprint.
    pub fn remote_fingerprint(&self) -> Option<RemoteFingerprint> {
        *self
            .progress
            .fingerprint
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn bytes_transferred(&self) -> u64 {
        self.progress.bytes.load(Ordering::Relaxed)
    }
    pub fn total_bytes(&self) -> Option<u64> {
        let total = self.progress.total.load(Ordering::Relaxed);
        (total != u64::MAX).then_some(total)
    }
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("File request ended with the session.".into())),
        }
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
pub(crate) struct Request {
    path: String,
    action: Option<Action>,
    answer: Reply,
    cancel: Arc<AtomicBool>,
    progress: Arc<Progress>,
}
#[derive(Clone, Debug)]
pub enum Action {
    DownloadScp {
        remote: String,
        local: std::path::PathBuf,
    },
    UploadScp {
        local: std::path::PathBuf,
        remote: String,
    },
    CreateFile {
        path: String,
    },
    SetPermissions {
        path: String,
        mode: u32,
    },
    Upload {
        local: std::path::PathBuf,
        remote: String,
    },
    Download {
        remote: String,
        local: std::path::PathBuf,
    },
    CreateDirectory {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    RemoveFile {
        path: String,
    },
    RemoveDirectory {
        path: String,
    },
    /// Download a file for an edit session into a new local path, refusing it
    /// when the server copy no longer matches the reviewed listing.
    EditDownload {
        remote: String,
        local: std::path::PathBuf,
        expected: RemoteFingerprint,
        scp: bool,
    },
    /// Edit sessions only: atomically REPLACE `remote` with `local`.
    ///
    /// This is the single, explicit exception to the rule that transfers never
    /// overwrite: the user reviewed "saving replaces the remote file" when the
    /// edit session started. The bytes go to a unique staging file in the same
    /// folder, which gets the original permission bits and is then renamed over
    /// the original (SFTP posix-rename, or `mv -f -T` through the shell/SCP
    /// backends) only if the server copy still matches `expected`. Otherwise
    /// nothing is replaced and [`Task::remote_conflict`] reports the conflict.
    EditReplace {
        local: std::path::PathBuf,
        remote: String,
        expected: RemoteFingerprint,
        scp: bool,
    },
}
enum Response {
    Directory(Listing),
    Done,
    /// SFTP wrote and verified the staging file, but the server lacks
    /// posix-rename; publish it with the shell backend.
    ShellRename {
        staging: String,
        destination: String,
        expected: RemoteFingerprint,
    },
}
enum Reply {
    Directory(mpsc::SyncSender<Result<Listing, String>>),
    Action(mpsc::SyncSender<Result<(), String>>),
}
impl Reply {
    fn send(&self, result: Result<Response, String>) {
        match self {
            Self::Directory(sender) => {
                let _ = sender.try_send(result.and_then(|value| match value {
                    Response::Directory(listing) => Ok(listing),
                    _ => Err("Unexpected file response.".into()),
                }));
            }
            Self::Action(sender) => {
                let _ = sender.try_send(result.and_then(|value| match value {
                    Response::Done => Ok(()),
                    _ => Err("Unexpected file response.".into()),
                }));
            }
        }
    }
}
pub(crate) fn submit_action(
    queue: &mpsc::SyncSender<Request>,
    action: Action,
) -> Result<ActionTask, String> {
    match &action {
        Action::SetPermissions { path, mode } => {
            validate_path(path)?;
            if *mode > 0o7777 {
                return Err("Permissions must be between 0000 and 7777 octal.".into());
            }
        }
        Action::Download { remote, local }
        | Action::DownloadScp { remote, local }
        | Action::Upload { remote, local }
        | Action::UploadScp { remote, local }
        | Action::EditDownload { remote, local, .. }
        | Action::EditReplace { remote, local, .. } => {
            validate_path(remote)?;
            if !local.is_absolute() || local.file_name().is_none() {
                return Err("Choose an absolute local file path.".into());
            }
        }
        Action::Rename { from, to } => {
            validate_path(from)?;
            validate_path(to)?;
            if from == to {
                return Err("Choose a different destination name.".into());
            }
        }
        Action::CreateFile { path }
        | Action::CreateDirectory { path }
        | Action::RemoveFile { path }
        | Action::RemoveDirectory { path } => validate_path(path)?,
    }
    let (answer, result) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Progress::default());
    queue
        .try_send(Request {
            path: String::new(),
            action: Some(action),
            progress: progress.clone(),
            answer: Reply::Action(answer),
            cancel: cancel.clone(),
        })
        .map_err(|_| "File tools are busy or disconnected.".to_owned())?;
    Ok(Task {
        result,
        cancel,
        progress,
    })
}
pub(crate) fn submit(
    queue: &mpsc::SyncSender<Request>,
    path: String,
) -> Result<DirectoryTask, String> {
    validate_path(&path)?;
    let (answer, result) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Progress::default());
    queue
        .try_send(Request {
            path,
            action: None,
            progress: progress.clone(),
            answer: Reply::Directory(answer),
            cancel: cancel.clone(),
        })
        .map_err(|_| "File tools are busy or disconnected.".to_owned())?;
    Ok(Task {
        result,
        cancel,
        progress,
    })
}
fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err("Remote path must contain 1–4096 bytes and no NUL characters.".into());
    }
    Ok(())
}
// The vendored transport uses this local error for SSH_MSG_CHANNEL_FAILURE.
// Match the typed transport error, never server-supplied SFTP status text.
fn subsystem_denied(error: &ssh::SshError) -> bool {
    matches!(error, ssh::SshError::GeneralError(message) if message == "channel failure.")
}
pub(crate) struct Worker<S: Read + Write> {
    queue: mpsc::Receiver<Request>,
    active: Option<(Request, Job<S>)>,
    scp: Option<(Request, scp::Job<S>)>,
    scp_download: Option<(Request, scp_download::Job<S>)>,
    shell: Option<(Request, shell::Job<S>)>,
    edit_rename: Option<(Request, edit::RenameJob<S>)>,
}
impl<S: Read + Write> Worker<S> {
    /// Start the SCP backend for a transfer; if it cannot start, answer the
    /// request with the reason, prefixed by `context` when given.
    fn start_scp(
        &mut self,
        session: &mut ssh::LocalSession<S>,
        request: Request,
        context: Option<&str>,
    ) {
        let fail = |request: Request, error: String| {
            request.answer.send(Err(match context {
                Some(context) => format!("{context}: {error}"),
                None => error,
            }))
        };
        let progress = request.progress.clone();
        let cancel = request.cancel.clone();
        let (local, remote, expected) = match &request.action {
            Some(
                Action::Upload { local, remote }
                | Action::UploadScp { local, remote }
                | Action::Download { local, remote }
                | Action::DownloadScp { local, remote },
            ) => (local.clone(), remote.clone(), None),
            Some(
                Action::EditReplace {
                    local,
                    remote,
                    expected,
                    ..
                }
                | Action::EditDownload {
                    local,
                    remote,
                    expected,
                    ..
                },
            ) => (local.clone(), remote.clone(), Some(*expected)),
            _ => return fail(request, "Not an SCP transfer.".into()),
        };
        match &request.action {
            Some(Action::Upload { .. } | Action::UploadScp { .. }) => {
                match scp::Job::new(session, &local, remote, progress, cancel) {
                    Ok(job) => self.scp = Some((request, job)),
                    Err(error) => fail(request, error),
                }
            }
            Some(Action::EditReplace { .. }) => {
                let expected = expected.expect("edit replacement fingerprint");
                match scp::Job::replace(session, &local, remote, expected, progress, cancel) {
                    Ok(job) => self.scp = Some((request, job)),
                    Err(error) => fail(request, error),
                }
            }
            _ => {
                match scp_download::Job::new(session, local, &remote, expected, progress, cancel) {
                    Ok(job) => self.scp_download = Some((request, job)),
                    Err(error) => fail(request, error),
                }
            }
        }
    }
    fn fallback(&mut self, session: &mut ssh::LocalSession<S>, request: Request, error: String) {
        if request.cancel.load(Ordering::Acquire) {
            request.answer.send(Err("File request cancelled.".into()));
            return;
        }
        let transfer = match &request.action {
            Some(Action::Upload { .. }) => Some("SFTP unavailable; SCP upload failed"),
            Some(Action::EditReplace { .. }) => Some("SFTP unavailable; SCP edit upload failed"),
            Some(Action::Download { .. } | Action::EditDownload { .. }) => {
                Some("SFTP unavailable; SCP download failed")
            }
            _ => None,
        };
        if let Some(context) = transfer {
            request.progress.scp_fallback.store(true, Ordering::Release);
            self.start_scp(session, request, Some(context));
            return;
        }
        match shell::Job::new(
            session,
            &request.path,
            request.action.as_ref(),
            request.cancel.clone(),
        ) {
            Ok(job) => self.shell = Some((request, job)),
            Err(cause) => request
                .answer
                .send(Err(format!("{error}; shell fallback failed: {cause}"))),
        }
    }
    pub fn new(queue: mpsc::Receiver<Request>) -> Self {
        Self {
            queue,
            active: None,
            scp: None,
            scp_download: None,
            shell: None,
            edit_rename: None,
        }
    }
    pub fn has_pending_work(&self) -> bool {
        self.active.is_some()
            || self.scp.is_some()
            || self.scp_download.is_some()
            || self.shell.is_some()
            || self.edit_rename.is_some()
    }
    pub fn poll(&mut self, session: &mut ssh::LocalSession<S>) {
        if let Some((request, job)) = &mut self.shell {
            match job.poll() {
                Ok(None) => {}
                Ok(Some(response)) => {
                    request.answer.send(Ok(response));
                    self.shell = None;
                }
                Err(error) => {
                    request.answer.send(Err(error));
                    self.shell = None;
                }
            }
            return;
        }
        if let Some((request, job)) = &mut self.edit_rename {
            match job.poll() {
                Ok(false) => {}
                Ok(true) => {
                    request.answer.send(Ok(Response::Done));
                    self.edit_rename = None;
                }
                Err(error) => {
                    request.answer.send(Err(error));
                    self.edit_rename = None;
                }
            }
            return;
        }
        if let Some((request, job)) = &mut self.scp_download {
            match job.poll() {
                Ok(false) => {}
                Ok(true) => {
                    request.answer.send(Ok(Response::Done));
                    self.scp_download = None;
                }
                Err(error) => {
                    request.answer.send(Err(error));
                    self.scp_download = None;
                }
            }
            return;
        }
        if let Some((request, job)) = &mut self.scp {
            match job.poll() {
                Ok(false) => {}
                Ok(true) => {
                    request.answer.send(Ok(Response::Done));
                    self.scp = None;
                }
                Err(error) => {
                    request.answer.send(Err(error));
                    self.scp = None;
                }
            }
            return;
        }
        if self.active.is_none() {
            if let Ok(request) = self.queue.try_recv() {
                if request.cancel.load(Ordering::Acquire) {
                    return;
                }
                if matches!(
                    request.action,
                    Some(
                        Action::UploadScp { .. }
                            | Action::DownloadScp { .. }
                            | Action::EditReplace { scp: true, .. }
                            | Action::EditDownload { scp: true, .. }
                    )
                ) {
                    self.start_scp(session, request, None);
                    return;
                }
                match session.open_sftp() {
                    Ok(channel) => {
                        let job = Job::new(
                            channel,
                            request.path.clone(),
                            request.action.clone(),
                            request.progress.clone(),
                            request.cancel.clone(),
                        );
                        self.active = Some((request, job));
                    }
                    Err(error) => {
                        if subsystem_denied(&error) {
                            self.fallback(session, request, format!("Cannot open SFTP: {error}"));
                        } else {
                            request
                                .answer
                                .send(Err(format!("Cannot open SFTP: {error}")));
                        }
                    }
                }
            }
        }
        let Some((request, job)) = &mut self.active else {
            return;
        };
        if request.cancel.load(Ordering::Acquire) && job.upload.is_none() {
            request.answer.send(Err("File request cancelled.".into()));
            self.active = None;
            return;
        }
        let result = job.poll();
        if job.subsystem_unavailable && result.is_err() {
            let (request, job) = self.active.take().unwrap();
            drop(job);
            self.fallback(session, request, result.err().unwrap());
            return;
        }
        match result {
            Ok(None) => {}
            Ok(Some(Response::ShellRename {
                staging,
                destination,
                expected,
            })) => {
                let (request, job) = self.active.take().unwrap();
                drop(job);
                match edit::RenameJob::new(
                    session,
                    staging.clone(),
                    &destination,
                    expected,
                    request.progress.clone(),
                    request.cancel.clone(),
                ) {
                    Ok(job) => self.edit_rename = Some((request, job)),
                    Err(error) => request.answer.send(Err(format!(
                        "SFTP alone cannot replace this file safely (no posix-rename, or its owner must be kept) and the shell replacement could not start: {error}. Remote temporary file may remain at {staging:?}."
                    ))),
                }
            }
            Ok(Some(listing)) => {
                request.answer.send(Ok(listing));
                self.active = None;
            }
            Err(error) => {
                request.answer.send(Err(error));
                self.active = None;
            }
        }
    }
}
#[derive(PartialEq)]
enum Stage {
    CreateFileOpen,
    Version,
    Canonical,
    Open,
    Read,
    Close,
    Action,
    DownloadOpen,
    DownloadStat,
    DownloadRead,
    DownloadClose,
    UploadOpen,
    UploadWrite,
    UploadClose,
    UploadRename,
    UploadCleanupClose,
    UploadCleanupRemove,
    ReplaceStagingStat,
    ReplaceCheck,
    ReplaceMode,
}
struct Job<S: Read + Write> {
    subsystem_unavailable: bool,
    /// The server advertised `posix-rename@openssh.com` (rename(2) semantics).
    posix_rename: bool,
    /// Edit replacement: the fingerprint the destination must still have.
    replace: Option<RemoteFingerprint>,
    /// Edit download: the reviewed fingerprint the file must still have.
    download_expected: Option<RemoteFingerprint>,
    /// Edit replacement: the uploaded copy's owner and group, if reported.
    staging_owner: Option<(u32, u32)>,
    /// Edit replacement: SETSTAT also asked for the original owner and group.
    owner_change: bool,
    upload: Option<crate::upload::Upload>,
    upload_handle_open: bool,
    abort_error: Option<String>,
    response_received: bool,
    download: Option<crate::download::StagedDownload>,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    channel: ssh::LocalSftp<S>,
    stage: Stage,
    id: u32,
    outgoing: Vec<u8>,
    sent: usize,
    path: String,
    action: Option<Action>,
    handle: Vec<u8>,
    entries: Vec<Entry>,
    deadline: Instant,
}
impl<S: Read + Write> Drop for Job<S> {
    fn drop(&mut self) {
        self.channel.close_now();
    }
}
impl<S: Read + Write> Job<S> {
    fn new(
        channel: ssh::LocalSftp<S>,
        path: String,
        action: Option<Action>,
        progress: Arc<Progress>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            upload: None,
            subsystem_unavailable: false,
            posix_rename: false,
            replace: None,
            download_expected: None,
            staging_owner: None,
            owner_change: false,
            upload_handle_open: false,
            abort_error: None,
            response_received: false,
            download: None,
            progress,
            cancel,
            channel,
            stage: Stage::Version,
            id: 0,
            outgoing: vec![0, 0, 0, 5, 1, 0, 0, 0, 3],
            sent: 0,
            path,
            action,
            handle: Vec::new(),
            entries: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
    fn request(&mut self, kind: u8, value: &[u8], stage: Stage) {
        self.id = self.id.wrapping_add(1);
        let mut body = vec![kind];
        body.extend_from_slice(&self.id.to_be_bytes());
        body.extend_from_slice(&(value.len() as u32).to_be_bytes());
        body.extend_from_slice(value);
        self.outgoing = (body.len() as u32).to_be_bytes().to_vec();
        self.outgoing.extend(body);
        self.sent = 0;
        self.stage = stage;
    }
    fn action_request(&mut self, action: Action) -> Result<(), String> {
        let (kind, first, second) = match action {
            Action::UploadScp { .. } | Action::DownloadScp { .. } => {
                return Err("SCP transfer reached the SFTP worker.".into())
            }
            Action::CreateFile { path } => {
                self.request(3, path.as_bytes(), Stage::CreateFileOpen);
                self.outgoing.extend_from_slice(&0x2au32.to_be_bytes());
                self.outgoing.extend_from_slice(&4u32.to_be_bytes());
                self.outgoing.extend_from_slice(&0o600u32.to_be_bytes());
                self.reframe();
                return Ok(());
            }
            Action::SetPermissions { path, mode } => {
                self.request(9, path.as_bytes(), Stage::Action);
                self.outgoing.extend_from_slice(&4u32.to_be_bytes());
                self.outgoing.extend_from_slice(&mode.to_be_bytes());
                self.reframe();
                return Ok(());
            }
            Action::Upload { local, remote } => {
                self.start_upload(crate::upload::Upload::new(&local, remote)?);
                return Ok(());
            }
            Action::EditReplace {
                local,
                remote,
                expected,
                ..
            } => {
                self.replace = Some(expected);
                self.start_upload(crate::upload::Upload::for_replace(&local, remote)?);
                return Ok(());
            }
            Action::Download { remote, local } => return self.start_download(remote, local),
            Action::EditDownload {
                remote,
                local,
                expected,
                ..
            } => {
                self.download_expected = Some(expected);
                return self.start_download(remote, local);
            }
            Action::CreateDirectory { path } => (14, path, None),
            Action::Rename { from, to } => (18, from, Some(to)),
            Action::RemoveFile { path } => (13, path, None),
            Action::RemoveDirectory { path } => (15, path, None),
        };
        self.request(kind, first.as_bytes(), Stage::Action);
        if let Some(second) = second {
            self.outgoing
                .extend_from_slice(&(second.len() as u32).to_be_bytes());
            self.outgoing.extend_from_slice(second.as_bytes());
        } else if kind == 14 {
            self.outgoing.extend_from_slice(&4u32.to_be_bytes()); // permissions attribute
            self.outgoing.extend_from_slice(&0o755u32.to_be_bytes());
        }
        self.reframe();
        Ok(())
    }
    fn start_upload(&mut self, upload: crate::upload::Upload) {
        self.progress.total.store(upload.total, Ordering::Relaxed);
        self.request(3, upload.staging.as_bytes(), Stage::UploadOpen);
        self.outgoing.extend_from_slice(&0x2au32.to_be_bytes()); // write/create/exclusive
        self.outgoing.extend_from_slice(&4u32.to_be_bytes()); // permissions
        self.outgoing.extend_from_slice(&0o600u32.to_be_bytes());
        self.reframe();
        self.upload = Some(upload);
    }
    fn start_download(&mut self, remote: String, local: std::path::PathBuf) -> Result<(), String> {
        self.download = Some(crate::download::StagedDownload::new(local)?);
        self.request(3, remote.as_bytes(), Stage::DownloadOpen);
        self.outgoing.extend_from_slice(&1u32.to_be_bytes()); // read only
        self.outgoing.extend_from_slice(&0u32.to_be_bytes()); // no attributes
        self.reframe();
        Ok(())
    }
    fn reframe(&mut self) {
        let length = (self.outgoing.len() - 4) as u32;
        self.outgoing[..4].copy_from_slice(&length.to_be_bytes());
    }
    fn read_download(&mut self) {
        self.request(5, &self.handle.clone(), Stage::DownloadRead);
        self.outgoing
            .extend_from_slice(&self.progress.bytes.load(Ordering::Relaxed).to_be_bytes());
        self.outgoing.extend_from_slice(&32768u32.to_be_bytes());
        self.reframe();
    }
    fn poll(&mut self) -> Result<Option<Response>, String> {
        self.response_received = false;
        match self.poll_inner() {
            Err(error) => {
                if self.stage == Stage::UploadOpen && self.sent > 0 && !self.response_received {
                    let staging = &self.upload.as_ref().unwrap().staging;
                    return Err(format!("{error} Creation was not confirmed; a remote temporary file may remain at {staging:?}."));
                }
                if self.upload.as_ref().is_some_and(|upload| upload.created) {
                    if self.response_received && self.abort_error.is_none() {
                        self.abort_error = Some(error);
                        self.cleanup_upload();
                        return Ok(None);
                    }
                    let staging = &self.upload.as_ref().unwrap().staging;
                    if self.stage == Stage::UploadRename && !self.response_received {
                        self.progress.uncertain.store(true, Ordering::Release);
                        return Err(format!("{error} Upload publication was not confirmed. Check destination {:?} and temporary file {staging:?} before retrying.", self.upload.as_ref().unwrap().destination));
                    }
                    return Err(format!("{error} Remote temporary file may remain at {staging:?}; cleanup was not confirmed."));
                }
                Err(error)
            }
            result => result,
        }
    }
    fn cleanup_upload(&mut self) {
        self.deadline = Instant::now() + Duration::from_secs(10);
        if !self.upload_handle_open {
            let staging = self.upload.as_ref().unwrap().staging.clone();
            self.request(13, staging.as_bytes(), Stage::UploadCleanupRemove);
        } else {
            self.request(4, &self.handle.clone(), Stage::UploadCleanupClose);
            self.upload_handle_open = false;
            self.handle.clear();
        }
    }
    fn write_upload(&mut self) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("Upload cancelled.".into());
        }
        let offset = self.progress.bytes.load(Ordering::Relaxed);
        let bytes = self
            .upload
            .as_mut()
            .ok_or("Upload state unavailable.")?
            .read_chunk(offset)?;
        if bytes.is_empty() {
            self.request(4, &self.handle.clone(), Stage::UploadClose);
            self.upload_handle_open = false;
            self.handle.clear();
        } else {
            self.request(6, &self.handle.clone(), Stage::UploadWrite);
            self.outgoing.extend_from_slice(&offset.to_be_bytes());
            self.outgoing
                .extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            self.outgoing.extend_from_slice(&bytes);
            self.reframe();
        }
        Ok(())
    }
    fn poll_inner(&mut self) -> Result<Option<Response>, String> {
        if Instant::now() >= self.deadline {
            return Err("SFTP file request timed out.".into());
        }
        if self.sent < self.outgoing.len() {
            self.sent += self
                .channel
                .try_write(&self.outgoing[self.sent..])
                .map_err(|e| {
                    self.subsystem_unavailable =
                        self.stage == Stage::Version && subsystem_denied(&e);
                    e.to_string()
                })?;
            if self.sent < self.outgoing.len() {
                return Ok(None);
            }
        }
        let Some(packet) = self.channel.try_read_packet().map_err(|e| {
            self.subsystem_unavailable = self.stage == Stage::Version && subsystem_denied(&e);
            format!("SFTP: {e}")
        })?
        else {
            return Ok(None);
        };
        let mut reader = Reader(&packet);
        let kind = reader.byte()?;
        if self.stage == Stage::Version {
            if kind != 2 || reader.u32()? != 3 {
                return Err("Server does not support SFTP version 3.".into());
            }
            while !reader.0.is_empty() {
                let name = reader.string()?;
                let data = reader.string()?;
                if name == b"posix-rename@openssh.com" && data == b"1" {
                    self.posix_rename = true;
                }
            }
            if let Some(action) = self.action.take() {
                self.action_request(action)?;
            } else {
                self.request(16, self.path.clone().as_bytes(), Stage::Canonical);
            }
            return Ok(None);
        }
        if reader.u32()? != self.id {
            return Err("SFTP response has an unexpected request ID.".into());
        }
        self.response_received = true;
        if self.download.is_some() || self.upload.is_some() && self.abort_error.is_none() {
            self.deadline = Instant::now() + Duration::from_secs(30);
        }
        if kind == 101 {
            let code = reader.u32()?;
            let message = reader.text()?;
            reader.string()?;
            reader.end()?;
            if self.stage == Stage::UploadCleanupClose {
                self.cleanup_upload();
                return Ok(None);
            }
            if self.stage == Stage::UploadCleanupRemove {
                if code == 0 || code == 2 {
                    self.upload.as_mut().unwrap().created = false;
                    return Err(format!(
                        "{} Remote temporary file removed.",
                        self.abort_error
                            .take()
                            .unwrap_or_else(|| "Upload cancelled.".into())
                    ));
                }
                return Err(format!(
                    "{} Cleanup failed with SFTP error {code}: {message}.",
                    self.abort_error.as_deref().unwrap_or("Upload failed.")
                ));
            }
            if code == 2 && self.stage == Stage::ReplaceCheck {
                // The edited file was deleted on the server: never recreate it.
                self.progress.conflict.store(true, Ordering::Release);
                return Err(edit::CONFLICT.into());
            }
            if code == 0 && self.stage == Stage::ReplaceMode {
                if self.cancel.load(Ordering::Acquire) {
                    return Err("Upload cancelled.".into());
                }
                let upload = self.upload.as_ref().unwrap();
                let staging = upload.staging.clone();
                let destination = upload.destination.clone();
                if !self.posix_rename {
                    // Publication is handed to the shell backend, which
                    // removes the staging file itself if it cannot replace.
                    self.upload.as_mut().unwrap().created = false;
                    return Ok(Some(Response::ShellRename {
                        staging,
                        destination,
                        expected: self.replace.ok_or("Replacement state unavailable.")?,
                    }));
                }
                // rename(2) semantics: atomically replaces the destination.
                self.request(200, b"posix-rename@openssh.com", Stage::UploadRename);
                for path in [staging, destination] {
                    self.outgoing
                        .extend_from_slice(&(path.len() as u32).to_be_bytes());
                    self.outgoing.extend_from_slice(path.as_bytes());
                }
                self.reframe();
                return Ok(None);
            }
            if code != 0 && self.stage == Stage::ReplaceMode && self.owner_change {
                // The original owner cannot be given to the copy (only root
                // may), so a rename would change who owns the file. The shell
                // backend writes the contents in place instead.
                let upload = self.upload.as_mut().unwrap();
                upload.created = false;
                return Ok(Some(Response::ShellRename {
                    staging: upload.staging.clone(),
                    destination: upload.destination.clone(),
                    expected: self.replace.ok_or("Replacement state unavailable.")?,
                }));
            }
            if code == 0 && self.stage == Stage::UploadWrite {
                let pending = self.upload.as_ref().unwrap().pending;
                self.progress.bytes.fetch_add(pending, Ordering::Relaxed);
                self.write_upload()?;
                return Ok(None);
            }
            if code == 0 && self.stage == Stage::UploadClose {
                if self.cancel.load(Ordering::Acquire) {
                    return Err("Upload cancelled.".into());
                }
                let upload = self.upload.as_ref().unwrap();
                let staging = upload.staging.clone();
                let destination = upload.destination.clone();
                if self.replace.is_some() {
                    // Edit replacement: record what will be published, then
                    // re-check the server copy just before renaming over it.
                    self.request(7, staging.as_bytes(), Stage::ReplaceStagingStat);
                    return Ok(None);
                }
                self.request(18, staging.as_bytes(), Stage::UploadRename);
                self.outgoing
                    .extend_from_slice(&(destination.len() as u32).to_be_bytes());
                self.outgoing.extend_from_slice(destination.as_bytes());
                self.reframe();
                return Ok(None);
            }
            if code == 0 && self.stage == Stage::UploadRename {
                self.upload.as_mut().unwrap().created = false;
                return Ok(Some(Response::Done));
            }
            if code == 1 && self.stage == Stage::DownloadRead {
                let expected = self.progress.total.load(Ordering::Relaxed);
                if expected != u64::MAX && expected != self.progress.bytes.load(Ordering::Relaxed) {
                    return Err("Remote file ended before its reported size.".into());
                }
                self.request(4, &self.handle.clone(), Stage::DownloadClose);
                return Ok(None);
            }
            if code == 0 && self.stage == Stage::DownloadClose {
                if self.cancel.load(Ordering::Acquire) {
                    return Err("Download cancelled.".into());
                }
                self.download
                    .take()
                    .ok_or("Download state unavailable.")?
                    .finish()?;
                return Ok(Some(Response::Done));
            }
            if code == 0 && self.stage == Stage::Action {
                return Ok(Some(Response::Done));
            }
            if code == 1 && self.stage == Stage::Read {
                self.request(4, &self.handle.clone(), Stage::Close);
                return Ok(None);
            }
            if code == 0 && self.stage == Stage::Close {
                sort_entries(&mut self.entries);
                return Ok(Some(Response::Directory(Listing {
                    path: self.path.clone(),
                    entries: std::mem::take(&mut self.entries),
                })));
            }
            return Err(format!("SFTP error {code}: {message}"));
        }
        match self.stage {
            Stage::CreateFileOpen if kind == 102 => {
                let handle = reader.string()?;
                reader.end()?;
                if handle.len() > 256 {
                    return Err("SFTP file handle is too large. Refresh to check whether the file was created.".into());
                }
                self.request(4, handle, Stage::Action);
            }
            Stage::UploadOpen if kind == 102 => {
                self.upload.as_mut().unwrap().created = true;
                self.handle = reader.string()?.to_vec();
                reader.end()?;
                if self.handle.len() > 256 {
                    self.handle.clear();
                    return Err("SFTP file handle is too large.".into());
                }
                self.upload_handle_open = true;
                self.write_upload()?;
            }
            Stage::DownloadOpen if kind == 102 => {
                self.handle = reader.string()?.to_vec();
                reader.end()?;
                if self.handle.len() > 256 {
                    return Err("SFTP file handle is too large.".into());
                }
                self.request(8, &self.handle.clone(), Stage::DownloadStat);
            }
            Stage::DownloadStat if kind == 105 => {
                let entry = attributes(&mut reader)?;
                reader.end()?;
                downloadable(&entry)?;
                if self
                    .download_expected
                    .is_some_and(|expected| !expected.matches(&entry))
                {
                    return Err("The remote file changed after the folder was listed. Refresh and try again.".into());
                }
                self.progress
                    .total
                    .store(entry.size.unwrap_or(u64::MAX), Ordering::Relaxed);
                self.read_download();
            }
            Stage::ReplaceStagingStat if kind == 105 => {
                let (entry, owner) = attributes_with_owner(&mut reader)?;
                reader.end()?;
                self.staging_owner = owner;
                let (Some(size), Some(modified)) = (entry.size, entry.modified) else {
                    return Err(
                        "The server did not report the uploaded file's size and modification time."
                            .into(),
                    );
                };
                *self
                    .progress
                    .fingerprint
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) =
                    Some(RemoteFingerprint { size, modified });
                let destination = self.upload.as_ref().unwrap().destination.clone();
                // LSTAT: a symbolic link in place of the file is a conflict.
                self.request(7, destination.as_bytes(), Stage::ReplaceCheck);
            }
            Stage::ReplaceCheck if kind == 105 => {
                let (entry, owner) = attributes_with_owner(&mut reader)?;
                reader.end()?;
                let Some(mode) = entry.permissions else {
                    return Err(
                        "The server did not report the remote file's type and permissions.".into(),
                    );
                };
                let expected = self.replace.ok_or("Replacement state unavailable.")?;
                if mode & 0o170000 != 0o100000 || !expected.matches(&entry) {
                    self.progress.conflict.store(true, Ordering::Release);
                    return Err(edit::CONFLICT.into());
                }
                // Keep the original permission bits; drop setuid/setgid as an
                // in-place write by an unprivileged user would. A rename
                // would also hand the file to the uploading account, so ask
                // for the original owner and group when they differ.
                let staging = self.upload.as_ref().unwrap().staging.clone();
                self.request(9, staging.as_bytes(), Stage::ReplaceMode);
                let change = owner
                    .filter(|&owner| self.staging_owner.is_some_and(|staging| staging != owner));
                self.owner_change = change.is_some();
                if let Some((uid, gid)) = change {
                    self.outgoing.extend_from_slice(&6u32.to_be_bytes()); // uid/gid, permissions
                    self.outgoing.extend_from_slice(&uid.to_be_bytes());
                    self.outgoing.extend_from_slice(&gid.to_be_bytes());
                } else {
                    self.outgoing.extend_from_slice(&4u32.to_be_bytes()); // permissions
                }
                self.outgoing
                    .extend_from_slice(&(mode & 0o777).to_be_bytes());
                self.reframe();
            }
            Stage::DownloadRead if kind == 103 => {
                let data = reader.string()?;
                reader.end()?;
                if data.is_empty() || data.len() > 32768 {
                    return Err("Invalid SFTP download data length.".into());
                }
                let download = self
                    .download
                    .as_mut()
                    .ok_or("Download state unavailable.")?;
                if download.bytes.saturating_add(data.len() as u64)
                    > self.progress.total.load(Ordering::Relaxed)
                {
                    return Err("Remote file grew beyond its reported size.".into());
                }
                download.append(data)?;
                self.progress.bytes.store(download.bytes, Ordering::Relaxed);
                self.read_download();
            }
            Stage::Canonical if kind == 104 => {
                let entries = names(&mut reader)?;
                if entries.len() != 1 {
                    return Err("SFTP returned an invalid canonical path.".into());
                }
                self.path = entries[0].name.clone();
                validate_path(&self.path)?;
                self.request(11, self.path.clone().as_bytes(), Stage::Open);
            }
            Stage::Open if kind == 102 => {
                self.handle = reader.string()?.to_vec();
                reader.end()?;
                if self.handle.len() > 256 {
                    return Err("SFTP directory handle is too large.".into());
                }
                self.request(12, &self.handle.clone(), Stage::Read);
            }
            Stage::Read if kind == 104 => {
                let entries = names(&mut reader)?;
                if self.entries.len() + entries.len() > 10_000 {
                    return Err("Directory exceeds the 10,000-entry browsing limit.".into());
                }
                for entry in entries {
                    if entry.name == "." || entry.name == ".." {
                        continue;
                    }
                    if entry.name.contains('/')
                        || entry.name.contains('\0')
                        || entry.name.is_empty()
                    {
                        return Err("SFTP returned an invalid directory entry name.".into());
                    }
                    self.entries.push(entry);
                }
                self.request(12, &self.handle.clone(), Stage::Read);
            }
            _ => return Err("Unexpected SFTP response type.".into()),
        }
        Ok(None)
    }
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let value = self.0.get(..n).ok_or("Truncated SFTP response.")?;
        self.0 = &self.0[n..];
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn text(&mut self) -> Result<String, String> {
        let bytes = self.string()?;
        if bytes.len() > 4096 {
            return Err("SFTP name or message exceeds 4096 bytes.".into());
        }
        String::from_utf8(bytes.to_vec())
            .map_err(|_| "SFTP returned a name that is not UTF-8.".into())
    }
    fn end(&self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("Trailing SFTP response data.".into())
        }
    }
}
fn names(reader: &mut Reader<'_>) -> Result<Vec<Entry>, String> {
    let count = reader.u32()? as usize;
    if count > 10_000 {
        return Err("SFTP name count exceeds browsing limit.".into());
    }
    let mut entries = Vec::new();
    for _ in 0..count {
        let name = reader.text()?;
        reader.string()?;
        let mut entry = attributes(reader)?;
        entry.name = name;
        entries.push(entry);
    }
    reader.end()?;
    Ok(entries)
}
/// Listing order for every backend: folders first, then by name.
fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_directory()
            .cmp(&a.is_directory())
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Downloads need a regular file whose length bounds the transfer: a reported
/// size (exceeding it is an error) or a regular-file type. A server reporting
/// neither could stream a device such as /dev/zero until the disk fills.
fn downloadable(entry: &Entry) -> Result<(), String> {
    match (entry.permissions, entry.size) {
        (Some(mode), _) if mode & 0o170000 != 0o100000 => {
            Err("Only regular remote files can be downloaded.".into())
        }
        (None, None) => Err(
            "The server reported neither the file's type nor its size, so the download cannot be bounded."
                .into(),
        ),
        _ => Ok(()),
    }
}

fn attributes(reader: &mut Reader<'_>) -> Result<Entry, String> {
    attributes_with_owner(reader).map(|(entry, _)| entry)
}
/// SFTP v3 attributes plus the numeric owner and group, when reported.
fn attributes_with_owner(reader: &mut Reader<'_>) -> Result<(Entry, Option<(u32, u32)>), String> {
    let flags = reader.u32()?;
    if flags & !0x8000_000f != 0 {
        return Err("Unsupported SFTP attribute flags.".into());
    }
    let size = if flags & 1 != 0 {
        Some(u64::from_be_bytes(reader.take(8)?.try_into().unwrap()))
    } else {
        None
    };
    let owner = if flags & 2 != 0 {
        Some((reader.u32()?, reader.u32()?))
    } else {
        None
    };
    let permissions = if flags & 4 != 0 {
        Some(reader.u32()?)
    } else {
        None
    };
    let modified = if flags & 8 != 0 {
        reader.u32()?;
        Some(reader.u32()?)
    } else {
        None
    };
    if flags & 0x8000_0000 != 0 {
        let count = reader.u32()?;
        if count > 1024 {
            return Err("Too many SFTP extended attributes.".into());
        }
        for _ in 0..count {
            reader.string()?;
            reader.string()?;
        }
    }
    Ok((
        Entry {
            name: String::new(),
            size,
            permissions,
            modified,
        },
        owner,
    ))
}

/// Lets crate tests play the file worker without an SSH session.
#[cfg(test)]
impl Request {
    pub(crate) fn action(&self) -> Option<&Action> {
        self.action.as_ref()
    }
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    pub(crate) fn complete(
        self,
        result: Result<(), String>,
        fingerprint: Option<RemoteFingerprint>,
        conflict: bool,
    ) {
        *self.progress.fingerprint.lock().unwrap() = fingerprint;
        self.progress.conflict.store(conflict, Ordering::Release);
        self.answer.send(result.map(|()| Response::Done));
    }
    /// Fails like a replacement whose confirmation was lost.
    pub(crate) fn complete_uncertain(self, error: String) {
        self.progress.uncertain.store(true, Ordering::Release);
        self.answer.send(Err(error));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn downloads_need_a_regular_file_or_a_bounding_size() {
        use super::{downloadable, sort_entries, Entry};
        let entry = |name: &str, permissions, size| Entry {
            name: name.into(),
            size,
            permissions,
            modified: None,
        };
        assert!(downloadable(&entry("a", Some(0o100644), None)).is_ok());
        assert!(downloadable(&entry("a", None, Some(5))).is_ok());
        for (permissions, size) in [
            (Some(0o020666), Some(0)), // /dev/zero
            (Some(0o010644), None),    // FIFO
            (None, None),
        ] {
            assert!(downloadable(&entry("a", permissions, size)).is_err());
        }
        let mut entries = vec![
            entry("b.txt", Some(0o100644), None),
            entry("z", Some(0o040755), None),
            entry("a.txt", Some(0o100644), None),
            entry("c", Some(0o040755), None),
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["c", "z", "a.txt", "b.txt"]);
    }

    #[test]
    fn attributes_report_owner_and_group_only_when_present() {
        use super::{attributes_with_owner, Reader};
        let mut packet = Vec::new();
        packet.extend_from_slice(&0xfu32.to_be_bytes()); // size, uid/gid, mode, times
        packet.extend_from_slice(&7u64.to_be_bytes());
        packet.extend_from_slice(&0u32.to_be_bytes()); // uid root
        packet.extend_from_slice(&42u32.to_be_bytes()); // gid
        packet.extend_from_slice(&0o100640u32.to_be_bytes());
        packet.extend_from_slice(&1u32.to_be_bytes()); // atime
        packet.extend_from_slice(&1_700_000_000u32.to_be_bytes());
        let mut reader = Reader(&packet);
        let (entry, owner) = attributes_with_owner(&mut reader).unwrap();
        assert!(reader.0.is_empty());
        assert_eq!(owner, Some((0, 42)));
        assert_eq!(
            (entry.size, entry.permissions, entry.modified),
            (Some(7), Some(0o100640), Some(1_700_000_000))
        );
        let mut packet = 4u32.to_be_bytes().to_vec(); // mode only
        packet.extend_from_slice(&0o100600u32.to_be_bytes());
        let (entry, owner) = attributes_with_owner(&mut Reader(&packet)).unwrap();
        assert_eq!((owner, entry.permissions), (None, Some(0o100600)));
        assert!(attributes_with_owner(&mut Reader(&packet[..6])).is_err());
    }

    #[test]
    fn edit_fingerprints_need_a_regular_file_with_size_and_time() {
        let entry = |permissions, size, modified| Entry {
            name: "file".into(),
            size,
            permissions,
            modified,
        };
        let expected = RemoteFingerprint {
            size: 3,
            modified: 9,
        };
        assert_eq!(
            RemoteFingerprint::of(&entry(Some(0o100644), Some(3), Some(9))),
            Some(expected)
        );
        for missing in [
            entry(None, Some(3), Some(9)),
            entry(Some(0o040755), Some(3), Some(9)),
            entry(Some(0o120777), Some(3), Some(9)),
            entry(Some(0o100644), None, Some(9)),
            entry(Some(0o100644), Some(3), None),
        ] {
            assert_eq!(RemoteFingerprint::of(&missing), None);
        }
        assert!(expected.matches(&entry(Some(0o100600), Some(3), Some(9))));
        assert!(!expected.matches(&entry(Some(0o100644), Some(4), Some(9))));
        assert!(!expected.matches(&entry(Some(0o100644), Some(3), Some(10))));
        let (queue, _requests) = mpsc::sync_channel(4);
        let local = std::env::temp_dir().join("edited.txt");
        for (remote, local) in [
            ("", local.clone()),
            ("/a\0b", local.clone()),
            ("/ok", std::path::PathBuf::from("relative.txt")),
        ] {
            for scp in [false, true] {
                let replace = Action::EditReplace {
                    local: local.clone(),
                    remote: remote.into(),
                    expected,
                    scp,
                };
                let download = Action::EditDownload {
                    local: local.clone(),
                    remote: remote.into(),
                    expected,
                    scp,
                };
                assert!(submit_action(&queue, replace).is_err());
                assert!(submit_action(&queue, download).is_err());
            }
        }
        let task = submit_action(
            &queue,
            Action::EditReplace {
                local,
                remote: "/ok".into(),
                expected,
                scp: false,
            },
        )
        .unwrap();
        assert!(!task.remote_conflict());
        assert_eq!(task.remote_fingerprint(), None);
    }
    #[test]
    fn fallback_recognizes_only_the_transport_subsystem_refusal() {
        use ssh::SshError;
        assert!(super::subsystem_denied(&SshError::GeneralError(
            "channel failure.".into()
        )));
        for error in [
            SshError::TimeoutError,
            SshError::GeneralError("permission denied".into()),
            SshError::GeneralError("SFTP error 4: channel failure.".into()),
            SshError::KexError("channel failure.".into()),
        ] {
            assert!(!super::subsystem_denied(&error));
        }
    }
    use super::*;
    #[test]
    fn malformed_names_and_paths_are_rejected_without_panics() {
        for bytes in [
            &[][..],
            &[255, 255, 255, 255],
            &[0, 0, 0, 1],
            &[0, 0, 0, 1, 255, 255, 255, 255],
        ] {
            assert!(names(&mut Reader(bytes)).is_err());
        }
        for path in ["", "a\0b"] {
            assert!(validate_path(path).is_err());
        }
        assert!(validate_path("/Unicode 文件/a ' ; $().txt").is_ok());
        let (queue, receiver) = mpsc::sync_channel(1);
        let task = submit(&queue, ".".into()).unwrap();
        let request = receiver.recv().unwrap();
        drop(task);
        assert!(request.cancel.load(Ordering::Acquire));
    }
}
