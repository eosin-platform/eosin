use anyhow::{Context, Result};
use deadpool_postgres::Pool;

/// Initialize the database schema for dispatch tracking.
pub async fn init_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS compiler_dispatch (
                key TEXT PRIMARY KEY,
                discovered_at BIGINT NOT NULL,
                dispatched_at BIGINT
            )
            "#,
            &[],
        )
        .await
        .context("failed to create compiler_dispatch table")?;

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
        r#"
        INSERT INTO compiler_dispatch (key, discovered_at, dispatched_at)
        VALUES ($1, $2, NULL)
        ON CONFLICT (key) DO NOTHING
        "#,
        &[&key, &now_ms],
    )
    .await
    .context("failed to upsert dispatch row")?;

    // Lock the row and check if it's already dispatched
    let row = tx
        .query_one(
            r#"
            SELECT dispatched_at FROM compiler_dispatch
            WHERE key = $1
            FOR UPDATE
            "#,
            &[&key],
        )
        .await
        .context("failed to select dispatch row")?;

    let dispatched_at: Option<i64> = row.get(0);

    if dispatched_at.is_some() {
        // Already dispatched, rollback and return
        tx.rollback().await.context("failed to rollback transaction")?;
        return Ok(DispatchResult::AlreadyDispatched);
    }

    // Call the publish function while holding the lock
    if let Err(e) = publish_fn().await {
        tracing::error!(error = ?e, "publish failed, rolling back");
        tx.rollback().await.context("failed to rollback transaction")?;
        return Ok(DispatchResult::PublishFailed);
    }

    // Mark as dispatched
    tx.execute(
        r#"
        UPDATE compiler_dispatch
        SET dispatched_at = $1
        WHERE key = $2
        "#,
        &[&now_ms, &key],
    )
    .await
    .context("failed to mark key as dispatched")?;

    // Commit the transaction
    tx.commit().await.context("failed to commit transaction")?;

    Ok(DispatchResult::Dispatched)
}
