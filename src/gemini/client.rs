// Gemini API client with project resolution
// Author: kelexine (https://github.com/kelexine)

use super::{ProjectResolutionRequest, ProjectResolutionResponse};
use crate::config::GeminiConfig;
use crate::error::{ProxyError, Result};
use crate::oauth::OAuthManager;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info, error};

/// Client for the Google Gemini API.
///
/// Handles authentication, request signing, and sending requests to the Gemini API.
/// Includes support for:
/// - Content generation (streaming and blocking)
/// - Context caching
/// - Project ID resolution
pub struct GeminiClient {
    http_client: Client,
    #[allow(dead_code)] 
    config: GeminiConfig,
    oauth_manager: OAuthManager,
    project_id: String,
}

impl GeminiClient {
    /// Create a new Gemini client and resolve project ID via `loadCodeAssist`.
    ///
    /// This method will:
    /// 1. Configure an optimized HTTP client with connection pooling
    /// 2. Authenticate using the OAuth manager
    /// 3. Call the `loadCodeAssist` endpoint to resolve the GCP project ID
    pub async fn new(
        config: &GeminiConfig,
        oauth_manager: OAuthManager,
    ) -> Result<Self> {
        // Configure HTTP client for optimal streaming performance
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .tcp_nodelay(true)
            .use_rustls_tls()
            .build()
            .map_err(|e| ProxyError::Internal(format!("Failed to create HTTP client: {}", e)))?;

        debug!("Created HTTP client with connection pooling and keep-alive");

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
                    // Try to extract error message from JSON response
                    let error_msg = Self::extract_error_message(&response_text)
                        .unwrap_or_else(|| response_text.clone());
                    return Err((status.as_u16(), error_msg));
                }

                let project_response: ProjectResolutionResponse = serde_json::from_str(&response_text)
                    .map_err(|e| (500, format!("Invalid response: {}", e)))?;

                match project_response.cloudaicompanion_project {
                    Some(project_id) => Ok(project_id),
                    None => {
                        // Check for error in response or provide helpful message
                        let error_msg = Self::extract_error_message(&response_text)
                            .unwrap_or_else(|| {
                                "This Google account doesn't have a Gemini Pro subscription.\n\
                                 Please login with a Google One AI Premium or Gemini Advanced account,\n\
                                 or set GOOGLE_CLOUD_PROJECT environment variable for Workspace accounts.".to_string()
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
    
    /// Extract error message from API response JSON
    fn extract_error_message(response_text: &str) -> Option<String> {
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

    /// Call Gemini `generateContent` API (blocking).
    ///
    /// Returns errors immediately for client-side retry, as per Claude API behavior.
    pub async fn generate_content(
        &self,
        request: crate::models::gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<crate::models::gemini::GenerateContentResponse> {
        let url = format!("{}:generateContent", self.config.api_base_url);
        debug!("Calling generateContent API for model: {}", model);

        // Wrap request in internal API structure
        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: model.to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("req_{}", uuid::Uuid::new_v4().simple())),
            request,
        };

        // Per Claude API docs: return errors immediately, let Claude Code handle retries
        let access_token = self.oauth_manager.get_token().await?;

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&wrapped_request)
            .send()
            .await
            .map_err(|e| ProxyError::GeminiApi(format!("HTTP error: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Gemini API error: HTTP {} - Response body: {}",
                status, error_text
            );
            // Return error immediately with proper Claude error type
            return Err(match status.as_u16() {
                429 => ProxyError::TooManyRequests(format!("Gemini API quota exceeded: {}", error_text)),
                529 => ProxyError::Overloaded(format!("Gemini API overloaded: {}", error_text)),
                503 | 504 => ProxyError::ServiceUnavailable(format!("Upstream unavailable: {}", error_text)),
                _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_text)),
            });
        }

        let response_text = response.text().await
            .map_err(|e| ProxyError::GeminiApi(format!("Failed to read response body: {}", e)))?;

        debug!("Raw Gemini response (first 500 chars): {}",
            response_text.chars().take(500).collect::<String>());

        let gemini_response: crate::models::gemini::GenerateContentResponse =
            serde_json::from_str(&response_text)
            .map_err(|e| {
                error!("Failed to parse Gemini response: {}", e);
                error!("Response body: {}", response_text);
                ProxyError::GeminiApi(format!("Response parsing error: {}", e))
            })?;

        debug!("Successfully received Gemini response");
        Ok(gemini_response)
    }

    /// Call Gemini `streamGenerateContent` API for SSE streaming.
    ///
    /// Returns a stream of response chunks.
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

    /// Create a cached content entry via Gemini API.
    ///
    /// Returns the cache resource name (e.g., `cachedContents/abc123`).
    /// The cache TTL is currently set to 5 minutes.
    pub async fn create_cache(
        &self,
        model: &str,
        system_instruction: Option<crate::models::gemini::SystemInstruction>,
        contents: Vec<crate::models::gemini::Content>,
    ) -> Result<String> {
        use crate::gemini::cache_models::{CreateCachedContentRequest, CachedContentResponse};

        let url = format!("{}/cachedContents", self.config.api_base_url.trim_end_matches("/v1internal"));
        
        debug!("Creating cache for model: {}", model);

        let request = CreateCachedContentRequest {
            model: model.to_string(),
            system_instruction,
            contents,
            ttl: Some("300s".to_string()),  // 5 minutes
        };

        debug!("Creating cache for model: {}", model);

        // Clone for retry closure
        let http_client = self.http_client.clone();
        let url = url.clone();
        let request = request.clone();
        let oauth_manager = self.oauth_manager.clone();

        crate::utils::retry::with_retry(
            "Create Cache",
            || async {
                let access_token = oauth_manager.get_token().await
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
            }
        )
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
    /// Check connectivity to Gemini API.
    ///
    /// Sends a minimal `generateContent` request ("hi") to verify API is reachable
    /// and authentication is working.
    pub async fn check_connectivity(&self) -> Result<Duration> {
        let url = format!("{}:generateContent", self.config.api_base_url);

        debug!("Checking connectivity via {}", url);

        let start = std::time::Instant::now();

        // Minimal request: just "hi" to test connectivity
        let request = crate::models::gemini::GenerateContentRequest {
            contents: vec![crate::models::gemini::Content {
                role: "user".to_string(),
                parts: vec![crate::models::gemini::Part::Text {
                    text: "hi".to_string(),
                    thought: None,
                    thought_signature: None,
                }],
            }],
            system_instruction: None,
            generation_config: Some(crate::models::gemini::GenerationConfig {
                max_output_tokens: Some(1),
                temperature: None,
                top_p: None,
                top_k: None,
                stop_sequences: None,
                candidate_count: None,
                thinking_config: None,
            }),
            tools: None,
            tool_config: None,
            cached_content: None,
        };

        // Wrap in internal API structure (same as generate_content)
        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: "gemini-2.5-flash-lite".to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("health_{}", uuid::Uuid::new_v4().simple())),
            request,
        };

        let access_token = self.oauth_manager.get_token().await?;

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&wrapped_request)
            .timeout(Duration::from_secs(5))  // Short timeout for health checks
            .send()
            .await
            .map_err(|e| ProxyError::GeminiApi(format!("Health check request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ProxyError::GeminiApi(format!("API check failed: {}", error_text)));
        }

        let latency = start.elapsed();
        debug!("API connectivity check passed in {:?}", latency);
        
        Ok(latency)
    }
}
mod tests {
    #[test]
    fn test_project_resolution_request_format() {
        use super::ProjectResolutionRequest;

        let request = ProjectResolutionRequest::default();
        let json = serde_json::to_value(&request).unwrap();

        // Check metadata fields match ClientMetadata structure
        assert_eq!(json["metadata"]["ideType"], "GEMINI_CLI");
        assert_eq!(json["metadata"]["platform"], "PLATFORM_UNSPECIFIED");
        assert_eq!(json["metadata"]["pluginType"], "GEMINI");
    }
}
