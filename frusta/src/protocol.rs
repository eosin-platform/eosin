//! WebSocket protocol message types and builders.

use bytes::Bytes;
use histion_common::streams::SlideCreatedEvent;
use uuid::Uuid;

use crate::viewport::TileMeta;

/// Message header sizes
pub const DPI_SIZE: usize = 4;
pub const IMAGE_DESC_SIZE: usize = 28;
pub const VIEWPORT_SIZE: usize = 20;
/// Tile payload header: 1 byte slot + 4 bytes x + 4 bytes y + 4 bytes level
pub const TILE_HEADER_SIZE: usize = 13;
/// Progress message size: 1 byte type + 16 bytes uuid + 4 bytes progress_steps + 4 bytes progress_total
pub const PROGRESS_SIZE: usize = 25;
/// Tile request size (after message type): 1 byte slot + 4 bytes x + 4 bytes y + 4 bytes level
pub const TILE_REQUEST_SIZE: usize = 13;
/// Rate limited message size: 1 byte type only (no payload)
pub const RATE_LIMITED_SIZE: usize = 1;

/// WebSocket message types for the frusta protocol.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Update = 0,
    Open = 1,
    Close = 2,
    ClearCache = 3,
    Progress = 4,
    RequestTile = 5,
    RateLimited = 6,
    SlideCreated = 7,
}

impl TryFrom<u8> for MessageType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MessageType::Update),
            1 => Ok(MessageType::Open),
            2 => Ok(MessageType::Close),
            3 => Ok(MessageType::ClearCache),
            4 => Ok(MessageType::Progress),
            5 => Ok(MessageType::RequestTile),
            6 => Ok(MessageType::RateLimited),
            7 => Ok(MessageType::SlideCreated),
            _ => Err(()),
        }
    }
}

/// Builder for outgoing WebSocket messages.
pub struct MessageBuilder {
    buf: Vec<u8>,
}

impl MessageBuilder {
    /// Create a new message builder with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    /// Build a tile data message.
    /// Format: [slot: u8][x: u32 le][y: u32 le][level: u32 le][data: bytes]
    pub fn tile_data(slot: u8, meta: &TileMeta, data: &[u8]) -> Bytes {
        let mut builder = Self::with_capacity(TILE_HEADER_SIZE + data.len());
        builder.buf.push(slot);
        builder.buf.extend_from_slice(&meta.x.to_le_bytes());
        builder.buf.extend_from_slice(&meta.y.to_le_bytes());
        builder.buf.extend_from_slice(&meta.level.to_le_bytes());
        builder.buf.extend_from_slice(data);
        builder.into_bytes()
    }

    /// Build a progress message.
    /// Format: [type: u8][uuid: 16 bytes][progress_steps: i32 le][progress_total: i32 le]
    pub fn progress(slide_id: Uuid, progress_steps: i32, progress_total: i32) -> Bytes {
        let mut builder = Self::with_capacity(PROGRESS_SIZE);
        builder.buf.push(MessageType::Progress as u8);
        builder.buf.extend_from_slice(slide_id.as_bytes());
        builder.buf.extend_from_slice(&progress_steps.to_le_bytes());
        builder.buf.extend_from_slice(&progress_total.to_le_bytes());
        builder.into_bytes()
    }

    /// Build a RateLimited notification message.
    /// Format: [type: u8]
    pub fn rate_limited() -> Bytes {
        let mut builder = Self::with_capacity(RATE_LIMITED_SIZE);
        builder.buf.push(MessageType::RateLimited as u8);
        builder.into_bytes()
    }

    /// Build a SlideCreated message.
    /// Format: [type: u8][json payload]
    /// The JSON payload is a serialized `SlideCreatedEvent`.
    pub fn slide_created(event: &SlideCreatedEvent) -> Bytes {
        let json = serde_json::to_vec(event).expect("SlideCreatedEvent serialization");
        let mut builder = Self::with_capacity(1 + json.len());
        builder.buf.push(MessageType::SlideCreated as u8);
        builder.buf.extend_from_slice(&json);
        builder.into_bytes()
    }

    /// Consume the builder and return the message as Bytes.
    pub fn into_bytes(self) -> Bytes {
        self.buf.into()
    }
}
