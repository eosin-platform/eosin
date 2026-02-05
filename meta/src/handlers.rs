use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    db,
    models::{CreateSlideRequest, ListSlidesRequest, UpdateSlideRequest},
    server::AppState,
};

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
        req.width,
        req.height,
        &req.url,
        req.full_size,
    )
    .await
    .map_err(|e| {
        tracing::error!("failed to create slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create slide: {}", e),
        )
    })?;

    Ok((StatusCode::CREATED, Json(slide)))
}

/// Get a slide by ID
pub async fn get_slide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let slide = db::get_slide(&state.pool, id).await.map_err(|e| {
        tracing::error!("failed to get slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to get slide: {}", e),
        )
    })?;

    match slide {
        Some(s) => Ok(Json(s)),
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
        req.width,
        req.height,
        req.url.as_deref(),
        req.full_size,
    )
    .await
    .map_err(|e| {
        tracing::error!("failed to update slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update slide: {}", e),
        )
    })?;

    match slide {
        Some(s) => Ok(Json(s)),
        None => Err((StatusCode::NOT_FOUND, format!("slide {} not found", id))),
    }
}

/// Delete a slide by ID
pub async fn delete_slide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = db::delete_slide(&state.pool, id).await.map_err(|e| {
        tracing::error!("failed to delete slide: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to delete slide: {}", e),
        )
    })?;

    if deleted {
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
    // Validate pagination parameters
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

    // Cap limit to prevent excessive queries
    let limit = req.limit.min(1000);

    let response = db::list_slides(&state.pool, req.offset, limit)
        .await
        .map_err(|e| {
            tracing::error!("failed to list slides: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to list slides: {}", e),
            )
        })?;

    Ok(Json(response))
}
