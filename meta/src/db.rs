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
                url TEXT NOT NULL
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

    tracing::info!("database schema initialized");
    Ok(())
}

/// Insert a new slide into the database.
pub async fn insert_slide(pool: &Pool, width: i32, height: i32, url: &str) -> Result<Slide> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_one(
            r#"
            INSERT INTO slides (width, height, url)
            VALUES ($1, $2, $3)
            RETURNING id, width, height, url
            "#,
            &[&width, &height, &url],
        )
        .await
        .context("failed to insert slide")?;

    Ok(Slide {
        id: row.get("id"),
        width: row.get("width"),
        height: row.get("height"),
        url: row.get("url"),
    })
}

/// Get a slide by its ID.
pub async fn get_slide(pool: &Pool, id: Uuid) -> Result<Option<Slide>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r#"
            SELECT id, width, height, url
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
    }))
}

/// Update a slide by its ID. Only provided fields are updated.
pub async fn update_slide(
    pool: &Pool,
    id: Uuid,
    width: Option<i32>,
    height: Option<i32>,
    url: Option<&str>,
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

    if set_clauses.is_empty() {
        // Nothing to update, just return the existing slide
        return get_slide(pool, id).await;
    }

    let query = format!(
        "UPDATE slides SET {} WHERE id = ${} RETURNING id, width, height, url",
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
