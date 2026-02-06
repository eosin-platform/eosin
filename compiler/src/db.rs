use anyhow::{Context, Result};
use deadpool_postgres::Pool;
use uuid::Uuid;

/// Minimum number of tiles at a mip level to enable checkpoint tracking.
/// Levels with fewer tiles will not use checkpointing.
pub const CHECKPOINT_MIN_TILES: usize = 128;

/// How often to update the checkpoint (every N tiles).
pub const CHECKPOINT_INTERVAL: usize = 1024;

/// Initialize the database schema for dispatch tracking.
pub async fn init_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r"
            CREATE TABLE IF NOT EXISTS compiler_dispatch (
                key TEXT PRIMARY KEY,
                discovered_at BIGINT NOT NULL,
                dispatched_at BIGINT
            )
            ",
            &[],
        )
        .await
        .context("failed to create compiler_dispatch table")?;

    // Create table for tile progress checkpointing (sparse range representation)
    client
        .execute(
            r"
            CREATE TABLE IF NOT EXISTS compiler_tile_progress (
                slide_id UUID NOT NULL,
                level INTEGER NOT NULL,
                completed_up_to INTEGER NOT NULL,
                total_tiles INTEGER NOT NULL,
                updated_at BIGINT NOT NULL,
                PRIMARY KEY (slide_id, level)
            )
            ",
            &[],
        )
        .await
        .context("failed to create compiler_tile_progress table")?;

    tracing::info!("database schema initialized");
    Ok(())
}

/// Result of trying to dispatch a key.
pub enum DispatchResult {
    /// The key was newly dispatched
    Dispatched,
    /// The key was already dispatched previously
    AlreadyDispatched,
    /// Publishing failed
    PublishFailed,
}

/// Try to dispatch a key with a callback for publishing.
/// This function uses a transaction with row locking to ensure exactly-once dispatch.
/// The publish callback is called while holding the row lock. If it succeeds, the row
/// is marked as dispatched and the transaction is committed.
pub async fn try_dispatch_with_publish<F, Fut>(
    pool: &Pool,
    key: &str,
    now_ms: i64,
    publish_fn: F,
) -> Result<DispatchResult>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut client = pool.get().await.context("failed to get db connection")?;

    // Start a transaction
    let tx = client
        .transaction()
        .await
        .context("failed to start transaction")?;

    // Upsert the row
    tx.execute(
        r"
        INSERT INTO compiler_dispatch (key, discovered_at, dispatched_at)
        VALUES ($1, $2, NULL)
        ON CONFLICT (key) DO NOTHING
        ",
        &[&key, &now_ms],
    )
    .await
    .context("failed to upsert dispatch row")?;

    // Lock the row and check if it's already dispatched
    let row = tx
        .query_one(
            r"
            SELECT dispatched_at FROM compiler_dispatch
            WHERE key = $1
            FOR UPDATE
            ",
            &[&key],
        )
        .await
        .context("failed to select dispatch row")?;

    let dispatched_at: Option<i64> = row.get(0);

    if dispatched_at.is_some() {
        // Already dispatched, rollback and return
        tx.rollback()
            .await
            .context("failed to rollback transaction")?;
        return Ok(DispatchResult::AlreadyDispatched);
    }

    // Call the publish function while holding the lock
    if let Err(e) = publish_fn().await {
        tracing::error!(error = ?e, "publish failed, rolling back");
        tx.rollback()
            .await
            .context("failed to rollback transaction")?;
        return Ok(DispatchResult::PublishFailed);
    }

    // Mark as dispatched
    tx.execute(
        r"
        UPDATE compiler_dispatch
        SET dispatched_at = $1
        WHERE key = $2
        ",
        &[&now_ms, &key],
    )
    .await
    .context("failed to mark key as dispatched")?;

    // Commit the transaction
    tx.commit().await.context("failed to commit transaction")?;

    Ok(DispatchResult::Dispatched)
}

/// Get the checkpoint (number of tiles completed) for a slide level.
/// Returns 0 if no checkpoint exists.
pub async fn get_tile_checkpoint(pool: &Pool, slide_id: Uuid, level: u32) -> Result<usize> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r"
            SELECT completed_up_to FROM compiler_tile_progress
            WHERE slide_id = $1 AND level = $2
            ",
            &[&slide_id, &(level as i32)],
        )
        .await
        .context("failed to query tile checkpoint")?;

    match row {
        Some(r) => {
            let completed: i32 = r.get(0);
            Ok(completed as usize)
        }
        None => Ok(0),
    }
}

/// Update the checkpoint for a slide level.
/// Uses upsert to handle both insert and update cases.
pub async fn update_tile_checkpoint(
    pool: &Pool,
    slide_id: Uuid,
    level: u32,
    completed_up_to: usize,
    total_tiles: usize,
) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    client
        .execute(
            r"
            INSERT INTO compiler_tile_progress (slide_id, level, completed_up_to, total_tiles, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (slide_id, level) DO UPDATE
            SET completed_up_to = EXCLUDED.completed_up_to,
                total_tiles = EXCLUDED.total_tiles,
                updated_at = EXCLUDED.updated_at
            ",
            &[
                &slide_id,
                &(level as i32),
                &(completed_up_to as i32),
                &(total_tiles as i32),
                &now_ms,
            ],
        )
        .await
        .context("failed to update tile checkpoint")?;

    Ok(())
}

/// Mark a slide level as complete by setting completed_up_to = total_tiles.
/// We keep the row (rather than deleting it) so that on restart we can
/// distinguish "fully completed" from "never started".
pub async fn mark_level_complete(
    pool: &Pool,
    slide_id: Uuid,
    level: u32,
    total_tiles: usize,
) -> Result<()> {
    // Upsert so this works even if no checkpoint row existed yet (e.g. small levels
    // that skipped checkpointing).
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r"
            INSERT INTO compiler_tile_progress (slide_id, level, completed_up_to, total_tiles, updated_at)
            VALUES ($1, $2, $3, $3, $4)
            ON CONFLICT (slide_id, level) DO UPDATE
            SET completed_up_to = EXCLUDED.completed_up_to,
                total_tiles = EXCLUDED.total_tiles,
                updated_at = EXCLUDED.updated_at
            ",
            &[
                &slide_id,
                &(level as i32),
                &(total_tiles as i32),
                &now_ms,
            ],
        )
        .await
        .context("failed to mark level complete")?;

    Ok(())
}

/// Clear all checkpoints for a slide (e.g., when fully complete).
pub async fn clear_all_tile_checkpoints(pool: &Pool, slide_id: Uuid) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r"
            DELETE FROM compiler_tile_progress
            WHERE slide_id = $1
            ",
            &[&slide_id],
        )
        .await
        .context("failed to clear all tile checkpoints")?;

    Ok(())
}
