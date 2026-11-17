//! Opt-in metadata logging. This API cannot receive terminal text or credentials.
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
const LIMIT: u64 = 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub tab: usize,
    pub connected: bool,
    pub connecting: bool,
    pub trust_prompt: bool,
    pub auth_prompt: bool,
    pub failed: bool,
    pub received: u64,
    pub sent: u64,
    pub rows: u16,
    pub columns: u16,
}
pub struct Log {
    pub path: PathBuf,
    sender: Option<mpsc::SyncSender<Record>>,
    result: mpsc::Receiver<Result<(), String>>,
    ready: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
}
impl Log {
    pub fn start(path: PathBuf) -> Result<Self, String> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "The previous debug log is still closing.")?;
        let guard = Guard;
        let (sender, records) = mpsc::sync_channel(256);
        let (done, result) = mpsc::sync_channel(1);
        let ready = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicUsize::new(0));
        let (file_path, started, lost) = (path.clone(), ready.clone(), dropped.clone());
        std::thread::Builder::new()
            .name("debug-log".into())
            .spawn(move || {
                let _guard = guard;
                let result = write_records(file_path, records, started, lost);
                let _ = done.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            sender: Some(sender),
            result,
            ready,
            dropped,
        })
    }
    pub fn enabled(&self) -> bool {
        self.sender.is_some()
    }
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    pub fn record(&self, record: Record) {
        if let Some(sender) = &self.sender {
            if sender.try_send(record).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub fn stop(&mut self) {
        self.sender = None;
    }
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.result.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Debug log worker ended.".into())),
        }
    }
}
fn write_records(
    path: PathBuf,
    records: mpsc::Receiver<Record>,
    ready: Arc<AtomicBool>,
    lost: Arc<AtomicUsize>,
) -> Result<(), String> {
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("Debug log path must be a regular file.".into())
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    fn append(file: &mut std::fs::File, line: &str) -> Result<(), String> {
        if file
            .metadata()
            .map_err(|e| e.to_string())?
            .len()
            .saturating_add(line.len() as u64)
            > LIMIT
        {
            file.set_len(0).map_err(|e| e.to_string())?;
        }
        file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        file.write_all(line.as_bytes()).map_err(|e| e.to_string())
    }
    append(
        &mut file,
        &format!(
            "debug_log_started version={} metadata_only=true\n",
            env!("CARGO_PKG_VERSION")
        ),
    )?;
    ready.store(true, Ordering::Release);
    for record in records {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let skipped = lost.swap(0, Ordering::AcqRel);
        append(
            &mut file,
            &format!("time_ms={timestamp} dropped={skipped} {record:?}\n"),
        )?;
    }
    append(
        &mut file,
        &format!(
            "debug_log_stopped dropped={}\n",
            lost.load(Ordering::Acquire)
        ),
    )?;
    file.sync_all().map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_log_rotates_and_flushes_when_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("debug.log");
        std::fs::write(&path, vec![b'x'; LIMIT as usize]).unwrap();
        let mut log = Log::start(path.clone()).unwrap();
        log.record(Record {
            tab: 0,
            connected: true,
            connecting: false,
            trust_prompt: false,
            auth_prompt: false,
            failed: false,
            received: 42,
            sent: 7,
            rows: 24,
            columns: 80,
        });
        log.stop();
        log.result
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap()
            .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.starts_with("debug_log_started"));
        assert!(text.contains("received: 42"));
        assert!(text.ends_with("debug_log_stopped dropped=0\n"));
        assert!(text.len() < LIMIT as usize);
    }
}
