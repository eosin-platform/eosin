use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a slide image stored in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
    /// Current processing progress in steps of 10,000 tiles
    pub progress_steps: i32,
    /// Total tiles to process (progress_total)
    pub progress_total: i32,
}

/// Request to create a new slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSlideRequest {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
}

/// Request to update an existing slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSlideRequest {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub url: Option<String>,
    /// Full size of the original slide file in bytes
    pub full_size: Option<i64>,
}

/// Request to list slides with pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSlidesRequest {
    pub offset: i64,
    pub limit: i64,
}

/// Slide item for list responses (without url for efficiency).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideListItem {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
}

/// Response containing paginated list of slides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSlidesResponse {
    pub offset: i64,
    pub limit: i64,
    pub full_count: i64,
    pub truncated: bool,
    pub items: Vec<SlideListItem>,
}

/// Request to update slide progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSlideProgressRequest {
    pub progress_steps: i32,
    pub progress_total: i32,
}
