//! Prometheus metrics for the meta service.
//!
//! This module provides metrics collectors for monitoring slide and
//! annotation CRUD operations, database performance, and API usage.

use metrics::{counter, histogram};
use std::time::Instant;

/// Helper struct for tracking operation latency.
pub struct LatencyTracker {
    start: Instant,
    operation: &'static str,
}

impl LatencyTracker {
    pub fn new(operation: &'static str) -> Self {
        Self {
            start: Instant::now(),
            operation,
        }
    }

    pub fn finish(self) {
        let duration = self.start.elapsed().as_secs_f64();
        histogram!("meta_operation_duration_seconds", "operation" => self.operation).record(duration);
    }

    pub fn finish_with_status(self, success: bool) {
        let duration = self.start.elapsed().as_secs_f64();
        let status = if success { "success" } else { "error" };
        histogram!("meta_operation_duration_seconds", "operation" => self.operation, "status" => status).record(duration);
    }
}

// =============================================================================
// Slide Metrics
// =============================================================================

/// Record a slide creation.
pub fn slide_created() {
    counter!("meta_slides_created_total").increment(1);
}

/// Record a slide retrieval.
pub fn slide_retrieved() {
    counter!("meta_slides_retrieved_total").increment(1);
}

/// Record a slide update.
pub fn slide_updated() {
    counter!("meta_slides_updated_total").increment(1);
}

/// Record a slide deletion.
pub fn slide_deleted() {
    counter!("meta_slides_deleted_total").increment(1);
}

/// Record slides listed.
pub fn slides_listed(count: usize) {
    counter!("meta_slides_listed_total").increment(1);
    histogram!("meta_slides_list_size").record(count as f64);
}

/// Record slide progress update.
pub fn slide_progress_updated() {
    counter!("meta_slide_progress_updates_total").increment(1);
}

// =============================================================================
// Annotation Set Metrics
// =============================================================================

/// Record an annotation set creation.
pub fn annotation_set_created() {
    counter!("meta_annotation_sets_created_total").increment(1);
}

/// Record an annotation set retrieval.
pub fn annotation_set_retrieved() {
    counter!("meta_annotation_sets_retrieved_total").increment(1);
}

/// Record an annotation set update.
pub fn annotation_set_updated() {
    counter!("meta_annotation_sets_updated_total").increment(1);
}

/// Record an annotation set deletion.
pub fn annotation_set_deleted() {
    counter!("meta_annotation_sets_deleted_total").increment(1);
}

/// Record annotation sets listed.
pub fn annotation_sets_listed(count: usize) {
    counter!("meta_annotation_sets_listed_total").increment(1);
    histogram!("meta_annotation_sets_list_size").record(count as f64);
}

// =============================================================================
// Annotation Metrics
// =============================================================================

/// Record an annotation creation.
pub fn annotation_created(kind: &str) {
    counter!("meta_annotations_created_total", "kind" => kind.to_string()).increment(1);
}

/// Record an annotation retrieval.
pub fn annotation_retrieved(kind: &str) {
    counter!("meta_annotations_retrieved_total", "kind" => kind.to_string()).increment(1);
}

/// Record an annotation update.
pub fn annotation_updated(kind: &str) {
    counter!("meta_annotations_updated_total", "kind" => kind.to_string()).increment(1);
}

/// Record an annotation deletion.
pub fn annotation_deleted() {
    counter!("meta_annotations_deleted_total").increment(1);
}

/// Record annotations listed.
pub fn annotations_listed(count: usize) {
    counter!("meta_annotations_listed_total").increment(1);
    histogram!("meta_annotations_list_size").record(count as f64);
}

// =============================================================================
// Database Metrics
// =============================================================================

/// Record a database query.
pub fn db_query(operation: &str, duration_secs: f64) {
    histogram!("meta_db_query_duration_seconds", "operation" => operation.to_string()).record(duration_secs);
    counter!("meta_db_queries_total", "operation" => operation.to_string()).increment(1);
}

/// Record a database connection acquisition.
pub fn db_connection_acquired(duration_secs: f64) {
    histogram!("meta_db_connection_acquire_seconds").record(duration_secs);
}

/// Record a database error.
pub fn db_error(operation: &str) {
    counter!("meta_db_errors_total", "operation" => operation.to_string()).increment(1);
}

// =============================================================================
// API Metrics
// =============================================================================

/// Record an API request.
pub fn api_request(method: &str, path: &str, status: u16) {
    counter!(
        "meta_api_requests_total",
        "method" => method.to_string(),
        "path" => path.to_string(),
        "status" => status.to_string()
    ).increment(1);
}

/// Record an API request latency.
pub fn api_latency(method: &str, path: &str, duration_secs: f64) {
    histogram!(
        "meta_api_request_duration_seconds",
        "method" => method.to_string(),
        "path" => path.to_string()
    ).record(duration_secs);
}

/// Record authenticated request.
pub fn authenticated_request(user_id: &str) {
    counter!("meta_authenticated_requests_total", "user_id" => user_id.to_string()).increment(1);
}
