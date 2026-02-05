use anyhow::{bail, Context, Result};
use async_channel::Receiver;
use histion_storage::client::StorageClient;
use tokio_util::sync::CancellationToken;

use crate::protocol::MessageBuilder;
use crate::viewport::RetrieveTileWork;

pub async fn worker_main(
    cancel: CancellationToken,
    mut storage: StorageClient,
    rx: Receiver<RetrieveTileWork>,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            work = rx.recv() => {
                let work = work.context("failed to receive work")?;
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
                        let payload = MessageBuilder::tile_data(work.slot, &work.meta, &data);
                        tokio::select! {
                            _ = cancel.cancelled() => return Ok(()),
                            _ = work.cancel.cancelled() => {}
                            _ = work.tx.send(payload) => {
                                tracing::info!(
                                    slide_id = %work.slide_id,
                                    x = work.meta.x,
                                    y = work.meta.y,
                                    level = work.meta.level,
                                    slot = work.slot,
                                    size = data.len(),
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
