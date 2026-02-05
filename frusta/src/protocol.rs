//! WebSocket protocol message types and builders.

use bytes::Bytes;
use uuid::Uuid;

use crate::viewport::TileMeta;

/// Message header sizes
pub const DPI_SIZE: usize = 4;
pub const IMAGE_DESC_SIZE: usize = 28;
pub const UUID_SIZE: usize = 16;
pub const VIEWPORT_SIZE: usize = 20;
/// Tile payload header: 1 byte slot + 4 bytes x + 4 bytes y + 4 bytes level
pub const TILE_HEADER_SIZE: usize = 13;
/// Progress message size: 1 byte type + 1 byte slot + 4 bytes progress_steps + 4 bytes progress_total
pub const PROGRESS_SIZE: usize = 10;
/// Tile request size (after message type): 1 byte slot + 4 bytes x + 4 bytes y + 4 bytes level
pub const TILE_REQUEST_SIZE: usize = 13;

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

    /// Build an Open response message.
    /// Format: [type: u8][slot: u8][uuid: 16 bytes]
    pub fn open_response(slot: u8, id: Uuid) -> Bytes {
        let mut builder = Self::with_capacity(2 + UUID_SIZE);
        builder.buf.push(MessageType::Open as u8);
        builder.buf.push(slot);
        builder.buf.extend_from_slice(id.as_bytes());
        builder.into_bytes()
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
    /// Format: [type: u8][slot: u8][progress_steps: i32 le][progress_total: i32 le]
    pub fn progress(slot: u8, progress_steps: i32, progress_total: i32) -> Bytes {
        let mut builder = Self::with_capacity(PROGRESS_SIZE);
        builder.buf.push(MessageType::Progress as u8);
        builder.buf.push(slot);
        builder.buf.extend_from_slice(&progress_steps.to_le_bytes());
        builder.buf.extend_from_slice(&progress_total.to_le_bytes());
        builder.into_bytes()
    }

    /// Consume the builder and return the message as Bytes.
    pub fn into_bytes(self) -> Bytes {
        self.buf.into()
    }
}
