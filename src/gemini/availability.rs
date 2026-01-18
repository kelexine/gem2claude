//! Model Availability Service for health tracking and traffic management.
//!
//! This module implements a state machine to track the availability of upstream
//! Gemini models. It categorizes model health based on recent API responses,
//! allowing the proxy to provide early failure feedback to clients and potentially
//! handle intelligent retries or fallbacks.

// Author: kelexine (https://github.com/kelexine)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

/// Represents the current health and availability status of a specific model.
#[derive(Debug, Clone, PartialEq)]
pub enum AvailabilityStatus {
    /// Model is behaving as expected and responding to requests.
    Healthy,
    /// Model returned a transient error (e.g., 429) and is marked for a single retry.
    /// `consumed` tracks if the retry has already been attempted in the current logical turn.
    StickyRetry { reason: String, consumed: bool },
    /// Model is considered unavailable (e.g., 403 No Subscription or persistent 429).
    Terminal { reason: String },
}

impl AvailabilityStatus {
    /// Returns a string representation of the status for logging and metrics.
    pub fn as_str(&self) -> &'static str {
        match self {
            AvailabilityStatus::Healthy => "healthy",
            AvailabilityStatus::StickyRetry { .. } => "sticky_retry",
            AvailabilityStatus::Terminal { .. } => "terminal",
        }
    }
}

/// Service that maintains a global view of model health across all requests.
///
/// It uses an `Arc<RwLock<HashMap>>` to provide thread-safe access to health states,
/// as multiple request handlers will be reading from and updating this service concurrently.
#[derive(Debug)]
pub struct ModelAvailabilityService {
    /// Thread-safe map of Model Identifier -> Current Availability Status.
    health: Arc<RwLock<HashMap<String, AvailabilityStatus>>>,
}

impl ModelAvailabilityService {
    /// Creates a new, empty model availability service.
    pub fn new() -> Self {
        Self {
            health: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for ModelAvailabilityService {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelAvailabilityService {
    /// Marks a model as healthy, clearing any previous error or retry states.
    ///
    /// Should be called after any successful API interaction with the model.
    pub fn mark_healthy(&self, model: &str) {
        let mut health = self.health.write().unwrap();
        // Only mark as healthy if we already have a record (optimization)
        // or if it was previously unhealthy.
        if health.contains_key(model) {
            debug!("Marking model {} as HEALTHY", model);
            health.insert(model.to_string(), AvailabilityStatus::Healthy);
            self.record_metrics(model, "healthy");
        }
    }

    /// Marks a model as having a terminal error (e.g., Quota Exhausted / No Subscription).
    ///
    /// Terminal models will be blocked from receiving further traffic until the service
    /// is restarted or they are manually marked healthy.
    pub fn mark_terminal(&self, model: &str, reason: String) {
        warn!("Marking model {} as TERMINAL: {}", model, reason);
        let mut health = self.health.write().unwrap();
        health.insert(model.to_string(), AvailabilityStatus::Terminal { reason });
        self.record_metrics(model, "terminal");
    }

    /// Marks a model for a "sticky" retry due to a transient failure.
    ///
    /// This state allows for one immediate retry attempt. Terminal models cannot
    /// be downgraded back to a retry state.
    pub fn mark_retry_once(&self, model: &str, reason: String) {
        let mut health = self.health.write().unwrap();

        // Terminal errors are final; do not downgrade to sticky retry.
        if let Some(AvailabilityStatus::Terminal { .. }) = health.get(model) {
            return;
        }

        debug!("Marking model {} as STICKY_RETRY: {}", model, reason);
        health.insert(
            model.to_string(),
            AvailabilityStatus::StickyRetry {
                reason,
                consumed: false,
            },
        );
        self.record_metrics(model, "sticky_retry");
    }

    /// Checks if a model is currently eligible to receive traffic.
    ///
    /// Returns `false` ONLY for models in the `Terminal` state.
    /// `Healthy` and `StickyRetry` models are considered available.
    pub fn is_available(&self, model: &str) -> bool {
        let health = self.health.read().unwrap();
        !matches!(health.get(model), Some(AvailabilityStatus::Terminal { .. }))
    }

    /// Private helper to update Prometheus metrics for model health changes.
    fn record_metrics(&self, model: &str, status: &str) {
        crate::metrics::record_model_health(
            model,
            status,
            &["healthy", "sticky_retry", "terminal"],
        );
    }
}
