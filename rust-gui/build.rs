use image::ImageEncoder;
use image::imageops::FilterType;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

fn write_u16(writer: &mut impl Write, value: u16) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32(writer: &mut impl Write, value: u32) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn generate_windows_icon(
    source_path: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

    let source = image::open(source_path)?.into_rgba8();
    let mut frames = Vec::with_capacity(SIZES.len());

    for size in SIZES {
        let resized = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            resized.as_raw(),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )?;
        frames.push((size, png));
    }

    let directory_size = 6 + 16 * frames.len();
    let mut data_offset = directory_size as u32;
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);

    write_u16(&mut writer, 0)?;
    write_u16(&mut writer, 1)?;
    write_u16(&mut writer, frames.len() as u16)?;

    for (size, data) in &frames {
        let encoded_size = if *size == 256 { 0 } else { *size as u8 };
        writer.write_all(&[encoded_size, encoded_size, 0, 0])?;
        write_u16(&mut writer, 1)?;
        write_u16(&mut writer, 32)?;
        write_u32(&mut writer, data.len() as u32)?;
        write_u32(&mut writer, data_offset)?;
        data_offset += data.len() as u32;
    }

    for (_, data) in &frames {
        writer.write_all(data)?;
    }
    writer.flush()?;
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-changed=assets/stream-archive-icon.png");

    slint_build::compile_with_config(
        "ui/app-window.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Slint application shell");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_dir = PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"),
        );
        let source_path = manifest_dir.join("assets").join("stream-archive-icon.png");
        let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is missing"));
        let icon_path = out_dir.join("stream-archive.ico");

        generate_windows_icon(&source_path, &icon_path)
            .expect("failed to generate Windows application icon");

        let icon_path = icon_path
            .to_str()
            .expect("generated Windows icon path is not valid UTF-8");

        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon(icon_path)
            .set("ProductName", "Stream Archive")
            .set("FileDescription", "Stream Archive")
            .set("OriginalFilename", "StreamArchive.exe");
        resource
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
