// Error handling tests
// Author: kelexine (https://github.com/kelexine)

use gem2claude::error::ProxyError;

#[test]
fn test_error_display_messages() {
    let errors = vec![
        ProxyError::OAuth("Token failed".to_string()),
        ProxyError::GeminiApi("API error".to_string()),
        ProxyError::Translation("Translation failed".to_string()),
        ProxyError::InvalidRequest("Bad request".to_string()),
        ProxyError::TooManyRequests("Rate limited".to_string()),
        ProxyError::ServiceUnavailable("Service down".to_string()),
        ProxyError::Overloaded("Overloaded".to_string()),
    ];
    
    for error in errors {
        let display = format!("{}", error);
        assert!(!display.is_empty(), "Error should have display message");
    }
}

#[test]
fn test_invalid_request_error() {
    let error = ProxyError::InvalidRequest("Missing model field".to_string());
    assert!(format!("{}", error).contains("Missing model field"));
}

#[test]
fn test_rate_limit_error() {
    let error = ProxyError::TooManyRequests("Quota exceeded".to_string());
    assert!(format!("{}", error).contains("Quota exceeded"));
}

#[test]
fn test_overloaded_error() {
    let error = ProxyError::Overloaded("API overloaded".to_string());
    assert!(format!("{}", error).contains("overloaded"));
}

#[test]
fn test_gemini_api_error() {
    let error = ProxyError::GeminiApi("Connection refused".to_string());
    assert!(format!("{}", error).contains("Connection refused"));
}

#[test]
fn test_oauth_error() {
    let error = ProxyError::OAuth("Token refresh failed".to_string());
    assert!(format!("{}", error).contains("Token refresh failed"));
}

#[test]
fn test_translation_error() {
    let error = ProxyError::Translation("Invalid content block".to_string());
    assert!(format!("{}", error).contains("Invalid content block"));
}

#[test]
fn test_service_unavailable_error() {
    let error = ProxyError::ServiceUnavailable("Backend down".to_string());
    assert!(format!("{}", error).contains("Backend down"));
}
