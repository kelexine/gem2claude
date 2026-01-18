//! Vision models and types for image processing.
//!
//! This module defines the core data structures used to represent image data
//! and supported formats within the bridge, along with validation constants
//! and functions to ensure compliance with Gemini's API constraints.

// Author: kelexine (https://github.com/kelexine)

use serde::{Deserialize, Serialize};

/// Represents the source of an image content block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    /// Image data provided as a base64-encoded string.
    #[serde(rename = "base64")]
    Base64 {
        /// The MIME type of the image (e.g., "image/jpeg").
        media_type: String,
        /// The raw base64-encoded image data.
        data: String,
    },
}

/// Supported image formats for the Gemini API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// JPEG format (.jpg, .jpeg)
    Jpeg,
    /// Portable Network Graphics (.png)
    Png,
    /// WebP format (.webp)
    WebP,
    /// Graphics Interchange Format (.gif)
    Gif,
    /// High Efficiency Image File Format (.heic, .heif)
    Heic,
}

impl ImageFormat {
    /// Returns the standard MIME type string for the image format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Png => "image/png",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Heic => "image/heic",
        }
    }

    /// Attempts to determine the `ImageFormat` from a MIME type string.
    ///
    /// This is used to validate that an incoming request specifies a format
    /// that the bridge (and Gemini) can process.
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

/// The maximum allowed size for a single image, as defined by Gemini's constraints.
pub const MAX_IMAGE_SIZE_BYTES: usize = 20 * 1024 * 1024; // 20MB

/// Validates that the provided image data length does not exceed 20MB.
///
/// # Errors
///
/// Returns an `Err` containing a descriptive message if the size is exceeded.
pub fn validate_image_size(data_len: usize) -> Result<(), String> {
    if data_len > MAX_IMAGE_SIZE_BYTES {
        return Err(format!(
            "Image size ({} bytes) exceeds the Google Gemini maximum of 20MB ({} bytes).",
            data_len, MAX_IMAGE_SIZE_BYTES
        ));
    }
    Ok(())
}
