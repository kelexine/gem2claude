// Gemini API client with project resolution
// Author: kelexine (https://github.com/kelexine)

use super::{ProjectResolutionRequest, ProjectResolutionResponse};
use crate::config::GeminiConfig;
use crate::error::{ProxyError, Result};
use crate::oauth::OAuthManager;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info, error};

pub struct GeminiClient {
    http_client: Client,
    #[allow(dead_code)]  // Will be used in Phase 2 for timeouts and retries
    config: GeminiConfig,
    oauth_manager: OAuthManager,
    project_id: String,
}

impl GeminiClient {
    /// Create a new Gemini client and resolve project ID
    pub async fn new(
        config: &GeminiConfig,
        oauth_manager: OAuthManager,
    ) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .pool_max_idle_per_host(100)
            .use_rustls_tls()
            .build()
            .map_err(|e| ProxyError::Internal(format!("Failed to create HTTP client: {}", e)))?;

        debug!("Created HTTP client with timeout: {}s", config.timeout_seconds);

        // Resolve project ID via loadCodeAssist
        let project_id = Self::resolve_project_id(
            &http_client,
            &config.api_base_url,
            &oauth_manager,
        )
        .await?;

        info!("Successfully resolved project ID");

        Ok(Self {
            http_client,
            config: config.clone(),
            oauth_manager,
            project_id,
        })
    }

    /// Resolve Cloud AI Companion project ID via loadCodeAssist
    async fn resolve_project_id(
        client: &Client,
        base_url: &str,
        oauth_manager: &OAuthManager,
    ) -> Result<String> {
        let url = format!("{}:loadCodeAssist", base_url);
        let request_payload = ProjectResolutionRequest::default();
        
        debug!("Resolving project ID via {}", url);
        debug!("Request payload: {:?}", request_payload);

        let access_token = oauth_manager.get_token().await?;

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request_payload)
            .send()
            .await
            .map_err(|e| {
                ProxyError::ProjectResolution(format!("Request failed: {}", e))
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ProxyError::ProjectResolution(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        let project_response: ProjectResolutionResponse = response
            .json()
            .await
            .map_err(|e| {
                ProxyError::ProjectResolution(format!("Invalid response: {}", e))
            })?;

        debug!("Project resolution response: {:?}", project_response);

        // Handle optional cloudaicompanionProject field
        match project_response.cloudaicompanion_project {
            Some(project_id) => {
                debug!("Project ID resolved: {}", project_id);
                Ok(project_id)
            }
            None => {
                Err(ProxyError::ProjectResolution(
                    "No cloudaicompanionProject in response".to_string()
                ))
            }
        }
    }

    /// Get the resolved project ID
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Get the HTTP client
    pub fn client(&self) -> &Client {
        &self.http_client
    }

    /// Get the OAuth manager
    pub fn oauth_manager(&self) -> &OAuthManager {
        &self.oauth_manager
    }

    /// Get the API base_url
    pub fn base_url(&self) -> &str {
        &self.config.api_base_url
    }

    /// Call Gemini generateContent API with intelligent retry logic
    pub async fn generate_content(
        &self,
        request: crate::models::gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<crate::models::gemini::GenerateContentResponse> {
        let url = format!("{}:generateContent", self.config.api_base_url);
        debug!("Calling generateContent API for model: {}", model);

        // Wrap request in internal API structure (prepare once)
        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: model.to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("req_{}", uuid::Uuid::new_v4().simple())),
            request: request.clone(),
        };

        // Clone what we need for the retry closure
        let http_client = self.http_client.clone();
        let url = url.clone();
        let oauth_manager = self.oauth_manager.clone();
        let wrapped_request = wrapped_request.clone();

        // Use intelligent retry with Google's retryDelay hints
        crate::utils::retry::with_retry(
            "Gemini API",
            || async {
                let access_token = oauth_manager.get_token().await
                    .map_err(|e| (500, format!("OAuth error: {}", e)))?;

                let response = http_client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", access_token))
                    .header("Content-Type", "application/json")
                    .json(&wrapped_request)
                    .send()
                    .await
                    .map_err(|e| (500, format!("HTTP error: {}", e)))?;

                let status = response.status();
                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    error!(
                        "Gemini API error: HTTP {} - Response body: {}",
                        status, error_text
                    );
                    return Err((status.as_u16(), error_text));
                }

                let response_text = response.text().await
                    .map_err(|e| (500, format!("Failed to read response body: {}", e)))?;
                
                debug!("Raw Gemini response (first 500 chars): {}", 
                    response_text.chars().take(500).collect::<String>());
                
                let gemini_response: crate::models::gemini::GenerateContentResponse = 
                    serde_json::from_str(&response_text)
                    .map_err(|e| {
                        error!("Failed to parse Gemini response: {}", e);
                        error!("Response body: {}", response_text);
                        (500, format!("Response parsing error: {}", e))
                    })?;

                debug!("Successfully received Gemini response");
                Ok(gemini_response)
            },
        )
        .await
        .map_err(|(status, error_body)| {
            ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_body))
        })
    }

    /// Call Gemini streamGenerateContent API for SSE streaming
    pub async fn stream_generate_content(
        &self,
        request: crate::models::gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<impl futures::Stream<Item = Result<crate::models::gemini::GenerateContentResponse>> + Send> {
        let url = format!("{}:streamGenerateContent?alt=sse", self.config.api_base_url);

        debug!("Calling streamGenerateContent API for model: {}", model);

        // Wrap request in internal API structure
        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: model.to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("req_{}", uuid::Uuid::new_v4().simple())),
            request,
        };

        let request_body = serde_json::to_string(&wrapped_request)
            .map_err(|e| ProxyError::Internal(format!("Failed to serialize request: {}", e)))?;

        crate::gemini::streaming::stream_generate_content(
            &self.http_client,
            url,
            request_body,
            &self.oauth_manager,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_resolution_request_format() {
        let request = ProjectResolutionRequest::default();
        let json = serde_json::to_value(&request).unwrap();
        
        assert_eq!(json["metadata"]["clientType"], "EDITOR_CLIENT");
        assert_eq!(json["metadata"]["product"], "code_assist");
    }
}
