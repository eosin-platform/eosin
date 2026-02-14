//! Internal HTTP server for service-to-service communication.
//!
//! This server does not require authentication. The user_id is passed in
//! request bodies for operations that need it.

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    annotation_db,
    annotation_models::{
        AnnotationKind, ListAnnotationsQuery, PolygonPath,
    },
    bitmask::Bitmask,
    db,
    metrics,
    models::{
        CreateDatasetRequest, CreateDatasetSourceRequest, CreateSlideRequest, ListDatasetsRequest,
        ListSlidesRequest, UpdateDatasetRequest,
        UpdateSlideProgressRequest, UpdateSlideRequest,
    },
};

use super::AppState;

// =============================================================================
// Internal Request Types (include user_id where needed)
// =============================================================================

/// Request to create a new annotation set (internal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnnotationSetRequest {
    pub name: String,
    pub task_type: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub locked: Option<bool>,
    /// User ID who created this annotation set
    #[serde(default)]
    pub created_by: Option<Uuid>,
}

/// Request to create a new annotation (internal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnnotationRequest {
    pub kind: AnnotationKind,
    pub label_id: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub source: Option<String>,
    pub geometry: serde_json::Value,
    /// User ID who created this annotation
    #[serde(default)]
    pub created_by: Option<Uuid>,
}

/// Request to update an annotation set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAnnotationSetRequest {
    pub name: Option<String>,
    pub task_type: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub locked: Option<bool>,
}

/// Request to update an annotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAnnotationRequest {
    pub label_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub geometry: Option<serde_json::Value>,
}

// =============================================================================
// Server Setup
// =============================================================================

pub async fn run_server(cancel: CancellationToken, port: u16, state: AppState) -> Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Health checks
        .route("/readyz", get(health))
        .route("/healthz", get(health))
        // Slide routes
        .route("/slides", get(list_slides).post(create_slide))
        .route(
            "/slides/{id}",
            get(get_slide).patch(update_slide).delete(delete_slide),
        )
        .route("/slides/{id}/progress", put(update_slide_progress))
        // Dataset routes
        .route("/dataset", get(list_datasets).post(create_dataset))
        .route(
            "/dataset/{dataset_id}",
            get(get_dataset).patch(update_dataset).delete(delete_dataset),
        )
        .route(
            "/dataset/{dataset_id}/sources",
            get(list_dataset_sources).post(create_dataset_source),
        )
        .route(
            "/dataset/{dataset_id}/sources/{source_id}",
            axum::routing::delete(delete_dataset_source),
        )
        // Annotation set routes
        .route(
            "/slides/{slide_id}/annotation-sets",
            get(list_annotation_sets).post(create_annotation_set),
        )
        .route(
            "/annotation-sets/{id}",
            get(get_annotation_set)
                .patch(update_annotation_set)
                .delete(delete_annotation_set),
        )
        // Annotation routes
        .route(
            "/annotation-sets/{annotation_set_id}/annotations",
            get(list_annotations).post(create_annotation),
        )
        .route(
            "/annotations/{id}",
            get(get_annotation)
                .patch(update_annotation)
                .delete(delete_annotation),
        )
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .context("failed to bind internal server")?;
    tracing::info!(%addr, "starting internal meta HTTP server");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            cancel.cancelled().await;
        })
        .await
        .context("internal server failed")?;

    tracing::info!("internal server stopped gracefully");
    Ok(())
}

// =============================================================================
// Slide Handlers
// =============================================================================

/// Health check endpoint
pub async fn health() -> impl IntoResponse {
    "OK"
}

/// Create a new slide
pub async fn create_slide(
    State(state): State<AppState>,
    Json(req): Json<CreateSlideRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let slide = db::insert_slide(
        &state.pool,
        req.id,
        req.dataset_id,
        req.width,
        req.height,
        &req.url,
        &req.filename,
        req.full_size,
        req.metadata.as_ref(),
    )
    .await
    .map_err(|e| {
        metrics::db_error("insert_slide");
        tracing::error!("failed to create slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create slide: {}", e),
        )
    })?;

    metrics::slide_created();
    Ok((StatusCode::CREATED, Json(slide)))
}

/// Get a slide by ID
pub async fn get_slide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let slide = db::get_slide(&state.pool, id).await.map_err(|e| {
        metrics::db_error("get_slide");
        tracing::error!("failed to get slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to get slide: {}", e),
        )
    })?;

    match slide {
        Some(s) => {
            metrics::slide_retrieved();
            Ok(Json(s))
        }
        None => Err((StatusCode::NOT_FOUND, format!("slide {} not found", id))),
    }
}

/// Update a slide by ID
pub async fn update_slide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSlideRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let slide = db::update_slide(
        &state.pool,
        id,
        req.dataset,
        req.width,
        req.height,
        req.url.as_deref(),
        req.filename.as_deref(),
        req.full_size,
        req.metadata.as_ref(),
    )
    .await
    .map_err(|e| {
        metrics::db_error("update_slide");
        tracing::error!("failed to update slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update slide: {}", e),
        )
    })?;

    match slide {
        Some(s) => {
            metrics::slide_updated();
            Ok(Json(s))
        }
        None => Err((StatusCode::NOT_FOUND, format!("slide {} not found", id))),
    }
}

/// Delete a slide by ID
pub async fn delete_slide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = db::delete_slide(&state.pool, id).await.map_err(|e| {
        metrics::db_error("delete_slide");
        tracing::error!("failed to delete slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to delete slide: {}", e),
        )
    })?;

    if deleted {
        metrics::slide_deleted();
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("slide {} not found", id)))
    }
}

/// List slides with pagination
pub async fn list_slides(
    State(state): State<AppState>,
    Query(req): Query<ListSlidesRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if req.limit <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be positive".to_string(),
        ));
    }
    if req.offset < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "offset must be non-negative".to_string(),
        ));
    }

    let limit = req.limit.min(1000);

    let response = db::list_slides(&state.pool, req.dataset_id, req.offset, limit)
        .await
        .map_err(|e| {
            metrics::db_error("list_slides");
            tracing::error!("failed to list slides: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list slides: {}", e),
            )
        })?;

    metrics::slides_listed(response.items.len());
    Ok(Json(response))
}

/// Update slide progress by ID
pub async fn update_slide_progress(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSlideProgressRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let updated =
        db::update_slide_progress(&state.pool, id, req.progress_steps, req.progress_total)
            .await
            .map_err(|e| {
                metrics::db_error("update_slide_progress");
                tracing::error!("failed to update slide progress: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to update slide progress: {}", e),
                )
            })?;

    if updated {
        metrics::slide_progress_updated();
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("slide {} not found", id)))
    }
}

/// Create or upsert a dataset.
pub async fn create_dataset(
    State(state): State<AppState>,
    Json(req): Json<CreateDatasetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dataset = db::upsert_dataset(
        &state.pool,
        req.id,
        &req.name,
        req.description.as_deref(),
        req.credit.as_deref(),
        req.private,
        req.metadata.as_ref(),
    )
    .await
    .map_err(|e| {
        metrics::db_error("upsert_dataset");
        tracing::error!("failed to upsert dataset: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to upsert dataset: {}", e),
        )
    })?;

    Ok((StatusCode::CREATED, Json(dataset)))
}

/// List datasets with pagination.
pub async fn list_datasets(
    State(state): State<AppState>,
    Query(req): Query<ListDatasetsRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if req.limit <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be positive".to_string(),
        ));
    }
    if req.offset < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "offset must be non-negative".to_string(),
        ));
    }

    let limit = req.limit.min(1000);
    let response = db::list_datasets(&state.pool, req.offset, limit, true)
        .await
        .map_err(|e| {
            metrics::db_error("list_datasets");
            tracing::error!("failed to list datasets: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list datasets: {}", e),
            )
        })?;

    Ok(Json(response))
}

/// Get a dataset by ID.
/// Returns NOT_FOUND when dataset is soft-deleted.
pub async fn get_dataset(
    State(state): State<AppState>,
    Path(dataset_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dataset = db::get_dataset(&state.pool, dataset_id).await.map_err(|e| {
        metrics::db_error("get_dataset");
        tracing::error!("failed to get dataset: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to get dataset: {}", e),
        )
    })?;

    match dataset {
        Some(d) => Ok(Json(d)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("dataset {} not found", dataset_id),
        )),
    }
}

/// Update a dataset by ID.
/// Rejects updates when dataset is soft-deleted.
pub async fn update_dataset(
    State(state): State<AppState>,
    Path(dataset_id): Path<Uuid>,
    Json(req): Json<UpdateDatasetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let updated = db::update_dataset(
        &state.pool,
        dataset_id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.credit.as_deref(),
        req.private,
        req.metadata.as_ref(),
    )
    .await
    .map_err(|e| {
        metrics::db_error("update_dataset");
        tracing::error!("failed to update dataset: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update dataset: {}", e),
        )
    })?;

    match updated {
        db::UpdateDatasetResult::Updated(d) => Ok(Json(d)),
        db::UpdateDatasetResult::NotFound => Err((
            StatusCode::NOT_FOUND,
            format!("dataset {} not found", dataset_id),
        )),
        db::UpdateDatasetResult::Deleted => Err((
            StatusCode::CONFLICT,
            format!("dataset {} is deleted", dataset_id),
        )),
    }
}

/// Soft-delete a dataset by ID.
pub async fn delete_dataset(
    State(state): State<AppState>,
    Path(dataset_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = db::delete_dataset(&state.pool, dataset_id)
        .await
        .map_err(|e| {
            metrics::db_error("delete_dataset");
            tracing::error!("failed to delete dataset: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete dataset: {}", e),
            )
        })?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("dataset {} not found", dataset_id),
        ))
    }
}

/// Add or upsert a dataset source for a dataset.
pub async fn create_dataset_source(
    State(state): State<AppState>,
    Path(dataset_id): Path<Uuid>,
    Json(req): Json<CreateDatasetSourceRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let source = db::upsert_dataset_source(
        &state.pool,
        dataset_id,
        &req.endpoint,
        &req.region,
        &req.bucket,
        req.requires_credentials,
    )
    .await
    .map_err(|e| {
        metrics::db_error("upsert_dataset_source");
        tracing::error!("failed to upsert dataset source: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to upsert dataset source: {}", e),
        )
    })?;

    Ok((StatusCode::CREATED, Json(source)))
}

/// List all dataset sources for a dataset.
pub async fn list_dataset_sources(
    State(state): State<AppState>,
    Path(dataset_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sources = db::list_dataset_sources(&state.pool, dataset_id)
        .await
        .map_err(|e| {
            metrics::db_error("list_dataset_sources");
            tracing::error!("failed to list dataset sources: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list dataset sources: {}", e),
            )
        })?;

    Ok(Json(sources))
}

/// Delete a dataset source for a dataset.
pub async fn delete_dataset_source(
    State(state): State<AppState>,
    Path((dataset_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = db::delete_dataset_source(&state.pool, dataset_id, source_id)
        .await
        .map_err(|e| {
            metrics::db_error("delete_dataset_source");
            tracing::error!("failed to delete dataset source: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete dataset source: {}", e),
            )
        })?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!(
                "dataset source {} for dataset {} not found",
                source_id, dataset_id
            ),
        ))
    }
}

// =============================================================================
// Annotation Set Handlers
// =============================================================================

/// List all annotation sets for a slide.
pub async fn list_annotation_sets(
    State(state): State<AppState>,
    Path(slide_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let response = annotation_db::list_annotation_sets(&state.pool, slide_id)
        .await
        .map_err(|e| {
            metrics::db_error("list_annotation_sets");
            tracing::error!("failed to list annotation sets: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list annotation sets: {}", e),
            )
        })?;

    metrics::annotation_sets_listed(response.items.len());
    Ok(Json(response))
}

/// Create a new annotation set for a slide.
pub async fn create_annotation_set(
    State(state): State<AppState>,
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
        req.created_by, // passed from request
        locked,
        &metadata,
    )
    .await
    .map_err(|e| {
        metrics::db_error("create_annotation_set");
        tracing::error!("failed to create annotation set: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create annotation set: {}", e),
        )
    })?;

    metrics::annotation_set_created();
    Ok((StatusCode::CREATED, Json(annotation_set)))
}

/// Get a single annotation set by ID.
pub async fn get_annotation_set(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation_set = annotation_db::get_annotation_set(&state.pool, id)
        .await
        .map_err(|e| {
            metrics::db_error("get_annotation_set");
            tracing::error!("failed to get annotation set: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to get annotation set: {}", e),
            )
        })?;

    match annotation_set {
        Some(s) => {
            metrics::annotation_set_retrieved();
            Ok(Json(s))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation set {} not found", id),
        )),
    }
}

/// Update an annotation set.
pub async fn update_annotation_set(
    State(state): State<AppState>,
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
        metrics::db_error("update_annotation_set");
        tracing::error!("failed to update annotation set: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update annotation set: {}", e),
        )
    })?;

    match annotation_set {
        Some(s) => {
            metrics::annotation_set_updated();
            Ok(Json(s))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation set {} not found", id),
        )),
    }
}

/// Delete an annotation set.
pub async fn delete_annotation_set(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = annotation_db::delete_annotation_set(&state.pool, id)
        .await
        .map_err(|e| {
            metrics::db_error("delete_annotation_set");
            tracing::error!("failed to delete annotation set: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete annotation set: {}", e),
            )
        })?;

    if deleted {
        metrics::annotation_set_deleted();
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
pub async fn list_annotations(
    State(state): State<AppState>,
    Path(annotation_set_id): Path<Uuid>,
    Query(query): Query<ListAnnotationsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let response = annotation_db::list_annotations(&state.pool, annotation_set_id, &query)
        .await
        .map_err(|e| {
            metrics::db_error("list_annotations");
            tracing::error!("failed to list annotations: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list annotations: {}", e),
            )
        })?;

    metrics::annotations_listed(response.items.len());
    Ok(Json(response))
}

/// Create a new annotation with geometry.
pub async fn create_annotation(
    State(state): State<AppState>,
    Path(annotation_set_id): Path<Uuid>,
    Json(req): Json<CreateAnnotationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let metadata = req.metadata.unwrap_or(serde_json::json!({}));
    let source = req.source.as_deref();
    let kind_str = req.kind.as_str();

    let result = match req.kind {
        AnnotationKind::Point => {
            let geom = parse_point_geometry(&req.geometry)?;
            annotation_db::create_point_annotation(
                &state.pool,
                annotation_set_id,
                &req.label_id,
                req.created_by, // passed from request
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
                req.created_by,
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
                req.created_by,
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
                req.created_by,
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
        metrics::db_error("create_annotation");
        tracing::error!("failed to create annotation: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create annotation: {}", e),
        )
    })?;

    metrics::annotation_created(kind_str);
    Ok((StatusCode::CREATED, Json(annotation)))
}

/// Get a single annotation by ID.
pub async fn get_annotation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let annotation = annotation_db::get_annotation(&state.pool, id)
        .await
        .map_err(|e| {
            metrics::db_error("get_annotation");
            tracing::error!("failed to get annotation: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to get annotation: {}", e),
            )
        })?;

    match annotation {
        Some(a) => {
            metrics::annotation_retrieved(a.kind.as_str());
            Ok(Json(a))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation {} not found", id),
        )),
    }
}

/// Update an annotation.
pub async fn update_annotation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
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
        metrics::db_error("update_annotation");
        tracing::error!("failed to update annotation: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update annotation: {}", e),
        )
    })?;

    match annotation {
        Some(a) => {
            metrics::annotation_updated(a.kind.as_str());
            Ok(Json(a))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("annotation {} not found", id),
        )),
    }
}

/// Delete an annotation.
pub async fn delete_annotation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = annotation_db::delete_annotation(&state.pool, id)
        .await
        .map_err(|e| {
            metrics::db_error("delete_annotation");
            tracing::error!("failed to delete annotation: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to delete annotation: {}", e),
            )
        })?;

    if deleted {
        metrics::annotation_deleted();
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
