use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use image::GenericImageView;

use crate::error::RelayError;
use crate::types::{FileType, MediaMeta};

const THUMB_SIZE: u32 = 220;
const RAR_MAGIC: &[u8] = b"Rar!\x1a\x07";

pub fn contains_rarjpeg(bytes: &[u8]) -> bool {
    matches!(FileType::detect(bytes), Some(FileType::Jpeg | FileType::Png))
        && bytes.windows(RAR_MAGIC.len()).any(|window| window == RAR_MAGIC)
}

pub fn validate(data: &[u8]) -> Result<FileType, RelayError> {
    let file_type =
        FileType::detect(data).ok_or_else(|| RelayError::BadRequest("unsupported media format".into()))?;
    if contains_rarjpeg(data) {
        return Err(RelayError::BadRequest("rarjpeg rejected".into()));
    }
    Ok(file_type)
}

fn to_image(data: &[u8], path: &Path, file_type: FileType) -> Result<image::DynamicImage, RelayError> {
    match file_type {
        FileType::Jpeg | FileType::Png | FileType::WebP | FileType::Gif => {
            image::load_from_memory(data).map_err(|error| RelayError::BadRequest(format!("image decode: {error}")))
        }
        FileType::Mp4 | FileType::WebM => extract_frame(path),
        FileType::Mp3 | FileType::Ogg | FileType::Pdf => {
            Err(RelayError::BadRequest("media has no thumbnail".into()))
        }
    }
}

fn extract_frame(path: &Path) -> Result<image::DynamicImage, RelayError> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path.to_str().unwrap_or_default(),
            "-vframes",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .output()
        .map_err(|error| RelayError::Internal(format!("ffmpeg: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RelayError::BadRequest(format!("ffmpeg: {stderr}")));
    }
    image::load_from_memory(&output.stdout)
        .map_err(|error| RelayError::BadRequest(format!("frame decode: {error}")))
}

pub fn generate(data: &[u8], path: &Path) -> Result<Vec<u8>, RelayError> {
    let file_type = validate(data)?;
    let image = to_image(data, path, file_type)?;
    let (width, height) = image.dimensions();
    let thumb = if width <= THUMB_SIZE && height <= THUMB_SIZE {
        image
    } else {
        image.thumbnail(THUMB_SIZE, THUMB_SIZE)
    };
    let mut buffer = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut buffer), image::ImageFormat::Jpeg)
        .map_err(|error| RelayError::Internal(format!("jpeg encode: {error}")))?;
    Ok(buffer)
}

fn probe_duration(path: &Path) -> Option<u64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str()?,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let seconds: f64 = String::from_utf8_lossy(&output.stdout).trim().parse().ok()?;
    Some((seconds * 1000.0).round() as u64)
}

pub fn compute_meta(data: &[u8], path: &Path) -> Result<MediaMeta, RelayError> {
    let file_type = validate(data)?;
    let size = data.len() as u64;
    match file_type {
        FileType::Jpeg | FileType::Png | FileType::WebP | FileType::Gif => {
            let image = image::load_from_memory(data)
                .map_err(|error| RelayError::BadRequest(format!("image decode: {error}")))?;
            let (width, height) = image.dimensions();
            Ok(MediaMeta {
                mime: file_type.mime().to_string(),
                width,
                height,
                duration_ms: None,
                size,
            })
        }
        FileType::Mp4 | FileType::WebM => {
            let image = extract_frame(path)?;
            let (width, height) = image.dimensions();
            Ok(MediaMeta {
                mime: file_type.mime().to_string(),
                width,
                height,
                duration_ms: probe_duration(path),
                size,
            })
        }
        FileType::Mp3 | FileType::Ogg => {
            let duration_ms = probe_duration(path)
                .ok_or_else(|| RelayError::BadRequest("audio probe failed".into()))?;
            Ok(MediaMeta {
                mime: file_type.mime().to_string(),
                width: 0,
                height: 0,
                duration_ms: Some(duration_ms),
                size,
            })
        }
        FileType::Pdf => Ok(MediaMeta {
            mime: file_type.mime().to_string(),
            width: 0,
            height: 0,
            duration_ms: None,
            size,
        }),
    }
}
