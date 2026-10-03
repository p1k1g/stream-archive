//! Optional test-only render artifacts; never linked into the product.
use slint::Rgb8Pixel;

pub fn save(pixels: &[Rgb8Pixel], width: usize, height: usize) {
    // Optional snapshots help inspect the compiled layout without changing product code.
    if let Some(directory) = std::env::var_os("STREAM_ARCHIVE_UI_TEST_OUTPUT") {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
        for pixel in pixels {
            bytes.extend([pixel.r, pixel.g, pixel.b]);
        }
        std::fs::write(
            directory.join(format!("{number:02}-{width}x{height}.ppm")),
            bytes,
        )
        .unwrap();
    }
}
