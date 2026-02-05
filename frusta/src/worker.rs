use anyhow::{Context, Result, bail};
use async_channel::Receiver;
use histion_storage::client::StorageClient;
use tokio_util::sync::CancellationToken;

use crate::viewport::RetrieveTileWork;

pub async fn worker_main(
    cancel: CancellationToken,
    mut storage: StorageClient,
    rx: Receiver<RetrieveTileWork>,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => bail!("Context cancelled"),
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
                        let data = data.context("failed to get tile from storage")?;
                        let payload = {
                            let mut payload = Vec::with_capacity(data.len() + 13);
                            payload.push(work.slot);
                            payload.extend_from_slice(&work.meta.x.to_le_bytes());
                            payload.extend_from_slice(&work.meta.y.to_le_bytes());
                            payload.extend_from_slice(&work.meta.level.to_le_bytes());
                            payload.extend_from_slice(&data);
                            payload.into()
                        };
                        tokio::select! {
                            _ = cancel.cancelled() => bail!("Context cancelled"),
                            _ = work.cancel.cancelled() => {}
                            _ = work.tx.send(payload) => {}
                        }
                    }
                }
            }
        }
    }
}
