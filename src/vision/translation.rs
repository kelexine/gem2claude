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
    let (media_type, data) = match block {
        ContentBlock::Image { source } => match source {
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

    // Validate format is supported
    ImageFormat::from_mime_type(&media_type)
        .ok_or_else(|| {
            ProxyError::InvalidRequest(format!("Unsupported image format: {}", media_type))
        })?;

    // Decode base64 to validate and get size
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| ProxyError::InvalidRequest(format!("Invalid base64 image data: {}", e)))?;

    // Validate size
    validate_image_size(decoded.len())
        .map_err(|e| ProxyError::InvalidRequest(e))?;

    // Gemini expects base64 data as-is (no prefix like "data:image/png;base64,")
    Ok(InlineData {
        mime_type: media_type,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // use crate::models::claude::{ImageBlock, ImageSource}; // Removed as per instruction

    #[test]
    fn test_translate_valid_image() {
        // Tiny 1x1 PNG (base64 encoded)
        let png_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: png_data.to_string(),
            },
        };

        let result = translate_image_block(&image);
        assert!(result.is_ok());
        
        let inline_data = result.unwrap();
        assert_eq!(inline_data.mime_type, "image/png");
        assert_eq!(inline_data.data, png_data);
    }

    #[test]
    fn test_invalid_mime_type() {
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/bmp".to_string(), // Not supported
                data: "dGVzdA==".to_string(),
            },
        };

        let result = translate_image_block(&image);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_base64() {
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: "not-valid-base64!!!".to_string(),
            },
        };

        let result = translate_image_block(&image);
        assert!(result.is_err());
    }
}
