// Translation module - Anthropic ↔ Gemini API translation
// Author: kelexine (https://github.com/kelexine)

pub mod request;
pub mod response;
pub mod signature_store;
pub mod streaming;
pub mod tools;

pub use request::translate_request;
pub use response::translate_response;
pub use signature_store::{store_signature, get_signature};
