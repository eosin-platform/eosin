//! Prometheus metrics for the frusta WebSocket tile server.
//!
//! This module provides metrics collectors for monitoring tile streaming,
//! WebSocket connections, rate limiting, and worker queue performance.

use metrics::{counter, gauge, histogram};

/// Increment the WebSocket connection count.
pub fn websocket_connections_inc() {
    gauge!("frusta_websocket_connections").increment(1);
}

/// Decrement the WebSocket connection count.
pub fn websocket_connections_dec() {
    gauge!("frusta_websocket_connections").decrement(1);
}

/// Record a tile request.
pub fn tile_requested(slide_id: &str, level: u32) {
    counter!("frusta_tiles_requested_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record a tile being sent to the client.
pub fn tile_sent(slide_id: &str, level: u32, size_bytes: usize) {
    counter!("frusta_tiles_sent_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
    histogram!("frusta_tile_size_bytes", "slide_id" => slide_id.to_string(), "level" => level.to_string()).record(size_bytes as f64);
}

/// Record a tile being skipped because it's no longer visible.
pub fn tile_skipped_not_visible(slide_id: &str, level: u32) {
    counter!("frusta_tiles_skipped_total", "slide_id" => slide_id.to_string(), "level" => level.to_string(), "reason" => "not_visible").increment(1);
}

/// Record a tile being skipped because it was already delivered.
pub fn tile_skipped_already_delivered(slide_id: &str, level: u32) {
    counter!("frusta_tiles_skipped_total", "slide_id" => slide_id.to_string(), "level" => level.to_string(), "reason" => "already_delivered").increment(1);
}

/// Record a tile being skipped because of cancellation.
pub fn tile_skipped_cancelled(slide_id: &str, level: u32) {
    counter!("frusta_tiles_skipped_total", "slide_id" => slide_id.to_string(), "level" => level.to_string(), "reason" => "cancelled").increment(1);
}

/// Record a tile fetch failure.
pub fn tile_fetch_failed(slide_id: &str, level: u32) {
    counter!("frusta_tile_fetch_failures_total", "slide_id" => slide_id.to_string(), "level" => level.to_string()).increment(1);
}

/// Record tile fetch latency.
pub fn tile_fetch_latency(slide_id: &str, level: u32, duration_secs: f64) {
    histogram!("frusta_tile_fetch_duration_seconds", "slide_id" => slide_id.to_string(), "level" => level.to_string()).record(duration_secs);
}

/// Record a viewport update.
pub fn viewport_updated(slot: u8) {
    counter!("frusta_viewport_updates_total", "slot" => slot.to_string()).increment(1);
}

/// Record slide being opened.
pub fn slide_opened(slide_id: &str, slot: u8) {
    counter!("frusta_slides_opened_total", "slide_id" => slide_id.to_string(), "slot" => slot.to_string()).increment(1);
}

/// Record slide being closed.
pub fn slide_closed(slide_id: &str, slot: u8) {
    counter!("frusta_slides_closed_total", "slide_id" => slide_id.to_string(), "slot" => slot.to_string()).increment(1);
}

/// Set the current work queue size.
pub fn work_queue_size(size: usize) {
    gauge!("frusta_work_queue_size").set(size as f64);
}

/// Record a rate limited request.
pub fn rate_limited(client_ip: &str) {
    counter!("frusta_rate_limited_total", "client_ip" => client_ip.to_string()).increment(1);
}

/// Record active workers count.
pub fn active_workers(count: usize) {
    gauge!("frusta_active_workers").set(count as f64);
}

/// Record cache prune operation.
pub fn cache_pruned(slot: u8, tiles_removed: usize) {
    counter!("frusta_cache_prunes_total", "slot" => slot.to_string()).increment(1);
    histogram!("frusta_cache_prune_tiles_removed", "slot" => slot.to_string()).record(tiles_removed as f64);
}

/// Record messages received from client.
pub fn message_received(msg_type: &str) {
    counter!("frusta_messages_received_total", "type" => msg_type.to_string()).increment(1);
}

/// Record messages sent to client.
pub fn message_sent(msg_type: &str) {
    counter!("frusta_messages_sent_total", "type" => msg_type.to_string()).increment(1);
}
