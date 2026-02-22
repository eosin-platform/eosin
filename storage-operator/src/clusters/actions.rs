use eosin_types::{Cluster, ClusterPhase, ClusterStatus, ShardStatus};
use kube::Client;

use crate::util::{Error, patch::patch_status};

pub async fn patch_cluster_status(
    client: Client,
    instance: &Cluster,
    phase: ClusterPhase,
    config_epoch: u64,
    applied_shards: u32,
    message: Option<String>,
    shards: Vec<ShardStatus>,
) -> Result<(), Error> {
    patch_status::<ClusterStatus, Cluster>(client, instance, |status| {
        status.phase = phase;
        status.config_epoch = config_epoch;
        status.applied_shards = applied_shards;
        status.message = message;
        status.shards = shards;
    })
    .await
    .map_err(Error::from)?;
    Ok(())
}
