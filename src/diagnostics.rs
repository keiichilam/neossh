//! Explicit terminal reports and bounded crash records with panic payloads omitted.
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
const LIMIT: u64 = 64 * 1024;

/// Only terminal-derived state and numeric connection counters enter this report.
pub fn terminal_report(
    terminal: &crate::terminal::Terminal,
    connected: bool,
    sent: u64,
    received: u64,
) -> String {
    use base64::Engine;
    use std::fmt::Write as _;
    let screen = terminal.screen();
    let (rows, columns) = screen.size();
    let mut visible = String::new();
    let mut truncated = false;
    'rows: for row in 0..rows {
        for column in 0..columns {
            if let Some(cell) = screen.cell(row, column) {
                if cell.is_wide_continuation() {
                    continue;
                }
                let text = cell.contents();
                let text = if text.is_empty() { " " } else { &text };
                if visible.len() + text.len() + 1 > 64 * 1024 {
                    truncated = true;
                    break 'rows;
                }
                visible.push_str(text);
            }
        }
        visible.push('\n');
    }
    let mut report = format!("NEO-SSH diagnostic report\nversion={}\nconnected={connected}\nbytes_sent={sent}\nbytes_received={received}\nterminal_bytes_received={}\nrows={rows}\ncolumns={columns}\ncursor={:?}\ncursor_hidden={}\nscrollback_offset={}\napplication_cursor={}\nmodes={:?}\nvisible_text_truncated={truncated}\n", env!("CARGO_PKG_VERSION"), terminal.received, screen.cursor_position(), screen.hide_cursor(), screen.scrollback(), screen.application_cursor(), terminal.modes);
    let _ = writeln!(report, "visible_text={visible:?}");
    let raw = terminal.recent_output();
    let _ = writeln!(report, "recent_remote_output_bytes={}", raw.len());
    let _ = writeln!(
        report,
        "recent_remote_output_base64={}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    );
    report.push_str("Input buffers, credentials and connection configuration are excluded. Remote output may contain echoed input.\n");
    report
}

pub fn save_report(path: &Path, report: &str) -> Result<(), String> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("Choose an absolute diagnostic file path.".into());
    }
    let mut file = tempfile::Builder::new()
        .prefix(".ssh4-report-")
        .tempfile_in(path.parent().ok_or("Choose a report folder.")?)
        .map_err(|e| e.to_string())?;
    file.write_all(report.as_bytes())
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist_noclobber(path).map_err(|e| {
        format!("Cannot save diagnostic report without replacing an existing file: {e}")
    })?;
    Ok(())
}

static EXPORTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct ExportGuard;
impl Drop for ExportGuard {
    fn drop(&mut self) {
        EXPORTS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
pub struct ExportJob {
    result: std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>,
}
impl ExportJob {
    pub fn start(path: std::path::PathBuf, report: String) -> Result<Self, String> {
        use std::sync::atomic::Ordering;
        EXPORTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .map_err(|_| "Diagnostic exports are still being saved. Try again shortly.")?;
        let guard = ExportGuard;
        let (sender, result) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("diagnostic-export".into())
            .spawn(move || {
                let _guard = guard;
                let result = save_report(&path, &report).map(|()| path);
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { result })
    }
    pub fn poll(&self) -> Option<Result<std::path::PathBuf, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Diagnostic export worker ended.".into())),
        }
    }
}
struct CrashLog {
    file: Mutex<File>,
}
impl CrashLog {
    fn open(path: &Path) -> Result<Self, String> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err("Crash log path is not a regular file.".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
    fn record(&self, text: &str) -> std::io::Result<()> {
        let Ok(mut file) = self.file.try_lock() else {
            return Err(std::io::Error::other("Crash log is busy."));
        };
        if text.len() as u64 > LIMIT {
            return Err(std::io::Error::other("Crash record exceeds limit."));
        }
        if file.metadata()?.len().saturating_add(text.len() as u64) > LIMIT {
            file.set_len(0)?;
        }
        file.seek(SeekFrom::End(0))?;
        file.write_all(text.as_bytes())?;
        file.sync_data()
    }
}
fn record_text(source: &str, line: u32, column: u32) -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let source = Path::new(source)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    format!(
        "time={time} version={} thread={:?} panic_at={source}:{line}:{column}; payload omitted\n",
        env!("CARGO_PKG_VERSION"),
        std::thread::current().id()
    )
}
/// Install after resolving configuration and before starting application workers.
pub fn install(config_path: &Path) -> Result<(), String> {
    let path = config_path.with_extension("panic.log");
    let log = CrashLog::open(&path)?;
    std::panic::set_hook(Box::new(move |info| {
        let record = if let Some(location) = info.location() {
            record_text(location.file(), location.line(), location.column())
        } else {
            record_text("unknown", 0, 0)
        };
        if log.record(&record).is_ok() {
            eprintln!(
                "neo-ssh: unexpected failure; crash location recorded in {}",
                path.display()
            );
        } else {
            eprintln!("neo-ssh: unexpected failure; crash log could not be written.");
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_captures_terminal_state_without_paste_input_and_preserves_existing_files() {
        use base64::Engine;
        let mut terminal = crate::terminal::Terminal::new(3, 20, 10);
        let remote = b"visible output\x1b[?2004h";
        terminal.feed(remote);
        let _ = terminal.paste("LOCAL_SECRET_INPUT");
        let report = terminal_report(&terminal, true, 12, remote.len() as u64);
        assert!(report.contains("connected=true") && report.contains("bracketed_paste: true"));
        assert!(!report.contains("LOCAL_SECRET_INPUT"));
        let encoded = report
            .lines()
            .find_map(|line| line.strip_prefix("recent_remote_output_base64="))
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            remote
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.txt");
        save_report(&path, &report).unwrap();
        assert!(save_report(&path, "replacement").is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), report);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn crash_records_are_bounded_and_do_not_include_source_directory_or_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panic.log");
        let log = CrashLog::open(&path).unwrap();
        let record = record_text("private-folder/module.rs", 42, 3);
        assert!(record.contains("module.rs:42:3"));
        assert!(!record.contains("private-folder"));
        assert!(record.contains("payload omitted"));
        for _ in 0..1000 {
            log.record(&record).unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().len() <= LIMIT);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with(&record));
        assert!(CrashLog::open(dir.path()).is_err());
    }
}
