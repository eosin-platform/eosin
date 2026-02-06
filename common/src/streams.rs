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

/// Events sent over the `slide_progress` NATS topic.
///
/// The first byte is a tag that distinguishes the event type:
///   - `0` = Progress update (8 more bytes: progress_steps i32 LE, progress_total i32 LE)
///   - `1` = Slide created  (variable: see `SlideCreatedEvent`)
#[derive(Debug, Clone)]
pub enum SlideEvent {
    Progress(SlideProgressEvent),
    Created(SlideCreatedEvent),
}

/// Slide processing progress update.
#[derive(Debug, Clone, Copy)]
pub struct SlideProgressEvent {
    pub progress_steps: i32,
    pub progress_total: i32,
}

/// A new slide was inserted into meta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideCreatedEvent {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub filename: String,
    pub full_size: i64,
    pub url: String,
}

// -- wire helpers --

const TAG_PROGRESS: u8 = 0;
const TAG_CREATED: u8 = 1;

impl SlideEvent {
    /// Serialize to bytes for NATS.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            SlideEvent::Progress(p) => {
                let mut buf = Vec::with_capacity(9);
                buf.push(TAG_PROGRESS);
                buf.extend_from_slice(&p.progress_steps.to_le_bytes());
                buf.extend_from_slice(&p.progress_total.to_le_bytes());
                buf
            }
            SlideEvent::Created(c) => {
                let json = serde_json::to_vec(c).expect("SlideCreatedEvent serialization");
                let mut buf = Vec::with_capacity(1 + json.len());
                buf.push(TAG_CREATED);
                buf.extend_from_slice(&json);
                buf
            }
        }
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            // Legacy 8-byte format (no tag) — treat as progress for
            // backwards compatibility during rolling deploys.
            return None;
        }
        match data[0] {
            TAG_PROGRESS => {
                if data.len() < 9 {
                    return None;
                }
                Some(SlideEvent::Progress(SlideProgressEvent {
                    progress_steps: i32::from_le_bytes(data[1..5].try_into().ok()?),
                    progress_total: i32::from_le_bytes(data[5..9].try_into().ok()?),
                }))
            }
            TAG_CREATED => {
                let created: SlideCreatedEvent = serde_json::from_slice(&data[1..]).ok()?;
                Some(SlideEvent::Created(created))
            }
            _ => None,
        }
    }
}

impl SlideProgressEvent {
    /// Serialize to a `SlideEvent::Progress` payload for NATS.
    pub fn to_bytes(&self) -> Vec<u8> {
        SlideEvent::Progress(*self).to_bytes()
    }
}
