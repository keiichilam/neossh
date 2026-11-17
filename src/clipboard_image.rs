//! Off-thread clipboard capture with owned PNG staging and a small preview.
use image::ImageEncoder;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

static BUSY: AtomicBool = AtomicBool::new(false);
struct CaptureGuard;
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
pub struct Captured {
    pub file: tempfile::NamedTempFile,
    pub width: u32,
    pub height: u32,
    pub preview: egui::ColorImage,
}
pub struct Job {
    result: mpsc::Receiver<Result<Captured, String>>,
}
impl Job {
    pub fn start() -> Result<Self, String> {
        Self::start_with(|| {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| format!("Cannot open clipboard: {e}"))?;
            let pixels = clipboard
                .get_image()
                .map_err(|e| format!("Cannot read a clipboard image: {e}"))?;
            drop(clipboard);
            encode(pixels)
        })
    }
    fn start_with(
        capture: impl FnOnce() -> Result<Captured, String> + Send + 'static,
    ) -> Result<Self, String> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "A clipboard image is still being read. Try again shortly.")?;
        let guard = CaptureGuard;
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("clipboard-image".into())
            .spawn(move || {
                let _guard = guard;
                let captured = capture();
                // Dropping the receiver also drops a late result and its temporary file.
                let _ = sender.send(captured);
            })
            .map_err(|e| format!("Cannot start clipboard reader: {e}"))?;
        Ok(Self { result })
    }
    pub fn poll(&self) -> Option<Result<Captured, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Clipboard reader ended without an image.".into())),
        }
    }
}

fn encode(pixels: arboard::ImageData<'_>) -> Result<Captured, String> {
    let count = pixels
        .width
        .checked_mul(pixels.height)
        .ok_or("Clipboard image dimensions are too large.")?;
    if pixels.width == 0
        || pixels.height == 0
        || pixels.width > 16384
        || pixels.height > 16384
        || count > 16 * 1024 * 1024
    {
        return Err("Clipboard images must have between 1 and 16777216 pixels, with each side at most 16384 pixels.".into());
    }
    if count.checked_mul(4) != Some(pixels.bytes.len()) {
        return Err("Clipboard image has an invalid pixel buffer.".into());
    }
    let (width, height) = (pixels.width as u32, pixels.height as u32);
    let mut file = tempfile::Builder::new()
        .prefix("ssh4-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| format!("Cannot create clipboard PNG: {e}"))?;
    if file.path().to_str().is_none() {
        return Err("Clipboard staging requires a Unicode temporary-folder path.".into());
    }
    image::codecs::png::PngEncoder::new(file.as_file_mut())
        .write_image(
            &pixels.bytes,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Cannot encode clipboard PNG: {e}"))?;
    file.as_file()
        .sync_all()
        .map_err(|e| format!("Cannot flush clipboard PNG: {e}"))?;
    let original = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
        width,
        height,
        pixels.bytes.as_ref(),
    )
    .ok_or("Invalid clipboard pixels.")?;
    let thumb = image::imageops::thumbnail(&original, 320, 200);
    let preview = egui::ColorImage::from_rgba_unmultiplied(
        [thumb.width() as usize, thumb.height() as usize],
        thumb.as_raw(),
    );
    Ok(Captured {
        file,
        width,
        height,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_capture_drops_late_png_and_does_not_allow_parallel_readers() {
        let (ready, path) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let job = Job::start_with(move || {
            let capture = encode(arboard::ImageData {
                width: 1,
                height: 1,
                bytes: std::borrow::Cow::Owned(vec![0, 0, 0, 255]),
            })?;
            ready.send(capture.file.path().to_owned()).unwrap();
            wait.recv().unwrap();
            Ok(capture)
        })
        .unwrap();
        let path = path
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(path.exists());
        assert!(Job::start_with(|| unreachable!()).is_err());
        drop(job);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while BUSY.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!path.exists());
    }
    #[test]
    fn png_pixels_round_trip_and_temporary_files_are_removed_on_drop() {
        let rgba = vec![255, 0, 0, 255, 0, 0, 255, 64];
        let capture = encode(arboard::ImageData {
            width: 2,
            height: 1,
            bytes: std::borrow::Cow::Borrowed(&rgba),
        })
        .unwrap();
        let path = capture.file.path().to_path_buf();
        assert_eq!(image::open(&path).unwrap().to_rgba8().into_raw(), rgba);
        assert_eq!((capture.width, capture.height), (2, 1));
        drop(capture);
        assert!(!path.exists());
        for (width, height) in [(0, 1), (usize::MAX, 2), (16385, 1), (2, 2)] {
            assert!(encode(arboard::ImageData {
                width,
                height,
                bytes: std::borrow::Cow::Borrowed(&[])
            })
            .is_err());
        }
    }
}
