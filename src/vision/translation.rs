//! Vision translation logic for converting image data between API formats.
//!
//! This module provides robust image processing, including MIME type detection
//! from magic bytes and validation against Gemini's supported image configurations.
//!
//! Author: kelexine (<https://github.com/kelexine>)

use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, ImageSource};
use crate::models::gemini::InlineData;
use super::models::{ImageFormat, validate_image_size};
use base64::Engine;

/// Translates a Claude-formatted image content block into Gemini's `InlineData` format.
///
/// This function performs several steps:
/// 1. Extracts the base64 data and optional media type from the Anthropic block.
/// 2. Decodes the base64 data to validate its integrity and determine its size.
/// 3. Detects the MIME type if not explicitly provided, using magic byte analysis.
/// 4. Validates that the image format and size meet Gemini's API constraints.
///
/// # Arguments
///
/// * `block` - The `ContentBlock` received from an Anthropic message request.
///
/// # Returns
///
/// A `Result` containing the `InlineData` structure required by the Gemini API.
///
/// # Errors
///
/// Returns a `ProxyError::InvalidRequest` if:
/// * The block is not an image block.
/// * The base64 data is malformed.
/// * The image format is unsupported or cannot be detected.
/// * The image exceeds size limits.
pub fn translate_image_block(block: &ContentBlock) -> Result<InlineData> {
    // Extract Image variant details
    let (media_type_opt, data) = match block {
        ContentBlock::Image { source, .. } => match source {
            ImageSource::Base64 { media_type, data } => {
                (media_type.clone(), data.clone())
            }
        },
        _ => {
            return Err(ProxyError::InvalidRequest(
                "Expected Image content block for vision processing".to_string()
            ));
        }
    };

    // Decode base64 to validate and get raw byte size for constraint checking
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| ProxyError::InvalidRequest(format!("Invalid base64 image data: {}", e)))?;

    // Detect MIME type from magic bytes if not provided by the client
    let media_type = match media_type_opt {
        Some(mt) => mt,
        None => detect_mime_type(&decoded)
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Could not detect image format from data. Please provide a media_type.".to_string()
            ))?,
    };

    // Validate that the detected/provided format is supported by the bridge
    ImageFormat::from_mime_type(&media_type)
        .ok_or_else(|| {
            ProxyError::InvalidRequest(format!("Unsupported image format: {}", media_type))
        })?;

    // Validate image size against Gemini's specific limitations (e.g., 20MB limit)
    validate_image_size(decoded.len())
        .map_err(ProxyError::InvalidRequest)?;

    // Gemini expects the base64 data string as-is (without any "data:image/..." URI prefixes)
    Ok(InlineData {
        mime_type: media_type,
        data,
    })
}

/// Detects the MIME type of an image by analyzing its initial "magic bytes".
///
/// This is a lightweight implementation that covers the most common web image formats
/// supported by LLMs today.
///
/// Supported formats: JPEG, PNG, GIF, WebP, HEIC.
fn detect_mime_type(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }

    // Check magic bytes for common image formats
    if data.starts_with(b"\xFF\xD8\xFF") {
        // JPEG starts with FF D8 FF
        Some("image/jpeg".to_string())
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        // PNG signature: 89 50 4E 47 0D 0A 1A 0A
        Some("image/png".to_string())
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        // GIF versions
        Some("image/gif".to_string())
    } else if data.starts_with(b"RIFF") && data[8..12] == *b"WEBP" {
        // WebP uses RIFF container with WEBP type
        Some("image/webp".to_string())
    } else if data[4..12] == *b"ftypheic" || data[4..12] == *b"ftypheix" {
        // HEIC brand in ISO Base Media File Format (BMFF)
        Some("image/heic".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_valid_image() {
        // Tiny 1x1 PNG (base64 encoded)
        let png_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: Some("image/png".to_string()),
                data: png_data.to_string(),
            },
            cache_control: None,
        };

        let result = translate_image_block(&image);
        assert!(result.is_ok());
        
        let inline_data = result.unwrap();
        assert_eq!(inline_data.mime_type, "image/png");
        assert_eq!(inline_data.data, png_data);
    }

    #[test]
    fn test_image_without_media_type() {
        // Same PNG but without media_type - should detect it
        let png_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: None,  // Missing!
                data: png_data.to_string(),
            },
            cache_control: None,
        };

        let result = translate_image_block(&image);
        assert!(result.is_ok());
        
        let inline_data = result.unwrap();
        assert_eq!(inline_data.mime_type, "image/png");  // Should be detected
    }

    #[test]
    fn test_invalid_mime_type() {
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: Some("image/bmp".to_string()), // Not supported
                data: "dGVzdA==".to_string(),
            },
            cache_control: None,
        };

        let result = translate_image_block(&image);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_base64() {
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: Some("image/png".to_string()),
                data: "not-valid-base64!!!".to_string(),
            },
            cache_control: None,
        };

        let result = translate_image_block(&image);
        assert!(result.is_err());
    }
}
