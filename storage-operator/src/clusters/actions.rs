use crate::util::{self, Error, patch::*};
use eosin_common::annotations;
use eosin_types::*;

use kube::{
    Api, Client,
    api::{ObjectMeta, Resource},
};

fn instance_name(instance: &Cluster) -> Result<&str, Error> {
    instance
        .meta()
        .name
        .as_deref()
        .ok_or_else(|| Error::UserInput("Cluster is missing metadata.name".to_string()))
}

fn instance_namespace(instance: &Cluster) -> Result<&str, Error> {
    instance
        .meta()
        .namespace
        .as_deref()
        .ok_or_else(|| Error::UserInput("Cluster is missing metadata.namespace".to_string()))
}

/// Updates the `Cluster`'s phase to Active.
pub async fn active(client: Client, instance: &Cluster, peggy_pod_name: &str) -> Result<(), Error> {
    Ok(())
}

pub async fn pending(client: Client, instance: &Cluster, reason: String) -> Result<(), Error> {
    patch_status(client, instance, |status| {
        status.phase = ClusterPhase::Pending;
        status.message = Some(reason);
    })
    .await?;
    Ok(())
}

pub async fn error(client: Client, instance: &Cluster, message: String) -> Result<(), Error> {
    patch_status(client.clone(), instance, |status| {
        status.phase = ClusterPhase::Error;
        status.message = Some(message);
    })
    .await?;
    Ok(())
}
