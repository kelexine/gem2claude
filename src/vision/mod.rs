//! Vision and image processing module for image-to-text bridge.
//!
//! This module handles the translation of image content blocks from Anthropic's
//! format to Gemini's `InlineData` format. It includes MIME type detection,
//! image validation, and format conversion logic.
//!
//! # Submodules
//!
//! - `models`: Data structures and validation constraints for image data.
//! - `translation`: Logic for converting image blocks between API formats.
//!
//! Author: kelexine (<https://github.com/kelexine>)

pub mod models;
pub mod translation;

pub use translation::translate_image_block;
