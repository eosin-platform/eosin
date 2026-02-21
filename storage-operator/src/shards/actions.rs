use crate::util::{self, Error, patch::*};
use eosin_common::annotations;
use eosin_types::*;
use k8s_openapi::api::core::v1::{
    Container, EnvVar, EnvVarSource, ObjectFieldSelector, Pod, PodSpec, SecretKeySelector, Volume,
    VolumeMount,
};
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

pub async fn delete_pod(client: Client, instance: &Cluster, reason: String) -> Result<(), Error> {
    let pod_name = instance_name(instance)?;
    println!(
        "Deleting Pod '{}' for Cluster '{}' • reason: {}",
        pod_name, pod_name, reason
    );
    patch_status(client.clone(), instance, |status| {
        status.phase = ClusterPhase::Pending;
        status.message = Some(delete_message(&reason));
    })
    .await?;
    let pods: Api<Pod> = Api::namespaced(client.clone(), instance_namespace(instance)?);
    match pods.delete(pod_name, &Default::default()).await {
        Ok(_) => {}
        Err(kube::Error::Api(ae)) if ae.code == 404 => {}
        Err(e) => return Err(Error::from(e)),
    }
    Ok(())
}

fn delete_message(reason: &str) -> String {
    format!("The peggy Pod is being deleted. Reason: {}", reason)
}

pub async fn pending(client: Client, instance: &Cluster, reason: String) -> Result<(), Error> {
    patch_status(client, instance, |status| {
        status.phase = ClusterPhase::Pending;
        status.message = Some(reason);
    })
    .await?;
    Ok(())
}

pub fn pod_resource(instance: &Cluster) -> Result<Pod, Error> {
    // For simplicity, we create a pod spec with a single container
    // that runs ffmpeg to stream from the source to the destination
    const HLS_DIR: &str = "/hls";
    let image = String::from("thavlik/eosin-peggy:latest");
    let name = instance_name(instance)?.to_string();
    let namespace = instance_namespace(instance)?.to_string();
    Ok(Pod {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(namespace),
            owner_references: Some(vec![instance.controller_owner_ref(&()).unwrap()]),
            annotations: Some({
                let mut annotations = std::collections::BTreeMap::new();
                annotations.insert(
                    annotations::SPEC_HASH.to_string(),
                    util::hash_spec(&instance.spec),
                );
                annotations.insert(
                    annotations::CREATED_BY.to_string(),
                    "eosin-operator".to_string(),
                );
                annotations
            }),
            ..Default::default()
        },
        spec: Some(PodSpec {
            volumes: Some(vec![Volume {
                name: "hls-storage".to_string(),
                empty_dir: Some(Default::default()),
                ..Default::default()
            }]),
            containers: vec![],
            restart_policy: Some("Never".to_string()),
            // resources: Some(ResourceRequirements {
            //     requests: Some({
            //         let mut m = std::collections::BTreeMap::new();
            //         m.insert("cpu".to_string(), Quantity("500m".to_string()));
            //         m.insert("memory".to_string(), Quantity("64Mi".to_string()));
            //         m
            //     }),
            //     limits: Some({
            //         let mut m = std::collections::BTreeMap::new();
            //         //m.insert("cpu".to_string(), Quantity("2000m".to_string()));
            //         m.insert("memory".to_string(), Quantity("512Mi".to_string()));
            //         m
            //     }),
            //     ..Default::default()
            // }),
            ..Default::default()
        }),
        status: None,
    })
}

pub async fn create_pod(client: Client, instance: &Cluster) -> Result<(), Error> {
    let pod = pod_resource(instance)?;
    let pod_name = instance_name(instance)?;
    patch_status(client.clone(), instance, |status| {
        status.phase = ClusterPhase::Reconciling;
        status.message = Some(format!("Creating peggy Pod '{}'", pod_name));
    })
    .await?;
    let pods: Api<Pod> = Api::namespaced(client.clone(), instance_namespace(instance)?);
    match pods.create(&Default::default(), &pod).await {
        Ok(_) => Ok(()),
        Err(e) => match e {
            kube::Error::Api(ae) if ae.code == 409 => Ok(()),
            _ => return Err(Error::from(e)),
        },
    }
}

pub async fn error(client: Client, instance: &Cluster, message: String) -> Result<(), Error> {
    patch_status(client.clone(), instance, |status| {
        status.phase = ClusterPhase::Error;
        status.message = Some(message);
    })
    .await?;
    Ok(())
}
