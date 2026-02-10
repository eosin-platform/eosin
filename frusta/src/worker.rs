use anyhow::{bail, Result};
use eosin_storage::client::StorageClient;
use tokio_util::sync::CancellationToken;

use crate::priority_queue::PriorityWorkQueue;
use crate::protocol::MessageBuilder;
use crate::viewport::{compute_min_level, is_tile_in_viewport, RetrieveTileWork};

/// Check whether this tile was already delivered to the client.
/// Returns `true` if we should skip sending (another worker beat us to it).
fn is_tile_already_delivered(work: &RetrieveTileWork) -> bool {
    let key = work.meta.index_unchecked();
    let sent = work.sent.read();
    sent.get(&key).map(|info| info.delivered).unwrap_or(false)
}

/// Mark a tile as delivered in the sent map.
fn mark_tile_delivered(work: &RetrieveTileWork) {
    let key = work.meta.index_unchecked();
    let mut sent = work.sent.write();
    if let Some(info) = sent.get_mut(&key) {
        info.delivered = true;
    }
}

/// Check whether the tile described by `work` is still visible in the latest
/// viewport snapshot.  Returns `false` (skip) when:
/// - no viewport has been set yet,
/// - the tile falls outside the visible region, or
/// - the tile's mip level is finer than the minimum useful level for the
///   current zoom.
async fn is_tile_still_visible(work: &RetrieveTileWork) -> bool {
    let guard = work.viewport.read().await;
    let Some(viewport) = *guard else {
        return false;
    };
    drop(guard);

    if !is_tile_in_viewport(
        &viewport,
        &work.image,
        work.meta.x,
        work.meta.y,
        work.meta.level,
    ) {
        return false;
    }

    let min_level = compute_min_level(&viewport, work.dpi, work.image.levels);
    work.meta.level >= min_level
}

pub async fn worker_main(
    cancel: CancellationToken,
    mut storage: StorageClient,
    work_queue: PriorityWorkQueue,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            work = work_queue.pop() => {
                let Some(work) = work else {
                    // Queue was closed
                    return Ok(());
                };
                // Fast-path: skip stale work items immediately so the queue
                // drains quickly after a viewport change, making room for
                // fresh coarse tiles that enable progressive loading.
                if work.cancel.is_cancelled() {
                    continue;
                }
                // Check against the *latest* viewport before starting the
                // (potentially expensive) storage fetch.
                if !is_tile_still_visible(&work).await {
                    tracing::debug!(
                        slide_id = %work.slide_id,
                        x = work.meta.x,
                        y = work.meta.y,
                        level = work.meta.level,
                        "tile no longer visible, skipping fetch"
                    );
                    continue;
                }
                tokio::select! {
                    _ = cancel.cancelled() => bail!("Context cancelled"),
                    _ = work.cancel.cancelled() => continue,
                    data = storage.get_tile(
                        work.slide_id,
                        work.meta.x,
                        work.meta.y,
                        work.meta.level,
                    ) => {
                        // Handle missing tiles gracefully - they may not be processed yet
                        // The client can request again later when the tile becomes available
                        let data = match data {
                            Ok(data) => data,
                            Err(e) => {
                                tracing::debug!(
                                    slide_id = %work.slide_id,
                                    x = work.meta.x,
                                    y = work.meta.y,
                                    level = work.meta.level,
                                    error = %e,
                                    "tile not available, skipping"
                                );
                                continue;
                            }
                        };
                        // Re-check visibility against the *latest* viewport
                        // after the fetch completes – the user may have panned
                        // or zoomed while we were waiting on storage.
                        if !is_tile_still_visible(&work).await {
                            tracing::debug!(
                                slide_id = %work.slide_id,
                                x = work.meta.x,
                                y = work.meta.y,
                                level = work.meta.level,
                                "tile no longer visible after fetch, discarding"
                            );
                            continue;
                        }
                        // Final check: was this tile already delivered by another worker?
                        // This prevents duplicate sends when multiple workers race to
                        // deliver the same tile.
                        if is_tile_already_delivered(&work) {
                            tracing::debug!(
                                slide_id = %work.slide_id,
                                x = work.meta.x,
                                y = work.meta.y,
                                level = work.meta.level,
                                "tile already delivered by another worker, skipping"
                            );
                            continue;
                        }
                        let payload = MessageBuilder::tile_data(work.slot, &work.meta, &data);
                        tokio::select! {
                            _ = cancel.cancelled() => return Ok(()),
                            _ = work.cancel.cancelled() => {}
                            _ = work.tx.send(payload) => {
                                // Mark the tile as delivered so other workers don't
                                // send duplicates
                                mark_tile_delivered(&work);
                                tracing::info!(
                                    slide_id = %work.slide_id,
                                    x = work.meta.x,
                                    y = work.meta.y,
                                    level = work.meta.level,
                                    slot = work.slot,
                                    size = data.len(),
                                    client_ip = work.client_ip.as_deref().unwrap_or("-"),
                                    "sent tile"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
