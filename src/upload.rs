//! Bounded reads from an open local file and a unique remote staging name.
use rand::RngCore;
use std::{fs::File, io::Read, path::Path, time::SystemTime};

pub(crate) struct Upload {
    file: File,
    modified: Option<SystemTime>,
    pub total: u64,
    pub staging: String,
    pub destination: String,
    pub created: bool,
    pub pending: u64,
}
/// Unique hidden staging path in the destination's own folder, so the final
/// rename never crosses filesystems.
pub(crate) fn staging_path(
    destination: &str,
    prefix: &str,
    random: &[u8; 16],
) -> Result<String, String> {
    let (parent, name) = destination.rsplit_once('/').unwrap_or((".", destination));
    if name.is_empty() || matches!(name, "." | "..") {
        return Err("Choose a remote filename for the upload.".into());
    }
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let staging = format!("{parent}/{prefix}{suffix}.part");
    if staging.len() > 4096 {
        return Err("Remote folder path is too long for an upload staging file.".into());
    }
    Ok(staging)
}
impl Upload {
    pub fn new(local: &Path, destination: String) -> Result<Self, String> {
        Self::staged(local, destination, ".ssh4-upload-")
    }
    /// Staging for an edit-session replacement; the distinct prefix identifies
    /// leftovers from interrupted edit syncs.
    pub fn for_replace(local: &Path, destination: String) -> Result<Self, String> {
        Self::staged(local, destination, ".ssh4-edit-")
    }
    fn staged(local: &Path, destination: String, prefix: &str) -> Result<Self, String> {
        let metadata =
            std::fs::metadata(local).map_err(|e| format!("Cannot read upload source: {e}"))?;
        if !metadata.is_file() {
            return Err("Choose a regular local file to upload.".into());
        }
        let file = File::open(local).map_err(|e| format!("Cannot open upload source: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("Choose a regular local file to upload.".into());
        }
        let mut random = [0u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut random)
            .map_err(|e| e.to_string())?;
        let staging = staging_path(&destination, prefix, &random)?;
        Ok(Self {
            total: metadata.len(),
            modified: metadata.modified().ok(),
            file,
            staging,
            destination,
            created: false,
            pending: 0,
        })
    }
    pub fn read_chunk(&mut self, acknowledged: u64) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; 32768];
        let count = loop {
            match self.file.read(&mut bytes) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                result => break result.map_err(|e| format!("Cannot read upload source: {e}"))?,
            }
        };
        bytes.truncate(count);
        let end = acknowledged
            .checked_add(count as u64)
            .ok_or("Upload is too large.")?;
        if end > self.total || count == 0 && end != self.total {
            return Err("Local file size changed during upload.".into());
        }
        if count == 0 {
            let metadata = self.file.metadata().map_err(|e| e.to_string())?;
            if metadata.len() != self.total || metadata.modified().ok() != self.modified {
                return Err("Local file changed during upload.".into());
            }
        }
        self.pending = count as u64;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_sources_fail_and_remote_staging_stays_in_destination_folder() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        std::fs::write(&source, b"first").unwrap();
        let mut upload = Upload::new(&source, "/folder/file.bin".into()).unwrap();
        assert!(upload.staging.starts_with("/folder/.ssh4-upload-"));
        assert_eq!(upload.read_chunk(0).unwrap(), b"first");
        std::fs::write(&source, b"changed longer").unwrap();
        assert!(upload.read_chunk(5).is_err());
        assert!(Upload::new(dir.path(), "/file".into()).is_err());
        assert!(Upload::new(&source, "/folder/..".into()).is_err());
        let edit = Upload::for_replace(&source, "/folder/file.bin".into()).unwrap();
        assert!(edit.staging.starts_with("/folder/.ssh4-edit-"));
    }
    #[test]
    fn staging_names_are_unique_hidden_siblings_of_the_destination() {
        let name = staging_path("/a b/文件 ' ;.txt", ".ssh4-edit-", &[0xab; 16]).unwrap();
        assert_eq!(name, format!("/a b/.ssh4-edit-{}.part", "ab".repeat(16)));
        assert_ne!(
            name,
            staging_path("/a b/x", ".ssh4-edit-", &[0xac; 16]).unwrap()
        );
        assert_eq!(
            staging_path("/file", ".p-", &[0; 16]).unwrap(),
            format!("/.p-{}.part", "00".repeat(16))
        );
        assert!(staging_path("relative", ".p-", &[0; 16])
            .unwrap()
            .starts_with("./.p-"));
        for bad in ["/folder/", "/folder/.", "/folder/.."] {
            assert!(staging_path(bad, ".p-", &[0; 16]).is_err(), "{bad}");
        }
        assert!(staging_path(&format!("/{}/x", "d".repeat(4090)), ".p-", &[0; 16]).is_err());
    }
}
