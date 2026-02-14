use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a dataset grouping slides by origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// Unix epoch timestamp in milliseconds.
    pub created_at: i64,
    /// Unix epoch timestamp in milliseconds.
    pub updated_at: i64,
    /// Unix epoch timestamp in milliseconds. NULL means not deleted.
    pub deleted_at: Option<i64>,
    /// Arbitrary dataset metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Dataset item for list responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetListItem {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// Unix epoch timestamp in milliseconds.
    pub created_at: i64,
    /// Unix epoch timestamp in milliseconds.
    pub updated_at: i64,
    /// Arbitrary dataset metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Response containing paginated list of datasets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatasetsResponse {
    pub offset: i64,
    pub limit: i64,
    pub full_count: i64,
    pub truncated: bool,
    pub items: Vec<DatasetListItem>,
}

/// Request to update an existing dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDatasetRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Request to create or upsert a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDatasetRequest {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Represents a slide image stored in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub id: Uuid,
    pub dataset: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
    /// Original filename (with extension) extracted from the S3 key
    pub filename: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
    /// Current processing progress in steps of 10,000 tiles
    pub progress_steps: i32,
    /// Total tiles to process (progress_total)
    pub progress_total: i32,
    /// Arbitrary slide metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Request to create a new slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSlideRequest {
    pub id: Uuid,
    pub dataset: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
    /// Original filename (with extension) extracted from the S3 key
    pub filename: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
    /// Arbitrary slide metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Request to update an existing slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSlideRequest {
    pub dataset: Option<Uuid>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub url: Option<String>,
    /// Original filename (with extension) extracted from the S3 key
    pub filename: Option<String>,
    /// Full size of the original slide file in bytes
    pub full_size: Option<i64>,
    /// Arbitrary slide metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Request to list slides with pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSlidesRequest {
    pub dataset_id: Uuid,
    pub offset: i64,
    pub limit: i64,
}

/// Request to list datasets with pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatasetsRequest {
    pub offset: i64,
    pub limit: i64,
}

/// Slide item for list responses (without url for efficiency).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideListItem {
    pub id: Uuid,
    pub dataset: Uuid,
    pub width: i32,
    pub height: i32,
    /// Original filename (with extension) extracted from the S3 key
    pub filename: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
    /// Current processing progress in steps of 10,000 tiles
    pub progress_steps: i32,
    /// Total tiles to process (progress_total)
    pub progress_total: i32,
    /// Arbitrary slide metadata.
    pub metadata: Option<serde_json::Value>,
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

