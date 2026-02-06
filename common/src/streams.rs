use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod topics {
    use uuid::Uuid;

    pub const CACHE_MISS: &str = "histion.cache.miss";
    pub const PROCESS_SLIDE: &str = "histion.process.slide";

    pub fn tile_data(id: Uuid) -> String {
        format!("histion.tile.{}", id)
    }

    pub fn slide_progress(id: Uuid) -> String {
        format!("histion.slide.progress.{}", id)
    }

    /// Wildcard topic to subscribe to progress events for ALL slides.
    pub const SLIDE_PROGRESS_ALL: &str = "histion.slide.progress.*";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSlideEvent {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMissEvent {
    pub id: Uuid,
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

impl CacheMissEvent {
    pub fn hash(&self) -> String {
        format!("{}.{}.{}.{}", self.id, self.level, self.x, self.y)
    }
}

/// Slide processing progress event.
/// Sent over NATS to notify clients of processing progress.
/// Payload format: progress_steps (4 bytes LE i32) | progress_total (4 bytes LE i32)
#[derive(Debug, Clone, Copy)]
pub struct SlideProgressEvent {
    pub progress_steps: i32,
    pub progress_total: i32,
}

impl SlideProgressEvent {
    /// Serialize to 8-byte payload for NATS
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&self.progress_steps.to_le_bytes());
        buf[4..8].copy_from_slice(&self.progress_total.to_le_bytes());
        buf
    }

    /// Deserialize from 8-byte payload
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            progress_steps: i32::from_le_bytes(data[0..4].try_into().ok()?),
            progress_total: i32::from_le_bytes(data[4..8].try_into().ok()?),
        })
    }
}
