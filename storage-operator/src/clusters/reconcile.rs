use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use eosin_types::{Cluster, ReplicaRole, ReplicaSummary, ShardStatus};
use futures::stream::StreamExt;
use k8s_openapi::api::core::v1::{Container, ContainerPort, EnvVar, Pod, PodSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PostParams};
use kube::client::Client;
use kube::runtime::controller::Action;
use kube::runtime::Controller;
use kube::{Api, Resource, ResourceExt};

use crate::clusters::planner::{
    DEFAULT_NUM_SLOTS, ReplicaHealth, build_promotion_decision, compute_slot_to_shard,
    determine_cluster_phase, next_config_epoch, now_unix_ms,
};
use crate::proto::cluster::{
    BecomeMasterRequest, BecomeReplicaRequest, ClusterRoutingConfig, GetShardStatusRequest, Role,
    UpdateRoutingConfigRequest, control_service_client::ControlServiceClient,
};
use crate::util::{Error, PROBE_INTERVAL};

use super::actions;

pub async fn run(client: Client) -> Result<(), Error> {
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".to_string());
    let clusters: Api<Cluster> = Api::namespaced(client.clone(), &namespace);
    let context = Arc::new(ContextData { client });

    Controller::new(clusters, Default::default())
        .run(reconcile, on_error, context)
        .for_each(|_| async {})
        .await;
    Ok(())
}

struct ContextData {
    client: Client,
}

#[derive(Clone, Default)]
struct ReplicaEndpoint {
    control_addr: Option<String>,
    cluster_addr: Option<String>,
}

async fn reconcile(instance: Arc<Cluster>, context: Arc<ContextData>) -> Result<Action, Error> {
    let client = context.client.clone();
    let namespace = instance
        .namespace()
        .ok_or_else(|| Error::UserInput("Cluster must be namespaced".to_string()))?;
    let name = instance.name_any();
    let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);

    reconcile_pod_topology(&pods, &instance).await?;

    let shard_count = instance.spec.topology.shards;
    let replicas_per_shard = 1 + instance.spec.topology.read_replicas;
    let existing_status = instance.status.clone().unwrap_or_default();
    let previous_shards = existing_status.applied_shards;
    let target_config_epoch = next_config_epoch(existing_status.config_epoch, previous_shards, shard_count);
    let heartbeat_timeout = Duration::from_secs(instance.spec.failover.heartbeat_timeout_seconds);

    let mut new_shards = Vec::new();
    let mut all_shards_healthy = true;
    let mut all_control_targets = Vec::new();
    let mut shard_master_cluster_addr: HashMap<u32, String> = HashMap::new();

    for shard_id in 0..shard_count {
        let mut replica_summaries = Vec::new();
        let mut endpoints: HashMap<String, ReplicaEndpoint> = HashMap::new();
        let mut health_replicas = Vec::new();

        for replica_id in 0..replicas_per_shard {
            let replica_name = pod_name(&name, shard_id, replica_id);
            let pod = pods.get_opt(&replica_name).await?;
            let ready = pod.as_ref().is_some_and(pod_ready);

            let endpoint = pod
                .as_ref()
                .and_then(|p| p.status.as_ref())
                .and_then(|s| s.pod_ip.clone())
                .map(|ip| ReplicaEndpoint {
                    control_addr: Some(format!("{}:{}", ip, instance.spec.control_port)),
                    cluster_addr: Some(format!("{}:{}", ip, instance.spec.cluster_port)),
                })
                .unwrap_or_default();

            let mut summary = ReplicaSummary {
                name: replica_name.clone(),
                role: ReplicaRole::ReadReplica,
                ready,
                node: pod
                    .as_ref()
                    .and_then(|p| p.spec.as_ref())
                    .and_then(|s| s.node_name.clone()),
                ..Default::default()
            };

            if let Some(control_addr) = endpoint.control_addr.clone() {
                all_control_targets.push(control_addr.clone());
                if let Ok(status) = get_status(&control_addr, shard_id).await {
                    summary.role = if status.role == Role::Master as i32 {
                        ReplicaRole::Master
                    } else {
                        ReplicaRole::ReadReplica
                    };
                    summary.epoch = Some(status.epoch);
                    summary.applied_offset = Some(status.applied_offset);
                    summary.current_offset = Some(status.current_offset);
                    summary.replication_lag = Some(status.replication_lag);
                    summary.master_addr = if status.master_addr.is_empty() {
                        None
                    } else {
                        Some(status.master_addr)
                    };
                    summary.ready = summary.ready && status.ready;
                    summary.config_epoch = Some(status.config_epoch);
                    health_replicas.push(ReplicaHealth {
                        name: replica_name.clone(),
                        role: summary.role,
                        ready: summary.ready,
                        last_heartbeat_unix_ms: status.last_heartbeat_unix_ms,
                        replication_lag: summary.replication_lag,
                    });

                    if summary.role == ReplicaRole::Master
                        && let Some(cluster_addr) = endpoint.cluster_addr.clone()
                    {
                        shard_master_cluster_addr.insert(shard_id, cluster_addr);
                    }
                }
            }

            endpoints.insert(replica_name.clone(), endpoint);
            replica_summaries.push(summary);
        }

        let now_ms = now_unix_ms();
        let mut shard_status = existing_status
            .shards
            .iter()
            .find(|s| s.shard_id == shard_id)
            .cloned()
            .unwrap_or(ShardStatus {
                shard_id,
                epoch: 0,
                master: None,
                replicas: vec![],
                ready_replicas: 0,
                expected_replicas: replicas_per_shard,
                current_offset: None,
                config_epoch: 0,
                migration_queue_len: 0,
                misplaced_tiles: 0,
                message: None,
                last_failover: None,
                cooldown_until: None,
            });

        if let Some(decision) = build_promotion_decision(
            &shard_status,
            &health_replicas,
            now_ms,
            heartbeat_timeout,
            false,
        ) {
            if let Some(promote_ep) = endpoints.get(&decision.promote)
                && let Some(control_addr) = promote_ep.control_addr.clone()
                && become_master(&control_addr, shard_id, decision.new_epoch).await
            {
                if let Some(cluster_addr) = promote_ep.cluster_addr.clone() {
                    shard_master_cluster_addr.insert(shard_id, cluster_addr.clone());
                    for replica in &replica_summaries {
                        if replica.name == decision.promote {
                            continue;
                        }
                        if let Some(rep_ep) = endpoints.get(&replica.name)
                            && let Some(control) = rep_ep.control_addr.clone()
                        {
                            let _ = become_replica(
                                &control,
                                shard_id,
                                decision.new_epoch,
                                &cluster_addr,
                            )
                            .await;
                        }
                    }
                }
                shard_status.epoch = decision.new_epoch;
                shard_status.master = Some(decision.promote.clone());
            }
        }

        let ready_replicas = replica_summaries.iter().filter(|r| r.ready).count() as u32;
        let current_master = replica_summaries
            .iter()
            .find(|r| r.role == ReplicaRole::Master)
            .map(|r| r.name.clone())
            .or(shard_status.master.clone());
        if current_master.is_none() {
            all_shards_healthy = false;
        }
        if ready_replicas == 0 {
            all_shards_healthy = false;
        }

        let current_offset = replica_summaries
            .iter()
            .filter_map(|r| r.current_offset)
            .max();

        let highest_config_epoch = replica_summaries
            .iter()
            .filter_map(|r| r.config_epoch)
            .max()
            .unwrap_or(0);

        shard_status.master = current_master;
        shard_status.replicas = replica_summaries;
        shard_status.ready_replicas = ready_replicas;
        shard_status.expected_replicas = replicas_per_shard;
        shard_status.current_offset = current_offset;
        shard_status.config_epoch = highest_config_epoch;
        new_shards.push(shard_status);
    }

    let num_slots = (instance.spec.num_slots as usize).max(DEFAULT_NUM_SLOTS);
    let slot_to_shard = compute_slot_to_shard(shard_count, num_slots);
    let routing = ClusterRoutingConfig {
        config_epoch: target_config_epoch,
        slot_to_shard,
        shard_masters: shard_master_cluster_addr,
    };

    let mut pushed = 0_u32;
    for target in all_control_targets {
        if update_routing_config(&target, routing.clone()).await {
            pushed = pushed.saturating_add(1);
        }
    }

    let phase = determine_cluster_phase(&new_shards, target_config_epoch, all_shards_healthy);
    let msg = Some(format!(
        "routing_epoch={} pushed_to={} targets",
        target_config_epoch, pushed
    ));

    actions::patch_cluster_status(
        client,
        &instance,
        phase,
        target_config_epoch,
        shard_count,
        msg,
        new_shards,
    )
    .await?;

    Ok(Action::requeue(PROBE_INTERVAL))
}

async fn reconcile_pod_topology(pods: &Api<Pod>, cluster: &Cluster) -> Result<(), Error> {
    let cluster_name = cluster.name_any();
    let replicas_per_shard = 1 + cluster.spec.topology.read_replicas;
    let mut desired = BTreeMap::new();
    for shard in 0..cluster.spec.topology.shards {
        for replica in 0..replicas_per_shard {
            let name = pod_name(&cluster_name, shard, replica);
            desired.insert(name.clone(), pod_resource(cluster, shard, replica));
        }
    }

    let existing = pods
        .list(&kube::api::ListParams::default().labels(&format!("eosin.io/cluster={cluster_name}")))
        .await?;
    for pod in existing.items {
        let pod_name = pod.name_any();
        if !desired.contains_key(&pod_name) {
            let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
        }
    }

    for (name, pod) in desired {
        if pods.get_opt(&name).await?.is_none() {
            let _ = pods.create(&PostParams::default(), &pod).await;
        }
    }

    Ok(())
}

fn pod_name(cluster: &str, shard: u32, replica: u32) -> String {
    format!("{cluster}-s{shard}-r{replica}")
}

fn pod_resource(cluster: &Cluster, shard_id: u32, replica_id: u32) -> Pod {
    let name = pod_name(&cluster.name_any(), shard_id, replica_id);
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "eosin-storage".to_string());
    labels.insert("eosin.io/cluster".to_string(), cluster.name_any());
    labels.insert("eosin.io/shard".to_string(), shard_id.to_string());
    labels.insert("eosin.io/replica".to_string(), replica_id.to_string());
    for (k, v) in &cluster.spec.extra_labels {
        labels.insert(k.clone(), v.clone());
    }

    Pod {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: cluster.namespace(),
            owner_references: Some(vec![cluster.controller_owner_ref(&()).unwrap()]),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "storage".to_string(),
                image: Some(cluster.spec.image.clone()),
                env: Some(vec![
                    EnvVar {
                        name: "API_PORT".to_string(),
                        value: Some(cluster.spec.api_port.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "CLUSTER_PORT".to_string(),
                        value: Some(cluster.spec.cluster_port.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "CONTROL_PORT".to_string(),
                        value: Some(cluster.spec.control_port.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SHARD".to_string(),
                        value: Some(shard_id.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "BACKLOG_CAPACITY".to_string(),
                        value: Some(cluster.spec.backlog_capacity.to_string()),
                        ..Default::default()
                    },
                ]),
                ports: Some(vec![
                    ContainerPort {
                        container_port: cluster.spec.api_port as i32,
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: cluster.spec.cluster_port as i32,
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: cluster.spec.control_port as i32,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }],
            restart_policy: Some("Always".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        })
}

async fn get_status(
    control_addr: &str,
    shard_id: u32,
) -> Result<crate::proto::cluster::GetShardStatusResponse, tonic::Status> {
    let mut client = ControlServiceClient::connect(format!("http://{control_addr}"))
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;
    client
        .get_shard_status(GetShardStatusRequest { shard_id })
        .await
        .map(|r| r.into_inner())
}

async fn become_master(control_addr: &str, shard_id: u32, epoch: u64) -> bool {
    let mut client = match ControlServiceClient::connect(format!("http://{control_addr}")).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .become_master(BecomeMasterRequest { shard_id, epoch })
        .await
        .map(|r| r.into_inner().accepted)
        .unwrap_or(false)
}

async fn become_replica(control_addr: &str, shard_id: u32, epoch: u64, master_addr: &str) -> bool {
    let mut client = match ControlServiceClient::connect(format!("http://{control_addr}")).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .become_replica(BecomeReplicaRequest {
            shard_id,
            epoch,
            master_addr: master_addr.to_string(),
        })
        .await
        .map(|r| r.into_inner().accepted)
        .unwrap_or(false)
}

async fn update_routing_config(control_addr: &str, config: ClusterRoutingConfig) -> bool {
    let mut client = match ControlServiceClient::connect(format!("http://{control_addr}")).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .update_routing_config(UpdateRoutingConfigRequest { config: Some(config) })
        .await
        .map(|r| r.into_inner().accepted)
        .unwrap_or(false)
}

fn on_error(_instance: Arc<Cluster>, _error: &Error, _context: Arc<ContextData>) -> Action {
    Action::requeue(Duration::from_secs(5))
}
