//! Prometheus metrics for the storage service.
//!
//! This module provides metrics collectors for monitoring tile storage
//! operations, cache performance, and gRPC API usage.

use metrics::{counter, gauge, histogram};

// =============================================================================
// Tile Operations
// =============================================================================

/// Record a tile retrieval (get_tile).
pub fn tile_get(slide_id: &str, level: u32) {
    counter!("storage_tile_gets_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record a successful tile retrieval.
pub fn tile_get_success(slide_id: &str, level: u32, size_bytes: usize) {
    counter!("storage_tile_get_success_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
    histogram!("storage_tile_size_bytes", "slide_id" => slide_id.to_string(), "level" => level.to_string(), "operation" => "get").record(size_bytes as f64);
}

/// Record a tile not found.
pub fn tile_get_not_found(slide_id: &str, level: u32) {
    counter!("storage_tile_get_not_found_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record a tile retrieval error.
pub fn tile_get_error(slide_id: &str, level: u32) {
    counter!("storage_tile_get_errors_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record tile get latency.
pub fn tile_get_latency(slide_id: &str, level: u32, duration_secs: f64) {
    histogram!("storage_tile_get_duration_seconds", "slide_id" => slide_id.to_string(), "level" => level.to_string()).record(duration_secs);
}

/// Record a tile write (put_tile).
pub fn tile_put(slide_id: &str, level: u32) {
    counter!("storage_tile_puts_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record a successful tile write.
pub fn tile_put_success(slide_id: &str, level: u32, size_bytes: usize) {
    counter!("storage_tile_put_success_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
    histogram!("storage_tile_size_bytes", "slide_id" => slide_id.to_string(), "level" => level.to_string(), "operation" => "put").record(size_bytes as f64);
}

/// Record a tile write error.
pub fn tile_put_error(slide_id: &str, level: u32) {
    counter!("storage_tile_put_errors_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record tile put latency.
pub fn tile_put_latency(slide_id: &str, level: u32, duration_secs: f64) {
    histogram!("storage_tile_put_duration_seconds", "slide_id" => slide_id.to_string(), "level" => level.to_string()).record(duration_secs);
}

// =============================================================================
// Cache Metrics
// =============================================================================

/// Record a cache miss event published.
pub fn cache_miss_published(slide_id: &str, level: u32) {
    counter!("storage_cache_miss_events_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record a cache miss publish failure.
pub fn cache_miss_publish_failed(slide_id: &str, level: u32) {
    counter!("storage_cache_miss_publish_failures_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

// =============================================================================
// Cluster Metrics
// =============================================================================

/// Record cluster node count.
pub fn cluster_nodes(count: usize) {
    gauge!("storage_cluster_nodes").set(count as f64);
}

/// Record cluster operation.
pub fn cluster_operation(operation: &str) {
    counter!("storage_cluster_operations_total", "operation" => operation.to_string()).increment(1);
}

// =============================================================================
// Health Check Metrics
// =============================================================================

/// Record a health check.
pub fn health_check() {
    counter!("storage_health_checks_total").increment(1);
}

// =============================================================================
// gRPC Metrics
// =============================================================================

/// Record a gRPC request.
pub fn grpc_request(method: &str) {
    counter!("storage_grpc_requests_total", "method" => method.to_string()).increment(1);
}

/// Record a gRPC request latency.
pub fn grpc_latency(method: &str, duration_secs: f64) {
    histogram!("storage_grpc_request_duration_seconds", "method" => method.to_string()).record(duration_secs);
}

/// Record a gRPC error.
pub fn grpc_error(method: &str, code: &str) {
    counter!("storage_grpc_errors_total", "method" => method.to_string(), "code" => code.to_string()).increment(1);
}

// =============================================================================
// Disk Metrics
// =============================================================================

/// Record disk write bytes.
pub fn disk_bytes_written(bytes: usize) {
    counter!("storage_disk_bytes_written_total").increment(bytes as u64);
}

/// Record disk read bytes.
pub fn disk_bytes_read(bytes: usize) {
    counter!("storage_disk_bytes_read_total").increment(bytes as u64);
}

/// Record directory creation.
pub fn directory_created() {
    counter!("storage_directories_created_total").increment(1);
}
