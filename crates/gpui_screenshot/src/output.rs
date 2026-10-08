use std::{
    env, fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use image::RgbaImage;
use sha2::{Digest as _, Sha256};

/// Set to rewrite goldens that are missing or differ.
pub const UPDATE_VAR: &str = "UPDATE_GOLDENS";
/// Set to print every image the harness writes to stdout as a [`PRINT_MARKER`] line.
pub const PRINT_VAR: &str = "PRINT_SCREENSHOTS";
/// Starts each printed line: `GPUI-SCREENSHOT <sha256> <path> <base64 PNG>`, with the path
/// relative to the workspace root (the nearest folder above the file with a `Cargo.lock`).
/// `script/screenshots-from-log` turns the lines back into files.
pub const PRINT_MARKER: &str = "GPUI-SCREENSHOT";

/// The PNG text keyword that records which adapter drew an image.
const SOURCE_KEYWORD: &str = "Source";

pub(crate) fn flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty() && value != "0")
}

/// An image read back from a PNG, with the adapter recorded in it, if any.
pub(crate) struct Decoded {
    pub image: RgbaImage,
    pub source: Option<String>,
}

pub(crate) fn encode_png(image: &RgbaImage, source: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder.add_text_chunk("Software".into(), "gpui-pre-screenshot".into())?;
    encoder.add_text_chunk(SOURCE_KEYWORD.into(), latin1(source))?;
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())?;
    writer.finish()?;
    Ok(bytes)
}

pub(crate) fn decode_png(bytes: &[u8]) -> Result<Decoded> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8();
    let source = png::Decoder::new(io::Cursor::new(bytes))
        .read_info()
        .ok()
        .and_then(|reader| {
            reader
                .info()
                .uncompressed_latin1_text
                .iter()
                .find(|chunk| chunk.keyword == SOURCE_KEYWORD)
                .map(|chunk| chunk.text.clone())
        });
    Ok(Decoded { image, source })
}

/// Writes `bytes` to `path`, creating its folders, and prints it when [`PRINT_VAR`] is set.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating folder {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    if flag(PRINT_VAR) {
        print(path, bytes);
    }
    Ok(())
}

pub(crate) fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            Err(error).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

fn print(path: &Path, bytes: &[u8]) {
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let line = format!(
        "\n{PRINT_MARKER} {digest} {} {data}\n",
        workspace_relative(path).display()
    );
    // Straight to the process's stdout: the test harness captures `print!`, and would show the
    // line only for failing tests.
    let mut stdout = io::stdout().lock();
    stdout.write_all(line.as_bytes()).ok();
    stdout.flush().ok();
}

/// `path` relative to the nearest folder above it that has a `Cargo.lock`, else `path` itself.
fn workspace_relative(path: &Path) -> PathBuf {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.ancestors()
        .skip(1)
        .find(|folder| folder.join("Cargo.lock").is_file())
        .and_then(|root| path.strip_prefix(root).ok())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.clone())
}

fn latin1(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == ' ' || c.is_ascii_graphic() {
                c
            } else {
                '?'
            }
        })
        .collect()
}
