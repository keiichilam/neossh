//! Owned-display clipboard image producer and pixel verifier for native QA.
use std::io::{self, Write};
fn pixels() -> Vec<u8> {
    (0..512)
        .flat_map(|n| {
            [
                (n % 256) as u8,
                (n / 2) as u8,
                91,
                if n % 2 == 0 { 255 } else { 64 },
            ]
        })
        .collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = std::env::args().nth(1) {
        if path == "--get-text" {
            let mut clipboard = arboard::Clipboard::new()?;
            print!("{}", clipboard.get_text()?);
            return Ok(());
        }
        if path == "--text" {
            let text = std::env::args().nth(2).ok_or("Expected clipboard text")?;
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(text)?;
            println!("READY");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            return Ok(());
        }
        let image = image::open(path)?.to_rgba8();
        assert_eq!(image.dimensions(), (32, 16));
        assert_eq!(image.as_raw(), &pixels());
        println!("PIXELS_OK 32 16");
    } else {
        let mut clipboard = arboard::Clipboard::new()?;
        clipboard.set_image(arboard::ImageData {
            width: 32,
            height: 16,
            bytes: std::borrow::Cow::Owned(pixels()),
        })?;
        println!("READY");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        drop(clipboard);
    }
    Ok(())
}
