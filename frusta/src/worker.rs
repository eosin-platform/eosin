use anyhow::{Context, Result, bail};
use async_channel::Receiver;
use histion_storage::client::StorageClient;
use tokio_util::sync::CancellationToken;

use crate::viewport::{RetrieveTileWork, Tile};

pub async fn worker_main(
    cancel: CancellationToken,
    storage: StorageClient,
    rx: Receiver<RetrieveTileWork>,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                bail!("Context cancelled");
            }
            work = rx.recv() => {
                let work = work.context("failed to receive work")?;
                tokio::select! {
                    _ = cancel.cancelled() => {
                        bail!("Context cancelled");
                    }
                    _ = work.cancel.cancelled() => {
                        continue;
                    }
                    data = storage.get_tile(
                        work.id,
                        work.meta.x,
                        work.meta.y,
                        work.meta.level,
                    ) => {
                        let data = data.context("failed to get tile from storage")?;
                        let tile = Tile {
                            meta: work.meta,
                            data,
                        };
                        work.tx.send(tile).await.context("failed to send tile to viewport")?;
                    }
                }
            }
        }
    }
}
