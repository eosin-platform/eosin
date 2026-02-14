use anyhow::{Context, Result};
use deadpool_postgres::Pool;
use uuid::Uuid;

use crate::models::{
    Dataset, DatasetListItem, ListDatasetsResponse, ListSlidesResponse, Slide, SlideListItem,
};

pub enum UpdateDatasetResult {
    Updated(Dataset),
    NotFound,
    Deleted,
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Initialize the database schema, creating tables if they don't exist.
pub async fn init_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS datasets (
                id UUID PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                deleted_at BIGINT,
                metadata JSONB
            )
            "#,
            &[],
        )
        .await
        .context("failed to create datasets table")?;

    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS slides (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                dataset UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
                width INT NOT NULL,
                height INT NOT NULL,
                url TEXT NOT NULL,
                filename TEXT NOT NULL DEFAULT '',
                full_size BIGINT NOT NULL DEFAULT 0,
                metadata JSONB,
                progress_steps INT NOT NULL DEFAULT 0,
                progress_total INT NOT NULL DEFAULT 0
            )
            "#,
            &[],
        )
        .await
        .context("failed to create slides table")?;

    // Create index on url for faster lookups
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_slides_url ON slides (url)
            "#,
            &[],
        )
        .await
        .context("failed to create url index")?;

    // Add filename column to existing tables (migration for existing databases)
    client
        .execute(
            r#"
            ALTER TABLE slides ADD COLUMN IF NOT EXISTS filename TEXT NOT NULL DEFAULT ''
            "#,
            &[],
        )
        .await
        .context("failed to add filename column")?;

    tracing::info!("database schema initialized");
    Ok(())
}

/// Insert a new slide into the database.
pub async fn insert_slide(
    pool: &Pool,
    id: Uuid,
    dataset: Uuid,
    width: i32,
    height: i32,
    url: &str,
    filename: &str,
    full_size: i64,
    metadata: Option<&serde_json::Value>,
) -> Result<Slide> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_one(
            r#"
            INSERT INTO slides (id, dataset, width, height, url, filename, full_size, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (id) DO UPDATE
            SET id = slides.id
            RETURNING id, dataset, width, height, url, filename, full_size, progress_steps, progress_total, metadata;
            "#,
            &[
                &id,
                &dataset,
                &width,
                &height,
                &url,
                &filename,
                &full_size,
                &metadata,
            ],
        )
        .await
        .context("failed to insert slide")?;

    Ok(Slide {
        id: row.get("id"),
        dataset: row.get("dataset"),
        width: row.get("width"),
        height: row.get("height"),
        url: row.get("url"),
        filename: row.get("filename"),
        full_size: row.get("full_size"),
        progress_steps: row.get("progress_steps"),
        progress_total: row.get("progress_total"),
        metadata: row.get("metadata"),
    })
}

/// Get a slide by its ID.
pub async fn get_slide(pool: &Pool, id: Uuid) -> Result<Option<Slide>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r#"
            SELECT id, dataset, width, height, url, filename, full_size, progress_steps, progress_total, metadata
            FROM slides
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query slide")?;

    Ok(row.map(|r| Slide {
        id: r.get("id"),
        dataset: r.get("dataset"),
        width: r.get("width"),
        height: r.get("height"),
        url: r.get("url"),
        filename: r.get("filename"),
        full_size: r.get("full_size"),
        progress_steps: r.get("progress_steps"),
        progress_total: r.get("progress_total"),
        metadata: r.get("metadata"),
    }))
}

/// Update a slide by its ID. Only provided fields are updated.
pub async fn update_slide(
    pool: &Pool,
    id: Uuid,
    dataset: Option<Uuid>,
    width: Option<i32>,
    height: Option<i32>,
    url: Option<&str>,
    filename: Option<&str>,
    full_size: Option<i64>,
    metadata: Option<&serde_json::Value>,
) -> Result<Option<Slide>> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Build dynamic update query
    let mut set_clauses = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref d) = dataset {
        set_clauses.push(format!("dataset = ${}", param_idx));
        params.push(d);
        param_idx += 1;
    }
    // We need to store the values to keep them alive for the query
    if let Some(ref w) = width {
        set_clauses.push(format!("width = ${}", param_idx));
        params.push(w);
        param_idx += 1;
    }
    if let Some(ref h) = height {
        set_clauses.push(format!("height = ${}", param_idx));
        params.push(h);
        param_idx += 1;
    }
    if let Some(ref u) = url {
        set_clauses.push(format!("url = ${}", param_idx));
        params.push(u);
        param_idx += 1;
    }
    if let Some(ref f) = filename {
        set_clauses.push(format!("filename = ${}", param_idx));
        params.push(f);
        param_idx += 1;
    }
    if let Some(ref fs) = full_size {
        set_clauses.push(format!("full_size = ${}", param_idx));
        params.push(fs);
        param_idx += 1;
    }
    if let Some(ref m) = metadata {
        set_clauses.push(format!("metadata = ${}", param_idx));
        params.push(m);
        param_idx += 1;
    }

    if set_clauses.is_empty() {
        // Nothing to update, just return the existing slide
        return get_slide(pool, id).await;
    }

    let query = format!(
        "UPDATE slides SET {} WHERE id = ${} RETURNING id, dataset, width, height, url, filename, full_size, progress_steps, progress_total, metadata",
        set_clauses.join(", "),
        param_idx
    );
    params.push(&id);

    let row = client
        .query_opt(&query, &params)
        .await
        .context("failed to update slide")?;

    Ok(row.map(|r| Slide {
        id: r.get("id"),
        dataset: r.get("dataset"),
        width: r.get("width"),
        height: r.get("height"),
        url: r.get("url"),
        filename: r.get("filename"),
        full_size: r.get("full_size"),
        progress_steps: r.get("progress_steps"),
        progress_total: r.get("progress_total"),
        metadata: r.get("metadata"),
    }))
}

/// Delete a slide by its ID.
pub async fn delete_slide(pool: &Pool, id: Uuid) -> Result<bool> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows_affected = client
        .execute(
            r#"
            DELETE FROM slides
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to delete slide")?;

    Ok(rows_affected > 0)
}

/// List slides with pagination.
/// Uses a window function for efficient full count retrieval.
pub async fn list_slides(pool: &Pool, offset: i64, limit: i64) -> Result<ListSlidesResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Use window function to get full count in a single query
    // This is more efficient than running two separate queries
    let rows = client
        .query(
            r#"
            SELECT 
                id, 
                dataset,
                width, 
                height,
                filename,
                full_size,
                progress_steps,
                progress_total,
                metadata,
                COUNT(*) OVER() AS full_count
            FROM slides
            ORDER BY filename ASC, id ASC
            LIMIT $1
            OFFSET $2
            "#,
            &[&limit, &offset],
        )
        .await
        .context("failed to list slides")?;

    // Extract full_count from first row, or 0 if no rows
    let full_count: i64 = rows.first().map(|r| r.get("full_count")).unwrap_or(0);

    let items: Vec<SlideListItem> = rows
        .iter()
        .map(|r| SlideListItem {
            id: r.get("id"),
            dataset: r.get("dataset"),
            width: r.get("width"),
            height: r.get("height"),
            filename: r.get("filename"),
            full_size: r.get("full_size"),
            progress_steps: r.get("progress_steps"),
            progress_total: r.get("progress_total"),
            metadata: r.get("metadata"),
        })
        .collect();

    // Determine if results are truncated (there are more items beyond this page)
    let truncated = offset + (items.len() as i64) < full_count;

    Ok(ListSlidesResponse {
        offset,
        limit,
        full_count,
        truncated,
        items,
    })
}

/// Update progress for a slide by its ID.
/// Returns true if the slide was found and updated, false if not found.
pub async fn update_slide_progress(
    pool: &Pool,
    id: Uuid,
    progress_steps: i32,
    progress_total: i32,
) -> Result<bool> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows_affected = client
        .execute(
            r#"
            UPDATE slides
            SET progress_steps = $2, progress_total = $3
            WHERE id = $1
            "#,
            &[&id, &progress_steps, &progress_total],
        )
        .await
        .context("failed to update slide progress")?;

    Ok(rows_affected > 0)
}

/// List datasets with pagination.
/// Filters out soft-deleted rows.
pub async fn list_datasets(pool: &Pool, offset: i64, limit: i64) -> Result<ListDatasetsResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows = client
        .query(
            r#"
            SELECT
                id,
                name,
                description,
                created_at,
                updated_at,
                metadata,
                COUNT(*) OVER() AS full_count
            FROM datasets
            WHERE deleted_at IS NULL
            ORDER BY name ASC, id ASC
            LIMIT $1
            OFFSET $2
            "#,
            &[&limit, &offset],
        )
        .await
        .context("failed to list datasets")?;

    let full_count: i64 = rows.first().map(|r| r.get("full_count")).unwrap_or(0);

    let items: Vec<DatasetListItem> = rows
        .iter()
        .map(|r| DatasetListItem {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            metadata: r.get("metadata"),
        })
        .collect();

    let truncated = offset + (items.len() as i64) < full_count;

    Ok(ListDatasetsResponse {
        offset,
        limit,
        full_count,
        truncated,
        items,
    })
}

/// Get a dataset by ID if it is not soft-deleted.
pub async fn get_dataset(pool: &Pool, id: Uuid) -> Result<Option<Dataset>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r#"
            SELECT id, name, description, created_at, updated_at, deleted_at, metadata
            FROM datasets
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            &[&id],
        )
        .await
        .context("failed to query dataset")?;

    Ok(row.map(|r| Dataset {
        id: r.get("id"),
        name: r.get("name"),
        description: r.get("description"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        deleted_at: r.get("deleted_at"),
        metadata: r.get("metadata"),
    }))
}

/// Update a dataset by ID.
/// Rejects updates when the dataset is soft-deleted.
pub async fn update_dataset(
    pool: &Pool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    metadata: Option<&serde_json::Value>,
) -> Result<UpdateDatasetResult> {
    let client = pool.get().await.context("failed to get db connection")?;

    let state = client
        .query_opt(
            r#"
            SELECT deleted_at
            FROM datasets
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query dataset state")?;

    let Some(state_row) = state else {
        return Ok(UpdateDatasetResult::NotFound);
    };

    let deleted_at: Option<i64> = state_row.get("deleted_at");
    if deleted_at.is_some() {
        return Ok(UpdateDatasetResult::Deleted);
    }

    let mut set_clauses = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref n) = name {
        set_clauses.push(format!("name = ${}", param_idx));
        params.push(n);
        param_idx += 1;
    }

    if let Some(ref d) = description {
        set_clauses.push(format!("description = ${}", param_idx));
        params.push(d);
        param_idx += 1;
    }

    if let Some(ref m) = metadata {
        set_clauses.push(format!("metadata = ${}", param_idx));
        params.push(m);
        param_idx += 1;
    }

    let updated_at = now_ms();
    set_clauses.push(format!("updated_at = ${}", param_idx));
    params.push(&updated_at);
    param_idx += 1;

    let query = format!(
        "UPDATE datasets SET {} WHERE id = ${} RETURNING id, name, description, created_at, updated_at, deleted_at, metadata",
        set_clauses.join(", "),
        param_idx
    );
    params.push(&id);

    let row = client
        .query_one(&query, &params)
        .await
        .context("failed to update dataset")?;

    Ok(UpdateDatasetResult::Updated(Dataset {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        metadata: row.get("metadata"),
    }))
}

/// Soft-delete a dataset by setting deleted_at.
/// Returns true if a row was updated.
pub async fn delete_dataset(pool: &Pool, id: Uuid) -> Result<bool> {
    let client = pool.get().await.context("failed to get db connection")?;

    let deleted_at = now_ms();
    let rows_affected = client
        .execute(
            r#"
            UPDATE datasets
            SET deleted_at = $2, updated_at = $2
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            &[&id, &deleted_at],
        )
        .await
        .context("failed to soft-delete dataset")?;

    Ok(rows_affected > 0)
}
