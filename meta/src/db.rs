use anyhow::{Context, Result};
use deadpool_postgres::Pool;
use uuid::Uuid;

use crate::models::{ListSlidesResponse, Slide, SlideListItem};

/// Initialize the database schema, creating tables if they don't exist.
pub async fn init_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS slides (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                width INT NOT NULL,
                height INT NOT NULL,
                url TEXT NOT NULL,
                filename TEXT NOT NULL DEFAULT '',
                full_size BIGINT NOT NULL DEFAULT 0,
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
    width: i32,
    height: i32,
    url: &str,
    filename: &str,
    full_size: i64,
) -> Result<Slide> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_one(
            r#"
            INSERT INTO slides (id, width, height, url, filename, full_size)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE
            SET id = slides.id
            RETURNING id, width, height, url, filename, full_size, progress_steps, progress_total;
            "#,
            &[&id, &width, &height, &url, &filename, &full_size],
        )
        .await
        .context("failed to insert slide")?;

    Ok(Slide {
        id: row.get("id"),
        width: row.get("width"),
        height: row.get("height"),
        url: row.get("url"),
        filename: row.get("filename"),
        full_size: row.get("full_size"),
        progress_steps: row.get("progress_steps"),
        progress_total: row.get("progress_total"),
    })
}

/// Get a slide by its ID.
pub async fn get_slide(pool: &Pool, id: Uuid) -> Result<Option<Slide>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r#"
            SELECT id, width, height, url, filename, full_size, progress_steps, progress_total
            FROM slides
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query slide")?;

    Ok(row.map(|r| Slide {
        id: r.get("id"),
        width: r.get("width"),
        height: r.get("height"),
        url: r.get("url"),
        filename: r.get("filename"),
        full_size: r.get("full_size"),
        progress_steps: r.get("progress_steps"),
        progress_total: r.get("progress_total"),
    }))
}

/// Update a slide by its ID. Only provided fields are updated.
pub async fn update_slide(
    pool: &Pool,
    id: Uuid,
    width: Option<i32>,
    height: Option<i32>,
    url: Option<&str>,
    filename: Option<&str>,
    full_size: Option<i64>,
) -> Result<Option<Slide>> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Build dynamic update query
    let mut set_clauses = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    let mut param_idx = 1;

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

    if set_clauses.is_empty() {
        // Nothing to update, just return the existing slide
        return get_slide(pool, id).await;
    }

    let query = format!(
        "UPDATE slides SET {} WHERE id = ${} RETURNING id, width, height, url, filename, full_size, progress_steps, progress_total",
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
        width: r.get("width"),
        height: r.get("height"),
        url: r.get("url"),
        filename: r.get("filename"),
        full_size: r.get("full_size"),
        progress_steps: r.get("progress_steps"),
        progress_total: r.get("progress_total"),
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
                width, 
                height,
                filename,
                full_size,
                progress_steps,
                progress_total,
                COUNT(*) OVER() AS full_count
            FROM slides
            ORDER BY id
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
            width: r.get("width"),
            height: r.get("height"),
            filename: r.get("filename"),
            full_size: r.get("full_size"),
            progress_steps: r.get("progress_steps"),
            progress_total: r.get("progress_total"),
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
