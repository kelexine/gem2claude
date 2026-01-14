// Vision translation logic
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::models::anthropic::{ContentBlock, ImageSource};
use crate::models::gemini::InlineData;
use super::models::{ImageFormat, validate_image_size};
use base64::Engine;

/// Translate Claude image block to Gemini InlineData
pub fn translate_image_block(block: &ContentBlock) -> Result<InlineData> {
    // Extract Image variant
    let (media_type_opt, data) = match block {
        ContentBlock::Image { source, .. } => match source {
            ImageSource::Base64 { media_type, data } => {
                (media_type.clone(), data.clone())
            }
        },
        _ => {
            return Err(ProxyError::InvalidRequest(
                "Expected Image content block".to_string()
            ));
        }
    };

    // Decode base64 to validate and get size
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| ProxyError::InvalidRequest(format!("Invalid base64 image data: {}", e)))?;

    // Detect MIME type from magic bytes if not provided
    let media_type = match media_type_opt {
        Some(mt) => mt,
        None => detect_mime_type(&decoded)
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Could not detect image format from data".to_string()
            ))?,
    };

    // Validate format is supported
    ImageFormat::from_mime_type(&media_type)
        .ok_or_else(|| {
            ProxyError::InvalidRequest(format!("Unsupported image format: {}", media_type))
        })?;

    // Validate size
    validate_image_size(decoded.len())
        .map_err(|e| ProxyError::InvalidRequest(e))?;

    // Gemini expects base64 data as-is (no prefix like "data:image/png;base64,")
    Ok(InlineData {
        mime_type: media_type,
        data,
    })
}

/// Detect MIME type from magic bytes at start of image data
fn detect_mime_type(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }

    // Check magic bytes for common image formats
    if data.starts_with(b"\xFF\xD8\xFF") {
        Some("image/jpeg".to_string())
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png".to_string())
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif".to_string())
    } else if data.starts_with(b"RIFF") && data[8..12] == *b"WEBP" {
        Some("image/webp".to_string())
    } else if data[4..12] == *b"ftypheic" || data[4..12] == *b"ftypheix" {
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
