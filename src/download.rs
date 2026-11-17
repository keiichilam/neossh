//! Local download staging: never publish a partial file or replace an existing path.
use std::{io::Write, path::PathBuf};
pub(crate) struct StagedDownload {
    target: PathBuf,
    staging: Option<tempfile::NamedTempFile>,
    pub bytes: u64,
}
impl StagedDownload {
    pub fn new(target: PathBuf) -> Result<Self, String> {
        if !target.is_absolute() || target.file_name().is_none() {
            return Err("Choose an absolute local file path.".into());
        }
        match std::fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err("The local destination already exists. Choose a new filename.".into())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let parent = target.parent().ok_or("Local file needs a parent folder.")?;
        let staging = tempfile::Builder::new()
            .prefix(".ssh4-download-")
            .suffix(".part")
            .tempfile_in(parent)
            .map_err(|e| format!("Cannot create download file: {e}"))?;
        Ok(Self {
            target,
            staging: Some(staging),
            bytes: 0,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.staging
            .as_mut()
            .ok_or("Download already finished.")?
            .write_all(bytes)
            .map_err(|e| format!("Cannot write download: {e}"))?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or("Download is too large.")?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<(), String> {
        let staging = self.staging.take().ok_or("Download already finished.")?;
        staging
            .as_file()
            .sync_all()
            .map_err(|e| format!("Cannot flush download: {e}"))?;
        staging
            .persist_noclobber(&self.target)
            .map_err(|e| format!("Cannot publish download without replacing a file: {e}"))?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_downloads_clean_up_and_concurrent_destinations_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("文件.bin");
        {
            let mut pending = StagedDownload::new(target.clone()).unwrap();
            pending.append(b"partial").unwrap();
            assert!(!target.exists());
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        let mut pending = StagedDownload::new(target.clone()).unwrap();
        pending.append(b"new bytes").unwrap();
        std::fs::write(&target, b"other writer").unwrap();
        assert!(pending.finish().is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"other writer");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(StagedDownload::new(target).is_err());
        let target = dir.path().join("complete.bin");
        let mut pending = StagedDownload::new(target.clone()).unwrap();
        pending.append(b"complete\0\xff").unwrap();
        pending.finish().unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"complete\0\xff");
    }
}
