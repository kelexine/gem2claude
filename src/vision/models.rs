// Vision models and types
// Author: kelexine (https://github.com/kelexine)

use serde::{Deserialize, Serialize};

/// Image source type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    #[serde(rename = "base64")]
    Base64 { media_type: String, data: String },
    // Future: URL support
    // Url { url: String },
}

/// Supported image formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
    Heic,
}

impl ImageFormat {
    /// Get MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Png => "image/png",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Heic => "image/heic",
        }
    }

    /// Try to detect format from MIME type
    pub fn from_mime_type(mime: &str) -> Option<Self> {
        match mime.to_lowercase().as_str() {
            "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
            "image/png" => Some(ImageFormat::Png),
            "image/webp" => Some(ImageFormat::WebP),
            "image/gif" => Some(ImageFormat::Gif),
            "image/heic" => Some(ImageFormat::Heic),
            _ => None,
        }
    }
}

/// Validation limits
pub const MAX_IMAGE_SIZE_BYTES: usize = 20 * 1024 * 1024; // 20MB (Gemini limit)

/// Validate image data size
pub fn validate_image_size(data_len: usize) -> Result<(), String> {
    if data_len > MAX_IMAGE_SIZE_BYTES {
        return Err(format!(
            "Image size {} bytes exceeds maximum of {} bytes (20MB)",
            data_len, MAX_IMAGE_SIZE_BYTES
        ));
    }
    Ok(())
}
