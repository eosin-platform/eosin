//! Annotation models for WSI slide annotations.
//!
//! This module provides domain types for storing and retrieving slide annotations,
//! including annotation sets (layers), points, polygons, ellipses, and mask patches.
//! All coordinates are stored in level 0 (full-resolution) slide coordinates.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Type alias for polymorphic metadata JSON.
pub type Metadata = JsonValue;

/// Represents a path for polygon/polyline annotations as a sequence of (x, y) points.
/// All coordinates are in level 0 (full-resolution) slide coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct PolygonPath(pub Vec<(f64, f64)>);

impl PolygonPath {
    /// Create a new empty polygon path.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Create from a vector of points.
    pub fn from_points(points: Vec<(f64, f64)>) -> Self {
        Self(points)
    }

    /// Get the bounding box of the path.
    /// Returns (min_x, min_y, max_x, max_y) or None if empty.
    pub fn bounding_box(&self) -> Option<(f64, f64, f64, f64)> {
        if self.0.is_empty() {
            return None;
        }
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for (x, y) in &self.0 {
            min_x = min_x.min(*x);
            min_y = min_y.min(*y);
            max_x = max_x.max(*x);
            max_y = max_y.max(*y);
        }
        Some((min_x, min_y, max_x, max_y))
    }
}

/// Annotation kind/type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Point,
    Polygon,
    Polyline,
    Ellipse,
    MaskPatch,
}

impl AnnotationKind {
    /// Convert to database string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            AnnotationKind::Point => "point",
            AnnotationKind::Polygon => "polygon",
            AnnotationKind::Polyline => "polyline",
            AnnotationKind::Ellipse => "ellipse",
            AnnotationKind::MaskPatch => "mask_patch",
        }
    }

    /// Parse from database string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "point" => Some(AnnotationKind::Point),
            "polygon" => Some(AnnotationKind::Polygon),
            "polyline" => Some(AnnotationKind::Polyline),
            "ellipse" => Some(AnnotationKind::Ellipse),
            "mask_patch" => Some(AnnotationKind::MaskPatch),
            _ => None,
        }
    }
}

impl std::fmt::Display for AnnotationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Task type for annotation sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Classification,
    Segmentation,
    Qa,
    Measurement,
    Detection,
    Other,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::Classification => "classification",
            TaskType::Segmentation => "segmentation",
            TaskType::Qa => "qa",
            TaskType::Measurement => "measurement",
            TaskType::Detection => "detection",
            TaskType::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "classification" => Some(TaskType::Classification),
            "segmentation" => Some(TaskType::Segmentation),
            "qa" => Some(TaskType::Qa),
            "measurement" => Some(TaskType::Measurement),
            "detection" => Some(TaskType::Detection),
            "other" => Some(TaskType::Other),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Annotation source indicating origin of the annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationSource {
    Human,
    Model,
    Consensus,
}

impl AnnotationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnnotationSource::Human => "human",
            AnnotationSource::Model => "model",
            AnnotationSource::Consensus => "consensus",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "human" => Some(AnnotationSource::Human),
            "model" => Some(AnnotationSource::Model),
            "consensus" => Some(AnnotationSource::Consensus),
            _ => None,
        }
    }
}

// =============================================================================
// Database Row Models
// =============================================================================

/// An annotation set (layer) for a slide.
/// One slide can have multiple annotation sets, e.g., "Tumor vs Benign v1", "Nuclei labels".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationSet {
    pub id: Uuid,
    pub slide_id: Uuid,
    pub name: String,
    pub task_type: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub locked: bool,
    pub metadata: Metadata,
}

/// A single annotation belonging to an annotation set.
/// The actual geometry is stored in a separate table based on the annotation kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: Uuid,
    pub annotation_set_id: Uuid,
    pub kind: String,
    pub label_id: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Metadata,
    pub source: Option<String>,
}

/// Point annotation geometry. Stores x, y coordinates at level 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationPoint {
    pub annotation_id: Uuid,
    pub x_level0: f64,
    pub y_level0: f64,
}

/// Polygon/polyline annotation geometry. Stores path as JSON array of [x, y] points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationPolygon {
    pub annotation_id: Uuid,
    pub path: PolygonPath,
}

/// Ellipse annotation geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationEllipse {
    pub annotation_id: Uuid,
    /// Ellipse center x at level 0
    pub cx_level0: f64,
    /// Ellipse center y at level 0
    pub cy_level0: f64,
    /// Semi-major axis length in pixels at level 0 (x direction before rotation)
    pub radius_x: f64,
    /// Semi-minor axis length in pixels at level 0 (y direction before rotation)
    pub radius_y: f64,
    /// Rotation angle in radians (0 = aligned with x-axis)
    pub rotation_radians: f64,
}

impl AnnotationEllipse {
    /// Compute the axis-aligned bounding box of the ellipse.
    /// Returns (min_x, min_y, max_x, max_y).
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        // For a rotated ellipse, the bounding box can be computed as:
        // half_width = sqrt((radius_x * cos(θ))² + (radius_y * sin(θ))²)
        // half_height = sqrt((radius_x * sin(θ))² + (radius_y * cos(θ))²)
        let cos_t = self.rotation_radians.cos();
        let sin_t = self.rotation_radians.sin();
        let half_width = ((self.radius_x * cos_t).powi(2) + (self.radius_y * sin_t).powi(2)).sqrt();
        let half_height =
            ((self.radius_x * sin_t).powi(2) + (self.radius_y * cos_t).powi(2)).sqrt();
        (
            self.cx_level0 - half_width,
            self.cy_level0 - half_height,
            self.cx_level0 + half_width,
            self.cy_level0 + half_height,
        )
    }
}

/// Mask patch annotation for dense per-pixel annotation.
/// Stores a compact 1-bit-per-pixel bitmask.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationMask {
    pub annotation_id: Uuid,
    /// Left coordinate at level 0
    pub x0_level0: f64,
    /// Top coordinate at level 0
    pub y0_level0: f64,
    /// Width of the patch in pixels
    pub width: i32,
    /// Height of the patch in pixels
    pub height: i32,
    /// Encoding type (currently only "bitmask" is supported)
    pub encoding: String,
    /// Packed bitmask data (1 bit per pixel, row-major order)
    #[serde(skip)]
    pub data: Vec<u8>,
}

impl AnnotationMask {
    /// Compute the bounding box of the mask patch.
    /// Returns (min_x, min_y, max_x, max_y).
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        (
            self.x0_level0,
            self.y0_level0,
            self.x0_level0 + self.width as f64,
            self.y0_level0 + self.height as f64,
        )
    }
}

// =============================================================================
// API Request/Response Types
// =============================================================================

/// Request to create a new annotation set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnnotationSetRequest {
    pub name: String,
    pub task_type: String,
    #[serde(default)]
    pub metadata: Option<Metadata>,
    #[serde(default)]
    pub locked: Option<bool>,
}

/// Request to update an annotation set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAnnotationSetRequest {
    pub name: Option<String>,
    pub task_type: Option<String>,
    pub metadata: Option<Metadata>,
    pub locked: Option<bool>,
}

/// Response for listing annotation sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAnnotationSetsResponse {
    pub items: Vec<AnnotationSet>,
}

/// Geometry for point annotations in API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointGeometry {
    pub x_level0: f64,
    pub y_level0: f64,
}

/// Geometry for polygon/polyline annotations in API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolygonGeometry {
    /// Array of [x, y] coordinate pairs at level 0
    pub path: Vec<(f64, f64)>,
}

/// Geometry for ellipse annotations in API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EllipseGeometry {
    pub cx_level0: f64,
    pub cy_level0: f64,
    pub radius_x: f64,
    pub radius_y: f64,
    pub rotation_radians: f64,
}

/// Geometry for mask patch annotations in API requests/responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaskGeometry {
    pub x0_level0: f64,
    pub y0_level0: f64,
    pub width: i32,
    pub height: i32,
    pub encoding: String,
    /// Base64-encoded packed bitmask data
    pub data_base64: String,
}

/// Unified geometry enum for API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnnotationGeometry {
    Point(PointGeometry),
    Polygon(PolygonGeometry),
    Ellipse(EllipseGeometry),
    Mask(MaskGeometry),
}

/// Request to create a new annotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnnotationRequest {
    pub kind: AnnotationKind,
    pub label_id: String,
    #[serde(default)]
    pub metadata: Option<Metadata>,
    #[serde(default)]
    pub source: Option<String>,
    pub geometry: serde_json::Value,
}

/// Request to update an annotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAnnotationRequest {
    pub label_id: Option<String>,
    pub metadata: Option<Metadata>,
    pub geometry: Option<serde_json::Value>,
}

/// Query parameters for listing annotations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListAnnotationsQuery {
    pub label_id: Option<String>,
    pub kind: Option<String>,
    /// Bounding box filter: x_min
    pub x_min: Option<f64>,
    /// Bounding box filter: y_min
    pub y_min: Option<f64>,
    /// Bounding box filter: x_max
    pub x_max: Option<f64>,
    /// Bounding box filter: y_max
    pub y_max: Option<f64>,
    /// Whether to include mask data in response (default: false for efficiency)
    #[serde(default)]
    pub include_mask_data: Option<bool>,
}

/// Full annotation response with geometry included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationResponse {
    pub id: Uuid,
    pub annotation_set_id: Uuid,
    pub kind: String,
    pub label_id: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Metadata,
    pub source: Option<String>,
    pub geometry: serde_json::Value,
}

/// Response for listing annotations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAnnotationsResponse {
    pub items: Vec<AnnotationResponse>,
}
