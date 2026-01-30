// Project resolution logic for Gemini Cloud Code
// Author: kelexine (https://github.com/kelexine)

use super::{ProjectResolutionRequest, ProjectResolutionResponse};
use crate::error::{ProxyError, Result};
use crate::oauth::OAuthManager;
use reqwest::Client;
use tracing::debug;

/// Resolve Cloud AI Companion project ID via loadCodeAssist
pub async fn resolve_project_id(
    client: &Client,
    base_url: &str,
    oauth_manager: &OAuthManager,
) -> Result<String> {
    let url = format!("{}:loadCodeAssist", base_url);
    let request_payload = ProjectResolutionRequest::default();

    debug!("Resolving project ID via {}", url);

    // Clone for retry closure
    let client = client.clone();
    let url = url.clone();
    let request_payload = request_payload.clone();
    let oauth_manager = oauth_manager.clone();

    crate::utils::retry::with_retry(
        "Project Resolution",
        || async {
            let access_token = oauth_manager.get_token().await
                .map_err(|e| (500, format!("OAuth error: {}", e)))?;

            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .json(&request_payload)
                .send()
                .await
                .map_err(|e| (500, format!("HTTP error: {}", e)))?;

            let status = response.status();
            let response_text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                let error_msg = extract_error_message(&response_text)
                    .unwrap_or_else(|| response_text.clone());
                return Err((status.as_u16(), error_msg));
            }

            let project_response: ProjectResolutionResponse = serde_json::from_str(&response_text)
                .map_err(|e| (500, format!("Invalid response: {}", e)))?;

            match project_response.cloudaicompanion_project {
                Some(project_id) => Ok(project_id),
                None => {
                    let error_msg = extract_error_message(&response_text)
                        .unwrap_or_else(|| {
                            "Account check failed: No Gemini Pro subscription detected.\n\
                             Please ensure you are using an account with 'Google One AI Premium' or 'Gemini Advanced'."
                             .to_string()
                        });
                    Err((403, error_msg))
                }
            }
        }
    )
    .await
    .map_err(|(status, body)| match status {
        403 => ProxyError::ProjectResolution(body),
        429 => ProxyError::TooManyRequests(body),
        529 => ProxyError::Overloaded(format!("Gemini API overloaded: {}", body)),
        503 | 504 => ProxyError::ServiceUnavailable(format!("Upstream unavailable: {}", body)),
        _ => ProxyError::ProjectResolution(format!("HTTP {}: {}", status, body)),
    })
}

/// Extracts a user-friendly error message from a Google API JSON response.
pub fn extract_error_message(response_text: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct ErrorResponse {
        error: Option<ErrorDetail>,
    }

    #[derive(serde::Deserialize)]
    struct ErrorDetail {
        message: Option<String>,
        status: Option<String>,
    }

    if let Ok(error_resp) = serde_json::from_str::<ErrorResponse>(response_text) {
        if let Some(error) = error_resp.error {
            return error.message.or(error.status);
        }
    }
    None
}
