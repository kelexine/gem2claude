// Model Availability Service
// Tracks health status of Gemini models and manages fallback/retry state.
// Author: kelexine (https://github.com/kelexine)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum AvailabilityStatus {
    Healthy,
    StickyRetry { reason: String, consumed: bool },
    Terminal { reason: String },
}

impl AvailabilityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AvailabilityStatus::Healthy => "healthy",
            AvailabilityStatus::StickyRetry { .. } => "sticky_retry",
            AvailabilityStatus::Terminal { .. } => "terminal",
        }
    }
}

#[derive(Debug)]
pub struct ModelAvailabilityService {
    // Map of Model ID -> Health Status
    health: Arc<RwLock<HashMap<String, AvailabilityStatus>>>,
}

impl ModelAvailabilityService {
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
    /// Mark a model as healthy (clears any error state)
    pub fn mark_healthy(&self, model: &str) {
        let mut health = self.health.write().unwrap();
        if health.contains_key(model) {
            debug!("Marking model {} as HEALTHY", model);
            health.insert(model.to_string(), AvailabilityStatus::Healthy);
            self.record_metrics(model, "healthy");
        }
    }

    /// Mark a model as having a terminal error (e.g., Quota Exhausted)
    pub fn mark_terminal(&self, model: &str, reason: String) {
        warn!("Marking model {} as TERMINAL: {}", model, reason);
        let mut health = self.health.write().unwrap();
        health.insert(model.to_string(), AvailabilityStatus::Terminal { reason });
        self.record_metrics(model, "terminal");
    }

    /// Mark a model for retry in the current turn (transient error)
    pub fn mark_retry_once(&self, model: &str, reason: String) {
        let mut health = self.health.write().unwrap();
        
        // Don't downgrade terminal errors
        if let Some(AvailabilityStatus::Terminal { .. }) = health.get(model) {
            return;
        }

        debug!("Marking model {} as STICKY_RETRY: {}", model, reason);
        health.insert(model.to_string(), AvailabilityStatus::StickyRetry { 
            reason, 
            consumed: false 
        });
        self.record_metrics(model, "sticky_retry");
    }

    /// Check if a model is available
    pub fn is_available(&self, model: &str) -> bool {
        let health = self.health.read().unwrap();
        match health.get(model) {
            Some(AvailabilityStatus::Terminal { .. }) => false,
            // Sticky retry logic (if consumed) would go here, 
            // but for now we just report health status.
            _ => true, 
        }
    }

    fn record_metrics(&self, model: &str, status: &str) {
        crate::metrics::record_model_health(
            model, 
            status, 
            &["healthy", "sticky_retry", "terminal"]
        );
    }
}
