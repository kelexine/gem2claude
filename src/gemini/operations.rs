// Gemini API high-level operations
// Author: kelexine (https://github.com/kelexine)

use crate::error::{ProxyError, Result};
use crate::gemini::cache_models::{CachedContentResponse, CreateCachedContentRequest};
use crate::models::gemini::{GenerateContentRequest, GenerationConfig, InternalApiRequest, Part};
use crate::oauth::OAuthManager;
use reqwest::Client;
use std::time::{Duration, Instant};
use tracing::{debug, error};

/// Creates a persistent cached content entry in the Gemini API.
pub async fn create_cache(
    http_client: &Client,
    base_url: &str,
    oauth_manager: &OAuthManager,
    model: &str,
    system_instruction: Option<crate::models::gemini::SystemInstruction>,
    contents: Vec<crate::models::gemini::Content>,
) -> Result<String> {
    let url = format!(
        "{}/cachedContents",
        base_url.trim_end_matches("/v1internal")
    );

    let request = CreateCachedContentRequest {
        model: model.to_string(),
        system_instruction,
        contents,
        ttl: Some("300s".to_string()),
    };

    debug!("Creating cache for model: {}", model);

    let http_client = http_client.clone();
    let url = url.clone();
    let request = request.clone();
    let oauth_manager = oauth_manager.clone();

    crate::utils::retry::with_retry("Create Cache", || async {
        let access_token = oauth_manager
            .get_token()
            .await
            .map_err(|e| (500, format!("OAuth error: {}", e)))?;

        let response = http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| (500, format!("HTTP error: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Cache creation failed: HTTP {} - {}", status, error_text);
            return Err((status.as_u16(), error_text));
        }

        let cache_response: CachedContentResponse = response
            .json()
            .await
            .map_err(|e| (500, format!("Invalid response: {}", e)))?;

        Ok(cache_response)
    })
    .await
    .map_err(|(status, body)| match status {
        429 => ProxyError::TooManyRequests(body),
        503 | 504 => ProxyError::ServiceUnavailable(format!("Upstream unavailable: {}", body)),
        _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, body)),
    })
    .map(|res| {
        debug!("Cache created: {}", res.name);
        res.name
    })
}

/// Verifies basic connectivity and authentication.
pub async fn check_connectivity(
    http_client: &Client,
    base_url: &str,
    oauth_manager: &OAuthManager,
    project_id: &str,
) -> Result<Duration> {
    let url = format!("{}:generateContent", base_url);
    let start = Instant::now();

    let request = GenerateContentRequest {
        contents: vec![crate::models::gemini::Content {
            role: "user".to_string(),
            parts: vec![Part::Text {
                text: "hi".to_string(),
                thought: None,
                thought_signature: None,
            }],
        }],
        system_instruction: None,
        generation_config: Some(GenerationConfig {
            max_output_tokens: Some(1),
            ..Default::default()
        }),
        tools: None,
        tool_config: None,
        cached_content: None,
    };

    let wrapped_request = InternalApiRequest {
        model: "gemini-2.5-flash-lite".to_string(),
        project: Some(project_id.to_string()),
        user_prompt_id: Some(format!("health_{}", uuid::Uuid::new_v4().simple())),
        request,
    };

    let access_token = oauth_manager.get_token().await?;

    let response = http_client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&wrapped_request)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| ProxyError::GeminiApi(format!("Health check request failed: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ProxyError::GeminiApi(format!(
            "API check failed: {}",
            error_text
        )));
    }

    Ok(start.elapsed())
}
