//! Local file management for the local pane: validated names, creation, rename,
//! copy, move and delete. Existing entries are never replaced, folder deletion
//! accepts only empty folders, and copies are staged next to their destination
//! and published only once complete, so failures leave no partial copy behind.
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Shared progress and cancellation for a background copy or move.
#[derive(Default)]
pub struct Progress {
    cancel: AtomicBool,
    files: AtomicU64,
    bytes: AtomicU64,
}
impl Progress {
    /// Asks a running copy to stop; its temporary copy is removed.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    /// Files started so far.
    pub fn files(&self) -> u64 {
        self.files.load(Ordering::Relaxed)
    }
    /// Bytes copied so far.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
    fn step(&self, step: Step) -> io::Result<()> {
        if self.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Copy cancelled.",
            ));
        }
        match step {
            Step::File => self.files.fetch_add(1, Ordering::Relaxed),
            Step::Bytes(count) => self.bytes.fetch_add(count, Ordering::Relaxed),
        };
        Ok(())
    }
}

/// Copy progress events: before each file, and after each written chunk.
enum Step {
    File,
    Bytes(u64),
}

/// Formats a local path for messages: drops the Windows `\\?\` prefix that
/// canonical paths carry and escapes control characters.
pub fn display_path(path: &Path) -> String {
    let lossy = path.to_string_lossy();
    let text: &str = &lossy;
    let text = if cfg!(windows) {
        text.strip_prefix(r"\\?\UNC\")
            .map(|rest| format!(r"\\{rest}"))
            .or_else(|| text.strip_prefix(r"\\?\").map(str::to_owned))
            .unwrap_or_else(|| text.to_owned())
    } else {
        text.to_owned()
    };
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_debug().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Accepts exactly one file or folder name: not empty, `.` or `..`, without
/// `/`, `\`, NUL or control characters, and (on Windows) without characters,
/// endings or device names that Windows cannot store as typed.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a name.".into());
    }
    if name == "." || name == ".." {
        return Err("\".\" and \"..\" are not valid names.".into());
    }
    if name.contains(['/', '\\']) {
        return Err("Enter one name without \"/\" or \"\\\".".into());
    }
    if name.chars().any(char::is_control) {
        return Err("Names cannot contain control characters.".into());
    }
    #[cfg(windows)]
    {
        if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
            return Err("Windows names cannot contain < > : \" | ? or *.".into());
        }
        if name.ends_with([' ', '.']) {
            return Err("Windows names cannot end with a space or a dot.".into());
        }
        if windows_device_name(name) {
            return Err(format!("{name:?} is reserved by Windows."));
        }
    }
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(part)), None) if part == OsStr::new(name) => Ok(()),
        _ => Err("Enter a single file or folder name.".into()),
    }
}

/// Whether Windows treats `name` as a device (CON, NUL, COM1, LPT¹, …)
/// regardless of its extension. Pure string check, usable on any platform.
pub fn windows_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    (prefix == "COM" || prefix == "LPT")
        && matches!(chars.next(), Some('0'..='9' | '¹' | '²' | '³'))
        && chars.next().is_none()
}

/// Joins a validated name to an absolute folder.
pub fn child_path(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if !parent.is_absolute() {
        return Err("Open a local folder first.".into());
    }
    validate_name(name)?;
    Ok(parent.join(name))
}

/// Creates an empty folder; fails if any entry already has that name.
pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = child_path(parent, name)?;
    fs::create_dir(&path).map_err(|e| failure("create", &path, &path, e))?;
    Ok(path)
}

/// Creates an empty file; fails if any entry (even a dangling link) has that name.
pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = child_path(parent, name)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| failure("create", &path, &path, e))?;
    Ok(path)
}

/// Renames an entry within its folder without replacing an existing entry.
pub fn rename(source: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let parent = source.parent().ok_or("Select a file or folder first.")?;
    if source.file_name() == Some(OsStr::new(name)) {
        return Err("Enter a different name.".into());
    }
    let target = parent.join(name);
    rename_no_replace(source, &target).map_err(|e| failure("rename", source, &target, e))?;
    Ok(target)
}

/// Resolves where `source` would land inside `destination` and refuses
/// existing targets, the source's own folder, and a folder inside itself.
pub fn transfer_target(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let name = source.file_name().ok_or("Select a file or folder first.")?;
    fs::symlink_metadata(source)
        .map_err(|e| format!("Cannot read {}: {e}", display_path(source)))?;
    let folder = fs::canonicalize(destination).map_err(|e| {
        format!(
            "Cannot open destination folder {}: {e}",
            display_path(destination)
        )
    })?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder.", display_path(&folder)));
    }
    let home = source
        .parent()
        .and_then(|parent| fs::canonicalize(parent).ok());
    if home.as_deref() == Some(folder.as_path()) {
        return Err(format!(
            "{} is already in {}. Choose a different folder.",
            display_path(Path::new(name)),
            display_path(&folder)
        ));
    }
    if fs::metadata(source).is_ok_and(|meta| meta.is_dir()) {
        let real = fs::canonicalize(source)
            .map_err(|e| format!("Cannot read {}: {e}", display_path(source)))?;
        if folder.starts_with(&real) {
            return Err("A folder cannot be copied or moved into itself or its subfolders.".into());
        }
    }
    let target = folder.join(name);
    match fs::symlink_metadata(&target) {
        Ok(_) => Err(format!(
            "{} already exists. Nothing was copied or moved.",
            display_path(&target)
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(target),
        Err(e) => Err(format!("Cannot check {}: {e}", display_path(&target))),
    }
}

/// Copies a file or folder (recursively) into `destination`. The copy is
/// staged in a temporary folder there and published without replacing an
/// existing entry; on failure or cancellation the staged copy is removed.
/// Symbolic links inside folders and special files are refused.
pub fn copy_into(
    source: &Path,
    destination: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let target = transfer_target(source, destination)?;
    copy_staged(source, &target, false, &mut |step: Step| {
        progress.step(step)
    })?;
    Ok(target)
}

/// Moves a file or folder into `destination` with a no-replace rename. Across
/// volumes it copies (flushed to disk), publishes the complete copy, and only
/// then removes the originals that were copied and have not changed since.
pub fn move_into(
    source: &Path,
    destination: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let target = transfer_target(source, destination)?;
    match rename_no_replace(source, &target) {
        Ok(()) => Ok(target),
        Err(error) if crosses_devices(&error) => {
            move_by_copy(source, &target, &mut |step: Step| progress.step(step))?;
            Ok(target)
        }
        Err(error) => Err(failure("move", source, &target, error)),
    }
}

/// Deletes a file, a link, or an empty folder. Non-empty folders are refused.
pub fn delete(path: &Path) -> Result<(), String> {
    let shown = display_path(path);
    let not_empty = || format!("{shown} is not empty. Only empty folders can be deleted.");
    let meta = fs::symlink_metadata(path).map_err(|e| format!("Cannot read {shown}: {e}"))?;
    let result = if meta.is_dir() {
        let mut entries = fs::read_dir(path).map_err(|e| format!("Cannot read {shown}: {e}"))?;
        if entries.next().is_some() {
            return Err(not_empty());
        }
        fs::remove_dir(path)
    } else {
        remove_non_folder(path, &meta)
    };
    result.map_err(|e| {
        if e.kind() == io::ErrorKind::DirectoryNotEmpty {
            not_empty()
        } else {
            format!("Cannot delete {shown}: {e}")
        }
    })
}

#[cfg(windows)]
fn remove_non_folder(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt;
    if meta.file_type().is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}
#[cfg(not(windows))]
fn remove_non_folder(path: &Path, _meta: &fs::Metadata) -> io::Result<()> {
    fs::remove_file(path)
}

fn failure(action: &str, source: &Path, target: &Path, error: io::Error) -> String {
    match name_target(error, target) {
        error if error.kind() == io::ErrorKind::AlreadyExists => error.to_string(),
        error => format!("Cannot {action} {}: {error}", display_path(source)),
    }
}

/// Rewrites "already exists" errors to name the preserved target.
fn name_target(error: io::Error, target: &Path) -> io::Error {
    if error.kind() == io::ErrorKind::AlreadyExists {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists. Nothing was replaced.",
                display_path(target)
            ),
        )
    } else {
        error
    }
}

/// Adds the path to an I/O error while keeping its kind.
fn at(path: &Path) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{}: {error}", display_path(path)))
}

fn crosses_devices(error: &io::Error) -> bool {
    #[cfg(windows)]
    {
        let code = windows_sys::Win32::Foundation::ERROR_NOT_SAME_DEVICE as i32;
        if error.raw_os_error() == Some(code) {
            return true;
        }
    }
    error.kind() == io::ErrorKind::CrossesDevices
}

/// Renames atomically and fails if `to` exists (case-only renames of the same
/// entry are allowed). Fails across volumes instead of copying.
#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let text: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if text[..text.len() - 1].contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        Ok(text)
    };
    let (from, to) = (wide(from)?, wide(to)?);
    // SAFETY: both buffers are NUL-terminated UTF-16 paths that outlive the call.
    // Flags 0: fail when the destination exists and never copy across volumes.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Renames without replacing `to`. Regular files use a hard link, which fails
/// atomically when `to` exists; other entries are checked just before rename
/// (renaming a folder never replaces a file or a non-empty folder).
#[cfg(not(windows))]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(from)?.is_file() {
        match fs::hard_link(from, to) {
            Ok(()) => {
                return fs::remove_file(from).inspect_err(|_| {
                    let _ = fs::remove_file(to);
                })
            }
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists || crosses_devices(&error) =>
            {
                return Err(error)
            }
            Err(_) => {} // Hard links are unsupported here; fall back to rename.
        }
    }
    match fs::symlink_metadata(to) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
        Err(error) => Err(error),
    }
}

/// Cross-volume move: publish a complete, flushed copy, then remove exactly the
/// originals that were copied (deepest first). Files that changed after their
/// copy and entries added to the source meanwhile are kept, and the error says
/// where removal stopped.
fn move_by_copy(
    source: &Path,
    target: &Path,
    step: &mut dyn FnMut(Step) -> io::Result<()>,
) -> Result<(), String> {
    let meta = fs::symlink_metadata(source)
        .map_err(|e| format!("Cannot read {}: {e}", display_path(source)))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{} is a symbolic link and cannot be moved to another volume. Nothing was changed.",
            display_path(source)
        ));
    }
    let originals = copy_staged(source, target, true, step)?;
    let mut buffer = vec![0; 1 << 20];
    for original in originals.iter().rev() {
        let (path, removed) = match original {
            Original::Folder(path) => (path, fs::remove_dir(path)),
            Original::File(path, copied) => (path, remove_unchanged(path, copied, &mut buffer)),
        };
        if let Err(error) = removed {
            return Err(format!(
                "Copied everything to {}, but removing the original stopped at {}: {error}. \
                 Nothing was lost; check both locations before retrying.",
                display_path(target),
                display_path(path)
            ));
        }
    }
    Ok(())
}

/// An original that a move copied: only these are removed afterwards.
#[derive(Debug)]
enum Original {
    Folder(PathBuf),
    File(PathBuf, Fingerprint),
}

/// The length and SHA-256 of a file's bytes.
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    digest: [u8; 32],
}

fn fingerprint(mut input: impl Read, buffer: &mut [u8]) -> io::Result<Fingerprint> {
    let mut hasher = Sha256::new();
    let mut len = 0;
    loop {
        match input.read(buffer) {
            Ok(0) => break,
            Ok(count) => {
                hasher.update(&buffer[..count]);
                len += count as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(Fingerprint {
        len,
        digest: hasher.finalize().into(),
    })
}

/// Removes a moved file only if it still holds exactly the bytes that were
/// copied, so newer data written to it meanwhile is never discarded. It is
/// verified through a handle that does not follow links. On Windows that handle
/// also keeps writers out and deletes the very file it verified; elsewhere the
/// path must still name the verified file when it is removed. Unix has no
/// mandatory locks, so a program that already has the file open can still
/// write to it in the moment between verification and removal.
fn remove_unchanged(path: &Path, copied: &Fingerprint, buffer: &mut [u8]) -> io::Result<()> {
    let changed = || {
        io::Error::other(
            "it changed while it was being moved, so it was kept and the moved copy may be older",
        )
    };
    let mut file = open_original(path)?;
    if !file.metadata()?.is_file() || fingerprint(&mut file, buffer)? != *copied {
        return Err(changed());
    }
    #[cfg(windows)]
    {
        delete_by_handle(&file)
    }
    #[cfg(unix)]
    {
        // Unix cannot delete through a handle: the name must still be this file.
        use std::os::unix::fs::MetadataExt;
        let (opened, named) = (file.metadata()?, fs::symlink_metadata(path)?);
        if (opened.dev(), opened.ino()) != (named.dev(), named.ino()) {
            return Err(changed());
        }
        fs::remove_file(path)
    }
}

/// Opens an original for verification. Without write sharing, the open fails
/// while another program has the file open for writing, and later writers are
/// refused until the handle closes. A link put in its place is not followed.
#[cfg(windows)]
fn open_original(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ,
    };
    fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
/// Opens an original for verification without following a link or waiting on
/// a FIFO put in its place.
#[cfg(unix)]
fn open_original(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Deletes the file behind `file` (opened with DELETE access). POSIX semantics
/// free the name at once so its emptied folder can be removed next; file
/// systems without them delete the file when the handle closes.
#[cfg(windows)]
fn delete_by_handle(file: &fs::File) -> io::Result<()> {
    use std::{ffi::c_void, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
        Storage::FileSystem::{
            FileDispositionInfo, FileDispositionInfoEx, SetFileInformationByHandle,
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
            FILE_INFO_BY_HANDLE_CLASS,
        },
    };
    fn set<T>(file: &fs::File, class: FILE_INFO_BY_HANDLE_CLASS, info: &T) -> io::Result<()> {
        // SAFETY: the handle stays open for the call and `info` is a live value
        // of the structure that `class` names, passed with its exact size.
        let done = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                class,
                (info as *const T).cast::<c_void>(),
                std::mem::size_of::<T>() as u32,
            )
        };
        if done != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    // Read-only originals go too, as when a move within a volume renames them.
    let posix = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    match set(file, FileDispositionInfoEx, &posix) {
        Err(error)
            if [
                ERROR_INVALID_FUNCTION,
                ERROR_INVALID_PARAMETER,
                ERROR_NOT_SUPPORTED,
            ]
            .iter()
            .any(|&code| error.raw_os_error() == Some(code as i32)) =>
        {
            set(
                file,
                FileDispositionInfo,
                &FILE_DISPOSITION_INFO { DeleteFile: true },
            )
        }
        result => result,
    }
}

/// Copies `source` to a staging folder beside `target`, then publishes it with a
/// no-replace rename. A move (`moving`) flushes the copy to disk and returns the
/// copied originals (folders before contents); a plain copy returns none.
fn copy_staged(
    source: &Path,
    target: &Path,
    moving: bool,
    step: &mut dyn FnMut(Step) -> io::Result<()>,
) -> Result<Vec<Original>, String> {
    let parent = target
        .parent()
        .ok_or("The destination needs a parent folder.")?;
    let name = target.file_name().ok_or("The destination needs a name.")?;
    let staging = tempfile::Builder::new()
        .prefix(".neo-ssh-copy-")
        .tempdir_in(parent)
        .map_err(|e| format!("Cannot prepare a copy in {}: {e}", display_path(parent)))?;
    let staged = staging.path().join(name);
    let mut copier = Copier {
        buffer: vec![0; 1 << 20],
        originals: moving.then(Vec::new),
        step,
    };
    let result = copier
        .entry(source, &staged, true)
        .and_then(|()| rename_no_replace(&staged, target).map_err(|e| name_target(e, target)))
        .map_err(|error| match error.kind() {
            io::ErrorKind::Interrupted | io::ErrorKind::AlreadyExists => error.to_string(),
            _ => format!("Copy failed: {error}."),
        });
    let leftover = staging.path().to_owned();
    match (result, staging.close()) {
        (Ok(()), _) => Ok(copier.originals.unwrap_or_default()),
        (Err(error), Ok(())) => Err(format!("{error} The temporary copy was removed.")),
        (Err(error), Err(cleanup)) => Err(format!(
            "{error} The temporary copy {} could not be removed: {cleanup}",
            display_path(&leftover)
        )),
    }
}

struct Copier<'a> {
    buffer: Vec<u8>,
    /// Moves only: copies are flushed and each original is recorded, files with
    /// the fingerprint of the bytes copied.
    originals: Option<Vec<Original>>,
    step: &'a mut dyn FnMut(Step) -> io::Result<()>,
}
impl Copier<'_> {
    /// The top-level source may be a link (it is followed); links and special
    /// files inside folders are refused rather than followed or skipped.
    fn entry(&mut self, source: &Path, target: &Path, top: bool) -> io::Result<()> {
        let meta = if top {
            fs::metadata(source)
        } else {
            fs::symlink_metadata(source)
        }
        .map_err(at(source))?;
        if meta.is_dir() {
            fs::create_dir(target).map_err(at(target))?;
            if let Some(originals) = &mut self.originals {
                originals.push(Original::Folder(source.to_owned()));
            }
            for entry in fs::read_dir(source).map_err(at(source))? {
                let entry = entry.map_err(at(source))?;
                self.entry(&entry.path(), &target.join(entry.file_name()), false)?;
            }
            Ok(())
        } else if meta.is_file() {
            (self.step)(Step::File)?;
            let copied = self.file(source, target, &meta)?;
            if let (Some(originals), Some(copied)) = (&mut self.originals, copied) {
                originals.push(Original::File(source.to_owned(), copied));
            }
            Ok(())
        } else if meta.file_type().is_symlink() {
            Err(io::Error::other(format!(
                "{} is a symbolic link; folders containing links are not copied.",
                display_path(source)
            )))
        } else {
            Err(io::Error::other(format!(
                "{} is not a regular file or folder.",
                display_path(source)
            )))
        }
    }
    /// Copies one file; for a move, returns the fingerprint of the bytes copied.
    fn file(
        &mut self,
        source: &Path,
        target: &Path,
        meta: &fs::Metadata,
    ) -> io::Result<Option<Fingerprint>> {
        let mut input = fs::File::open(source).map_err(at(source))?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(at(target))?;
        let mut hasher = self.originals.is_some().then(Sha256::new);
        let mut len = 0;
        loop {
            let count = match input.read(&mut self.buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(at(source)(error)),
            };
            output
                .write_all(&self.buffer[..count])
                .map_err(at(target))?;
            if let Some(hasher) = &mut hasher {
                hasher.update(&self.buffer[..count]);
            }
            len += count as u64;
            (self.step)(Step::Bytes(count as u64))?;
        }
        #[cfg(unix)]
        output
            .set_permissions(meta.permissions())
            .map_err(at(target))?;
        if let Ok(modified) = meta.modified() {
            let _ = output.set_modified(modified);
        }
        if self.originals.is_some() {
            output.sync_all().map_err(at(target))?;
        }
        Ok(hasher.map(|hasher| Fingerprint {
            len,
            digest: hasher.finalize().into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Builds `root/tree/{a.txt, empty/, sub/{b.bin, deeper/c.txt}}`.
    fn tree(root: &Path) -> PathBuf {
        let tree = root.join("tree");
        fs::create_dir_all(tree.join("empty")).unwrap();
        fs::create_dir_all(tree.join("sub").join("deeper")).unwrap();
        fs::write(tree.join("a.txt"), b"alpha").unwrap();
        fs::write(tree.join("sub").join("b.bin"), [0u8, 255, 1, 254]).unwrap();
        fs::write(tree.join("sub").join("deeper").join("c.txt"), b"gamma").unwrap();
        tree
    }

    fn assert_tree(tree: &Path) {
        assert_eq!(names(tree), ["a.txt", "empty", "sub"]);
        assert!(names(&tree.join("empty")).is_empty());
        assert_eq!(fs::read(tree.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(
            fs::read(tree.join("sub").join("b.bin")).unwrap(),
            [0, 255, 1, 254]
        );
        assert_eq!(
            fs::read(tree.join("sub").join("deeper").join("c.txt")).unwrap(),
            b"gamma"
        );
    }

    #[test]
    fn names_must_be_single_plain_components() {
        for name in [
            "file.txt",
            "文件 ' ; $(x)",
            ".hidden",
            "..double",
            "a..b",
            " lead",
        ] {
            assert_eq!(validate_name(name), Ok(()), "{name:?}");
        }
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "/abs",
            "../up",
            "a\0b",
            "line\nbreak",
            "tab\t",
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        #[cfg(windows)]
        for name in [
            "C:x", "a:b", "x.", "x ", "CON", "con.txt", "Lpt1", "a?b", "a*b",
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert!(child_path(Path::new("relative"), "a").is_err());
    }

    #[test]
    fn display_path_hides_verbatim_prefix_and_escapes_controls() {
        assert_eq!(display_path(Path::new("/a\nb")), "/a\\nb");
        #[cfg(windows)]
        {
            assert_eq!(display_path(Path::new(r"\\?\C:\Users\x")), r"C:\Users\x");
            assert_eq!(
                display_path(Path::new(r"\\?\UNC\server\share\x")),
                r"\\server\share\x"
            );
        }
    }

    #[test]
    fn create_never_replaces_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let folder = create_folder(dir.path(), "folder").unwrap();
        assert!(folder.is_dir());
        let file = create_file(dir.path(), "new file.txt").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"");
        fs::write(&file, b"keep").unwrap();
        let error = create_file(dir.path(), "new file.txt").unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert!(create_folder(dir.path(), "new file.txt").is_err());
        assert!(create_file(dir.path(), "folder").is_err());
        assert!(create_folder(dir.path(), "folder").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"keep");
        for bad in ["", ".", "..", "x/y", "x\\y"] {
            assert!(create_file(dir.path(), bad).is_err());
            assert!(create_folder(dir.path(), bad).is_err());
        }
        assert_eq!(names(dir.path()), ["folder", "new file.txt"]);
    }

    #[test]
    fn rename_never_replaces_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"first").unwrap();
        fs::write(&b, b"second").unwrap();
        let error = rename(&a, "b.txt").unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(fs::read(&a).unwrap(), b"first");
        assert_eq!(fs::read(&b).unwrap(), b"second");
        assert!(rename(&a, "a.txt").is_err());
        assert!(rename(&a, "../escape.txt").is_err());
        let renamed = rename(&a, "c ' $(x).txt").unwrap();
        assert_eq!(renamed, dir.path().join("c ' $(x).txt"));
        assert_eq!(fs::read(&renamed).unwrap(), b"first");
        assert!(!a.exists());
        // Folders: neither an empty folder nor a file is replaced.
        fs::create_dir(dir.path().join("empty")).unwrap();
        fs::create_dir(dir.path().join("full")).unwrap();
        fs::write(dir.path().join("full").join("x"), b"x").unwrap();
        assert!(rename(&dir.path().join("full"), "empty").is_err());
        assert!(rename(&dir.path().join("full"), "b.txt").is_err());
        assert!(rename(&dir.path().join("empty"), "full").is_err());
        assert_eq!(fs::read(dir.path().join("full").join("x")).unwrap(), b"x");
        rename(&dir.path().join("full"), "moved").unwrap();
        assert_eq!(
            names(dir.path()),
            ["b.txt", "c ' $(x).txt", "empty", "moved"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn case_only_rename_is_allowed_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readme.txt");
        fs::write(&path, b"x").unwrap();
        rename(&path, "README.txt").unwrap();
        assert_eq!(names(dir.path()), ["README.txt"]);
    }

    #[test]
    fn copy_is_recursive_and_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let progress = Progress::default();
        let copied = copy_into(&source, &destination, &progress).unwrap();
        assert_eq!(copied, fs::canonicalize(&destination).unwrap().join("tree"));
        assert_tree(&copied);
        assert_tree(&source);
        assert_eq!(progress.files(), 3);
        assert_eq!(progress.bytes(), 14);
        // A second copy reports the existing destination and changes nothing.
        fs::write(copied.join("a.txt"), b"edited").unwrap();
        let error = copy_into(&source, &destination, &Progress::default()).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(fs::read(copied.join("a.txt")).unwrap(), b"edited");
        assert_eq!(names(&destination), ["tree"]);
        // Files, too.
        let file = dir.path().join("note.txt");
        fs::write(&file, b"note").unwrap();
        let copy = copy_into(&file, &destination, &Progress::default()).unwrap();
        assert_eq!(fs::read(copy).unwrap(), b"note");
        assert!(copy_into(&file, &destination, &Progress::default()).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"note");
        // Not into its own folder, itself or a subfolder.
        assert!(copy_into(&file, dir.path(), &Progress::default()).is_err());
        assert!(copy_into(&source, &source, &Progress::default()).is_err());
        assert!(copy_into(&source, &source.join("sub"), &Progress::default()).is_err());
        assert_tree(&source);
        assert_eq!(names(&destination), ["note.txt", "tree"]);
    }

    #[test]
    fn failed_or_cancelled_copies_leave_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("existing"), b"keep").unwrap();
        // Fail on the second file, after a folder and a file were copied.
        let mut files = 0;
        let error = copy_staged(
            &source,
            &destination.join("tree"),
            false,
            &mut |step: Step| {
                if matches!(step, Step::File) {
                    files += 1;
                    if files == 2 {
                        return Err(io::Error::other("disk full"));
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(
            error.contains("disk full") && error.contains("removed"),
            "{error}"
        );
        assert_eq!(names(&destination), ["existing"]);
        // Cancellation.
        let progress = Progress::default();
        progress.cancel();
        let error = copy_into(&source, &destination, &progress).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(names(&destination), ["existing"]);
        assert_tree(&source);
    }

    #[test]
    fn move_renames_and_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(destination.join("taken")).unwrap();
        let taken = dir.path().join("taken");
        fs::write(&taken, b"mine").unwrap();
        assert!(move_into(&taken, &destination, &Progress::default()).is_err());
        assert_eq!(fs::read(&taken).unwrap(), b"mine");
        assert!(destination.join("taken").is_dir());
        assert!(move_into(&source, &source.join("sub"), &Progress::default()).is_err());
        let moved = move_into(&source, &destination, &Progress::default()).unwrap();
        assert!(!source.exists());
        assert_tree(&moved);
        let file = dir.path().join("f.txt");
        fs::write(&file, b"f").unwrap();
        let moved = move_into(&file, &destination, &Progress::default()).unwrap();
        assert_eq!(fs::read(moved).unwrap(), b"f");
        assert!(!file.exists());
        assert_eq!(names(&destination), ["f.txt", "taken", "tree"]);
    }

    #[test]
    fn cross_volume_move_removes_originals_only_after_a_complete_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        // A failed copy keeps every original and publishes nothing.
        let mut files = 0;
        let error = move_by_copy(&source, &destination.join("tree"), &mut |step: Step| {
            if matches!(step, Step::File) {
                files += 1;
                if files == 3 {
                    return Err(io::Error::other("read error"));
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("read error"), "{error}");
        assert_tree(&source);
        assert!(names(&destination).is_empty());
        // A complete copy is published, then the originals are removed.
        move_by_copy(&source, &destination.join("tree"), &mut |_: Step| Ok(())).unwrap();
        assert!(!source.exists());
        assert_tree(&destination.join("tree"));
        // An existing target is never replaced.
        let file = dir.path().join("tree");
        fs::write(&file, b"other").unwrap();
        assert!(move_by_copy(&file, &destination.join("tree"), &mut |_: Step| Ok(())).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"other");
        assert_tree(&destination.join("tree"));
        assert_eq!(names(&destination), ["tree"]);
    }

    #[test]
    fn cross_volume_move_keeps_originals_that_change_after_their_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let changed = source.join("sub").join("b.bin");
        // Once every byte is copied, another writer saves same-sized new data.
        let mut copied = 0;
        let error = move_by_copy(&source, &destination.join("tree"), &mut |step: Step| {
            if let Step::Bytes(count) = step {
                copied += count;
                if copied == 14 {
                    fs::write(&changed, b"NEW!")?;
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            error.contains("changed") && error.contains(&display_path(&changed)),
            "{error}"
        );
        assert_eq!(fs::read(&changed).unwrap(), b"NEW!");
        // The complete earlier copy stays published.
        assert_tree(&destination.join("tree"));
    }

    #[test]
    fn cross_volume_move_removes_read_only_originals_like_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let locked = source.join("sub").join("b.bin");
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&locked, permissions).unwrap();
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        move_by_copy(&source, &destination.join("tree"), &mut |_: Step| Ok(())).unwrap();
        assert!(!source.exists());
        assert_tree(&destination.join("tree"));
    }

    #[test]
    fn delete_accepts_files_and_empty_folders_only() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        let error = delete(&source).unwrap_err();
        assert!(error.contains("not empty"), "{error}");
        assert_tree(&source);
        delete(&source.join("empty")).unwrap();
        delete(&source.join("a.txt")).unwrap();
        assert_eq!(names(&source), ["sub"]);
        assert!(delete(&source.join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_inside_copied_folders_are_refused_and_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let source = tree(dir.path());
        std::os::unix::fs::symlink("/", source.join("sub").join("root-link")).unwrap();
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let error = copy_into(&source, &destination, &Progress::default()).unwrap_err();
        assert!(error.contains("symbolic link"), "{error}");
        assert!(names(&destination).is_empty());
        // Deleting a link removes the link, not its target.
        delete(&source.join("sub").join("root-link")).unwrap();
        assert_tree(&source);
    }
}
