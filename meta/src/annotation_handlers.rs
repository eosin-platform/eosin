//! HTTP handlers for slide annotations.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use eosin_common::rbac::UserId;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::{
    annotation_db,
    annotation_models::{
        AnnotationKind, CreateAnnotationRequest, CreateAnnotationSetRequest, ListAnnotationsQuery,
        PolygonPath, UpdateAnnotationRequest, UpdateAnnotationSetRequest,
    },
    bitmask::Bitmask,
    server::AppState,
};

// =============================================================================
// Annotation Set Handlers
// =============================================================================

/// List all annotation sets for a slide.
/// GET /slides/{slide_id}/annotation-sets
pub async fn list_annotation_sets(
    State(state): State<AppState>,
    Path(slide_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let response = annotation_db::list_annotation_sets(&state.pool, slide_id)
        .await
        .map_err(|e| {
            tracing::error!("failed to list annotation sets: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list annotation sets: {}", e),
            )
        })?;

    Ok(Json(response))
}

/// Create a new annotation set for a slide.
/// POST /slides/{slide_id}/annotation-sets
pub async fn create_annotation_set(
    State(state): State<AppState>,
    UserId(user_id): UserId,
    Path(slide_id): Path<Uuid>,
    Json(req): Json<CreateAnnotationSetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let metadata = req.metadata.unwrap_or(serde_json::json!({}));
    let locked = req.locked.unwrap_or(false);

    let annotation_set = annotation_db::create_annotation_set(
        &state.pool,
        slide_id,
        &req.name,
        &req.task_type,
        None, // TODO: Get created_by from auth context
        locked,
        &metadata,
    )
    .await
    .map_err(|e| {
        tracing::error!("failed to create annotation set: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create annotation set: {}", e),
        )
    })?;

    Ok((StatusCode::CREATED, Json(annotation_set)))
}

/// Get a single annotation set by ID.
/// GET /annotation-sets/{id}
pub async fn get_annotation_set(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation_set = annotation_db::get_annotation_set(&state.pool, id)
        .await
        .map_err(|e| {
            tracing::error!("failed to get annotation set: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to get annotation set: {}", e),
            )
        })?;

    match annotation_set {
        Some(s) => Ok(Json(s)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation set {} not found", id),
        )),
    }
}

/// Update an annotation set.
/// PATCH /annotation-sets/{id}
pub async fn update_annotation_set(
    State(state): State<AppState>,
    UserId(user_id): UserId,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateAnnotationSetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation_set = annotation_db::update_annotation_set(
        &state.pool,
        id,
        req.name.as_deref(),
        req.task_type.as_deref(),
        req.locked,
        req.metadata.as_ref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("failed to update annotation set: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update annotation set: {}", e),
        )
    })?;

    match annotation_set {
        Some(s) => Ok(Json(s)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation set {} not found", id),
        )),
    }
}

/// Delete an annotation set.
/// DELETE /annotation-sets/{id}
pub async fn delete_annotation_set(
    State(state): State<AppState>,
    UserId(user_id): UserId,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = annotation_db::delete_annotation_set(&state.pool, id)
        .await
        .map_err(|e| {
            tracing::error!("failed to delete annotation set: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete annotation set: {}", e),
            )
        })?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("annotation set {} not found", id),
        ))
    }
}

// =============================================================================
// Annotation Handlers
// =============================================================================

/// List annotations in a set with optional filters.
/// GET /annotation-sets/{annotation_set_id}/annotations
pub async fn list_annotations(
    State(state): State<AppState>,
    Path(annotation_set_id): Path<Uuid>,
    Query(query): Query<ListAnnotationsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let response = annotation_db::list_annotations(&state.pool, annotation_set_id, &query)
        .await
        .map_err(|e| {
            tracing::error!("failed to list annotations: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list annotations: {}", e),
            )
        })?;

    Ok(Json(response))
}

/// Create a new annotation with geometry.
/// POST /annotation-sets/{annotation_set_id}/annotations
pub async fn create_annotation(
    State(state): State<AppState>,
    UserId(user_id): UserId,
    Path(annotation_set_id): Path<Uuid>,
    Json(req): Json<CreateAnnotationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let metadata = req.metadata.unwrap_or(serde_json::json!({}));
    let source = req.source.as_deref();

    let result = match req.kind {
        AnnotationKind::Point => {
            let geom = parse_point_geometry(&req.geometry)?;
            annotation_db::create_point_annotation(
                &state.pool,
                annotation_set_id,
                &req.label_id,
                None, // TODO: Get created_by from auth context
                &metadata,
                source,
                geom.0,
                geom.1,
            )
            .await
        }
        AnnotationKind::Polygon | AnnotationKind::Polyline => {
            let path = parse_polygon_geometry(&req.geometry)?;
            annotation_db::create_polygon_annotation(
                &state.pool,
                annotation_set_id,
                req.kind,
                &req.label_id,
                None,
                &metadata,
                source,
                &path,
            )
            .await
        }
        AnnotationKind::Ellipse => {
            let geom = parse_ellipse_geometry(&req.geometry)?;
            annotation_db::create_ellipse_annotation(
                &state.pool,
                annotation_set_id,
                &req.label_id,
                None,
                &metadata,
                source,
                geom.0,
                geom.1,
                geom.2,
                geom.3,
                geom.4,
            )
            .await
        }
        AnnotationKind::MaskPatch => {
            let (x0, y0, bitmask) = parse_mask_geometry(&req.geometry)?;
            annotation_db::create_mask_annotation(
                &state.pool,
                annotation_set_id,
                &req.label_id,
                None,
                &metadata,
                source,
                x0,
                y0,
                &bitmask,
            )
            .await
        }
    };

    let annotation = result.map_err(|e| {
        tracing::error!("failed to create annotation: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create annotation: {}", e),
        )
    })?;

    Ok((StatusCode::CREATED, Json(annotation)))
}

/// Get a single annotation by ID.
/// GET /annotations/{id}
pub async fn get_annotation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation = annotation_db::get_annotation(&state.pool, id)
        .await
        .map_err(|e| {
            tracing::error!("failed to get annotation: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to get annotation: {}", e),
            )
        })?;

    match annotation {
        Some(a) => Ok(Json(a)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation {} not found", id),
        )),
    }
}

/// Update an annotation.
/// PATCH /annotations/{id}
pub async fn update_annotation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    UserId(user_id): UserId,
    Json(req): Json<UpdateAnnotationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation = annotation_db::update_annotation(
        &state.pool,
        id,
        req.label_id.as_deref(),
        req.metadata.as_ref(),
        req.geometry.as_ref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("failed to update annotation: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update annotation: {}", e),
        )
    })?;

    match annotation {
        Some(a) => Ok(Json(a)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation {} not found", id),
        )),
    }
}

/// Delete an annotation.
/// DELETE /annotations/{id}
pub async fn delete_annotation(
    State(state): State<AppState>,
    UserId(user_id): UserId,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = annotation_db::delete_annotation(&state.pool, id)
        .await
        .map_err(|e| {
            tracing::error!("failed to delete annotation: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete annotation: {}", e),
            )
        })?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("annotation {} not found", id),
        ))
    }
}

// =============================================================================
// Geometry Parsing Helpers
// =============================================================================

/// Parse point geometry from JSON.
/// Returns (x_level0, y_level0).
fn parse_point_geometry(geom: &JsonValue) -> Result<(f64, f64), (StatusCode, String)> {
    let x = geom["x_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "x_level0 is required for point geometry".to_string(),
        )
    })?;
    let y = geom["y_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "y_level0 is required for point geometry".to_string(),
        )
    })?;
    Ok((x, y))
}

/// Parse polygon geometry from JSON.
fn parse_polygon_geometry(geom: &JsonValue) -> Result<PolygonPath, (StatusCode, String)> {
    let path_array = geom["path"].as_array().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "path is required for polygon/polyline geometry".to_string(),
        )
    })?;

    if path_array.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "path must not be empty".to_string(),
        ));
    }

    let mut points = Vec::with_capacity(path_array.len());
    for (i, point) in path_array.iter().enumerate() {
        if let Some(arr) = point.as_array() {
            if arr.len() >= 2 {
                let x = arr[0].as_f64().ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("invalid x coordinate at index {}", i),
                    )
                })?;
                let y = arr[1].as_f64().ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("invalid y coordinate at index {}", i),
                    )
                })?;
                points.push((x, y));
            } else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("point at index {} must have at least 2 elements", i),
                ));
            }
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("point at index {} must be an array", i),
            ));
        }
    }

    Ok(PolygonPath::from_points(points))
}

/// Parse ellipse geometry from JSON.
/// Returns (cx_level0, cy_level0, radius_x, radius_y, rotation_radians).
fn parse_ellipse_geometry(
    geom: &JsonValue,
) -> Result<(f64, f64, f64, f64, f64), (StatusCode, String)> {
    let cx = geom["cx_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "cx_level0 is required for ellipse geometry".to_string(),
        )
    })?;
    let cy = geom["cy_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "cy_level0 is required for ellipse geometry".to_string(),
        )
    })?;
    let rx = geom["radius_x"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "radius_x is required for ellipse geometry".to_string(),
        )
    })?;
    let ry = geom["radius_y"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "radius_y is required for ellipse geometry".to_string(),
        )
    })?;
    let rotation = geom["rotation_radians"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "rotation_radians is required for ellipse geometry".to_string(),
        )
    })?;

    if rx <= 0.0 || ry <= 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "radius_x and radius_y must be positive".to_string(),
        ));
    }

    Ok((cx, cy, rx, ry, rotation))
}

/// Parse mask geometry from JSON.
/// Returns (x0_level0, y0_level0, Bitmask).
fn parse_mask_geometry(geom: &JsonValue) -> Result<(f64, f64, Bitmask), (StatusCode, String)> {
    let x0 = geom["x0_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "x0_level0 is required for mask geometry".to_string(),
        )
    })?;
    let y0 = geom["y0_level0"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "y0_level0 is required for mask geometry".to_string(),
        )
    })?;
    let width = geom["width"].as_i64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "width is required for mask geometry".to_string(),
        )
    })? as i32;
    let height = geom["height"].as_i64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "height is required for mask geometry".to_string(),
        )
    })? as i32;

    if width <= 0 || height <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "width and height must be positive".to_string(),
        ));
    }

    let encoding = geom["encoding"].as_str().unwrap_or("bitmask");
    if encoding != "bitmask" {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "unsupported encoding '{}', only 'bitmask' is supported",
                encoding
            ),
        ));
    }

    let data_base64 = geom["data_base64"].as_str().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "data_base64 is required for mask geometry".to_string(),
        )
    })?;

    let bitmask = Bitmask::from_base64(width, height, data_base64).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid bitmask data: {}", e),
        )
    })?;

    Ok((x0, y0, bitmask))
}
