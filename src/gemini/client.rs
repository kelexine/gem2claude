//! Gemini API client for Google's Generative AI models.
//!
//! This module provides the core `GeminiClient` which serves as the primary
//! gateway to Google's Generative AI infrastructure. It handles:
//! - Automated GCP project resolution via `loadCodeAssist`
//! - Advanced HTTP connection management for low-latency streaming
//! - Comprehensive error mapping and retry handling
//! - Integration with the internal model availability service

// Author: kelexine (https://github.com/kelexine)

use super::{ProjectResolutionRequest, ProjectResolutionResponse};
use crate::config::GeminiConfig;
use crate::error::{ProxyError, Result};
use crate::oauth::OAuthManager;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, error, info};

/// Client for the Google Gemini API.
///
/// The `GeminiClient` encapsulates all logic required to communicate with Google's
/// internal API endpoints. It manages an optimized connection pool and high-level
/// authentication flows.
pub struct GeminiClient {
    /// Optimized reqwest client with pooling and keep-alives.
    http_client: Client,
    /// Configuration settings for the Gemini API.
    #[allow(dead_code)]
    config: GeminiConfig,
    /// Manager for Google OAuth2 credentials and token refreshes.
    oauth_manager: OAuthManager,
    /// The resolved Google Cloud Project ID.
    project_id: String,
    /// Service tracking model health to avoid routing to failed models.
    availability_service: super::ModelAvailabilityService,
}

impl GeminiClient {
    /// Create a new Gemini client and resolve project ID via `loadCodeAssist`.
    ///
    /// This method will:
    /// 1. Configure an optimized HTTP client with connection pooling
    /// 2. Authenticate using the OAuth manager
    /// 3. Call the `loadCodeAssist` endpoint to resolve the GCP project ID
    ///
    /// # Arguments
    ///
    /// * `config` - Gemini-specific configuration (timeouts, retry count)
    /// * `oauth_manager` - Initialized OAuth manager for token acquisition
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::ProjectResolution` if the project ID cannot be determined.
    pub async fn new(config: &GeminiConfig, oauth_manager: OAuthManager) -> Result<Self> {
        // Configure HTTP client for optimal streaming performance
        // We use a custom pool and keep-alive to minimize handshake overhead
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

        // Resolve project ID via loadCodeAssist (required for subsequent API calls)
        let project_id =
            Self::resolve_project_id(&http_client, &config.api_base_url, &oauth_manager).await?;

        info!("Successfully resolved project ID: {}", project_id);

        let availability_service = super::ModelAvailabilityService::new();

        Ok(Self {
            http_client,
            config: config.clone(),
            oauth_manager,
            project_id,
            availability_service,
        })
    }

    /// Resolve Cloud AI Companion project ID via loadCodeAssist
    ///
    /// This is a critical bootstrap step. Google's internal APIs often require
    /// an explicit project ID in the payload, even when using user-level credentials.
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
                    let error_msg = Self::extract_error_message(&response_text)
                        .unwrap_or_else(|| response_text.clone());
                    return Err((status.as_u16(), error_msg));
                }

                let project_response: ProjectResolutionResponse = serde_json::from_str(&response_text)
                    .map_err(|e| (500, format!("Invalid response: {}", e)))?;

                match project_response.cloudaicompanion_project {
                    Some(project_id) => Ok(project_id),
                    None => {
                        let error_msg = Self::extract_error_message(&response_text)
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

    /// Returns the resolved Google Cloud Project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Returns the internal HTTP client. Useful for low-level stream testing.
    pub fn client(&self) -> &Client {
        &self.http_client
    }

    /// Returns the active OAuth manager.
    pub fn oauth_manager(&self) -> &OAuthManager {
        &self.oauth_manager
    }

    /// Returns the configured API base URL.
    pub fn base_url(&self) -> &str {
        &self.config.api_base_url
    }

    /// Executes a unary (blocking) content generation request.
    ///
    /// This method maps errors directly to `ProxyError` variants to allow the client
    /// to handle retries idiomatic to the Claude API.
    ///
    /// # Arguments
    ///
    /// * `request` - Gemini-formatted generation request
    /// * `model` - The specific Gemini model identifier
    pub async fn generate_content(
        &self,
        request: crate::models::gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<crate::models::gemini::GenerateContentResponse> {
        // Track availability state (metrics only)
        let is_available = self.availability_service.is_available(model);
        if !is_available {
            debug!(
                "Requesting model {} which is marked unavailable/terminal",
                model
            );
        }

        let url = format!("{}:generateContent", self.config.api_base_url);
        debug!("Calling generateContent API for model: {}", model);

        // Wrap request in internal API structure
        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: model.to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("req_{}", uuid::Uuid::new_v4().simple())),
            request,
        };

        let start_time = std::time::Instant::now();
        let access_token = self.oauth_manager.get_token().await?;

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&wrapped_request)
            .send()
            .await
            .map_err(|e| ProxyError::GeminiApi(format!("HTTP error: {}", e)))?;

        let duration = start_time.elapsed().as_secs_f64();
        let status = response.status();

        crate::metrics::record_gemini_call(model, status.as_u16(), false, duration);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Gemini API error: HTTP {} - Response body: {}",
                status, error_text
            );

            // Update health scoring for subsequent requests
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let reason = if error_text.contains("Daily") {
                    "daily_quota"
                } else {
                    "rate_limit"
                };
                crate::metrics::record_retry_attempt(model, reason);
                if reason == "daily_quota" {
                    self.availability_service
                        .mark_terminal(model, error_text.clone());
                } else {
                    self.availability_service
                        .mark_retry_once(model, error_text.clone());
                }
            } else if status.is_server_error() {
                crate::metrics::record_retry_attempt(model, "server_error");
            }

            return Err(match status.as_u16() {
                429 => ProxyError::TooManyRequests(format!(
                    "Gemini API quota exceeded: {}",
                    error_text
                )),
                529 => ProxyError::Overloaded(format!("Gemini API overloaded: {}", error_text)),
                503 | 504 => {
                    ProxyError::ServiceUnavailable(format!("Upstream unavailable: {}", error_text))
                }
                _ => ProxyError::GeminiApi(format!("HTTP {}: {}", status, error_text)),
            });
        }

        self.availability_service.mark_healthy(model);

        let response_text = response
            .text()
            .await
            .map_err(|e| ProxyError::GeminiApi(format!("Failed to read response body: {}", e)))?;

        let gemini_response: crate::models::gemini::GenerateContentResponse =
            serde_json::from_str(&response_text).map_err(|e| {
                error!("Failed to parse Gemini response: {}", e);
                ProxyError::GeminiApi(format!("Response parsing error: {}", e))
            })?;

        Ok(gemini_response)
    }

    /// Executes a streaming content generation request using Server-Sent Events (SSE).
    ///
    /// The stream is parsed and converted into `GenerateContentResponse` chunks.
    pub async fn stream_generate_content(
        &self,
        request: crate::models::gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<
        impl futures::Stream<Item = Result<crate::models::gemini::GenerateContentResponse>> + Send,
    > {
        let is_available = self.availability_service.is_available(model);
        if !is_available {
            debug!(
                "Streaming model {} which is marked unavailable/terminal",
                model
            );
        }

        let url = format!("{}:streamGenerateContent?alt=sse", self.config.api_base_url);
        debug!("Calling streamGenerateContent API for model: {}", model);

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
            model,
        )
        .await
    }

    /// Creates a persistent cached content entry in the Gemini API.
    ///
    /// Context caching is used to handle large system instructions or repeated prefixes,
    /// significantly reducing latency and cost for subsequent calls.
    pub async fn create_cache(
        &self,
        model: &str,
        system_instruction: Option<crate::models::gemini::SystemInstruction>,
        contents: Vec<crate::models::gemini::Content>,
    ) -> Result<String> {
        use crate::gemini::cache_models::{CachedContentResponse, CreateCachedContentRequest};

        let url = format!(
            "{}/cachedContents",
            self.config.api_base_url.trim_end_matches("/v1internal")
        );

        let request = CreateCachedContentRequest {
            model: model.to_string(),
            system_instruction,
            contents,
            ttl: Some("300s".to_string()), // Default 5 minute TTL
        };

        debug!("Creating cache for model: {}", model);

        let http_client = self.http_client.clone();
        let url = url.clone();
        let request = request.clone();
        let oauth_manager = self.oauth_manager.clone();

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

    /// Verifies the basic connectivity and authentication status of the Gemini API.
    ///
    /// Sends a minimal placeholder request to confirm that the project resolution,
    /// OAuth token acquisition, and upstream response cycle are functional.
    pub async fn check_connectivity(&self) -> Result<Duration> {
        let url = format!("{}:generateContent", self.config.api_base_url);
        let start = std::time::Instant::now();

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

        let wrapped_request = crate::models::gemini::InternalApiRequest {
            model: "gemini-2.5-flash-lite".to_string(),
            project: Some(self.project_id.clone()),
            user_prompt_id: Some(format!("health_{}", uuid::Uuid::new_v4().simple())),
            request,
        };

        let access_token = self.oauth_manager.get_token().await?;

        let response = self
            .http_client
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

        let latency = start.elapsed();
        debug!("API connectivity check passed in {:?}", latency);

        Ok(latency)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_project_resolution_request_format() {
        use super::ProjectResolutionRequest;

        let request = ProjectResolutionRequest::default();
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["metadata"]["ideType"], "GEMINI_CLI");
        assert_eq!(json["metadata"]["platform"], "PLATFORM_UNSPECIFIED");
        assert_eq!(json["metadata"]["pluginType"], "GEMINI");
    }
}
