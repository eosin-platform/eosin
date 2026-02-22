use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
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

#[derive(Clone, Debug)]
struct ReplicaObservation {
    name: String,
    node: Option<String>,
    pod_ready: bool,
    control_addr: Option<String>,
    cluster_addr: Option<String>,
    status: Option<crate::proto::cluster::GetShardStatusResponse>,
}

struct ReconcileComputation {
    phase: eosin_types::ClusterPhase,
    target_config_epoch: u64,
    applied_shards: u32,
    message: Option<String>,
    shards: Vec<ShardStatus>,
}

#[async_trait]
trait ControlPlane {
    async fn become_master(&self, control_addr: &str, shard_id: u32, epoch: u64) -> bool;
    async fn become_replica(
        &self,
        control_addr: &str,
        shard_id: u32,
        epoch: u64,
        master_addr: &str,
    ) -> bool;
    async fn update_routing_config(&self, control_addr: &str, config: ClusterRoutingConfig) -> bool;
}

struct GrpcControlPlane;

#[async_trait]
impl ControlPlane for GrpcControlPlane {
    async fn become_master(&self, control_addr: &str, shard_id: u32, epoch: u64) -> bool {
        become_master(control_addr, shard_id, epoch).await
    }

    async fn become_replica(
        &self,
        control_addr: &str,
        shard_id: u32,
        epoch: u64,
        master_addr: &str,
    ) -> bool {
        become_replica(control_addr, shard_id, epoch, master_addr).await
    }

    async fn update_routing_config(&self, control_addr: &str, config: ClusterRoutingConfig) -> bool {
        update_routing_config(control_addr, config).await
    }
}

async fn reconcile(instance: Arc<Cluster>, context: Arc<ContextData>) -> Result<Action, Error> {
    let client = context.client.clone();
    let namespace = instance
        .namespace()
        .ok_or_else(|| Error::UserInput("Cluster must be namespaced".to_string()))?;
    let name = instance.name_any();
    let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);

    reconcile_pod_topology(&pods, &instance).await?;

    let observations = collect_observations(&pods, &instance, &name).await?;
    let control = GrpcControlPlane;
    let computation =
        reconcile_from_observations(&instance, &observations, instance.status.clone().unwrap_or_default(), &control)
            .await;

    actions::patch_cluster_status(
        client,
        &instance,
        computation.phase,
        computation.target_config_epoch,
        computation.applied_shards,
        computation.message,
        computation.shards,
    )
    .await?;

    Ok(Action::requeue(PROBE_INTERVAL))
}

async fn collect_observations(
    pods: &Api<Pod>,
    instance: &Cluster,
    cluster_name: &str,
) -> Result<HashMap<u32, Vec<ReplicaObservation>>, Error> {
    let shard_count = instance.spec.topology.shards;
    let replicas_per_shard = 1 + instance.spec.topology.read_replicas;
    let mut result: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();

    for shard_id in 0..shard_count {
        let mut shard_replicas = Vec::new();
        for replica_id in 0..replicas_per_shard {
            let replica_name = pod_name(cluster_name, shard_id, replica_id);
            let pod = pods.get_opt(&replica_name).await?;
            let pod_ready = pod.as_ref().is_some_and(pod_ready);

            let endpoint = pod
                .as_ref()
                .and_then(|p| p.status.as_ref())
                .and_then(|s| s.pod_ip.clone())
                .map(|ip| ReplicaEndpoint {
                    control_addr: Some(format!("{}:{}", ip, instance.spec.control_port)),
                    cluster_addr: Some(format!("{}:{}", ip, instance.spec.cluster_port)),
                })
                .unwrap_or_default();

            let status = if let Some(control_addr) = endpoint.control_addr.clone() {
                get_status(&control_addr, shard_id).await.ok()
            } else {
                None
            };

            shard_replicas.push(ReplicaObservation {
                name: replica_name,
                node: pod
                    .as_ref()
                    .and_then(|p| p.spec.as_ref())
                    .and_then(|s| s.node_name.clone()),
                pod_ready,
                control_addr: endpoint.control_addr,
                cluster_addr: endpoint.cluster_addr,
                status,
            });
        }
        result.insert(shard_id, shard_replicas);
    }
    Ok(result)
}

async fn reconcile_from_observations(
    instance: &Cluster,
    observations: &HashMap<u32, Vec<ReplicaObservation>>,
    existing_status: eosin_types::ClusterStatus,
    control: &impl ControlPlane,
) -> ReconcileComputation {
    let shard_count = instance.spec.topology.shards;
    let replicas_per_shard = 1 + instance.spec.topology.read_replicas;
    let previous_shards = existing_status.applied_shards;
    let target_config_epoch =
        next_config_epoch(existing_status.config_epoch, previous_shards, shard_count);
    let heartbeat_timeout = Duration::from_secs(instance.spec.failover.heartbeat_timeout_seconds);

    let mut new_shards = Vec::new();
    let mut all_shards_healthy = true;
    let mut all_control_targets = Vec::new();
    let mut shard_master_cluster_addr: HashMap<u32, String> = HashMap::new();

    for shard_id in 0..shard_count {
        let mut replica_summaries = Vec::new();
        let mut endpoints: HashMap<String, ReplicaEndpoint> = HashMap::new();
        let mut health_replicas = Vec::new();

        let shard_observations = observations.get(&shard_id).cloned().unwrap_or_default();
        for obs in shard_observations {
            let endpoint = ReplicaEndpoint {
                control_addr: obs.control_addr.clone(),
                cluster_addr: obs.cluster_addr.clone(),
            };

            let mut summary = ReplicaSummary {
                name: obs.name.clone(),
                role: ReplicaRole::ReadReplica,
                ready: obs.pod_ready,
                node: obs.node,
                ..Default::default()
            };

            if let Some(control_addr) = obs.control_addr.clone() {
                all_control_targets.push(control_addr.clone());
                if let Some(status) = obs.status {
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
                        name: obs.name.clone(),
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

            endpoints.insert(obs.name.clone(), endpoint);
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
                && control
                    .become_master(&control_addr, shard_id, decision.new_epoch)
                    .await
            {
                if let Some(cluster_addr) = promote_ep.cluster_addr.clone() {
                    shard_master_cluster_addr.insert(shard_id, cluster_addr.clone());
                    for replica in &replica_summaries {
                        if replica.name == decision.promote {
                            continue;
                        }
                        if let Some(rep_ep) = endpoints.get(&replica.name)
                            && let Some(replica_control_addr) = rep_ep.control_addr.clone()
                        {
                            let _ = control
                                .become_replica(
                                    &replica_control_addr,
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
        let observed_master = replica_summaries
            .iter()
            .find(|r| r.role == ReplicaRole::Master)
            .map(|r| r.name.clone());
        let current_master = shard_status.master.clone().or(observed_master);
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
        if control.update_routing_config(&target, routing.clone()).await {
            pushed = pushed.saturating_add(1);
        }
    }

    let phase = determine_cluster_phase(&new_shards, target_config_epoch, all_shards_healthy);
    let msg = Some(format!(
        "routing_epoch={} pushed_to={} targets",
        target_config_epoch, pushed
    ));

    ReconcileComputation {
        phase,
        target_config_epoch,
        applied_shards: shard_count,
        message: msg,
        shards: new_shards,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::cluster::control_service_server::{ControlService, ControlServiceServer};
    use crate::proto::cluster::{
        BecomeMasterResponse, BecomeReplicaResponse, GetShardStatusResponse,
        UpdateRoutingConfigResponse,
    };
    use eosin_types::{
        ClusterSpec, ClusterStatus, ClusterTopology, FailoverPolicy, Placement, ReplicaResources,
    };
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    use tonic::{Request, Response, Status};

    #[derive(Default)]
    struct MockControlPlane {
        accept_become_master: bool,
        accept_become_replica: bool,
        update_accept: HashSet<String>,
        become_master_calls: Mutex<Vec<(String, u32, u64)>>,
        become_replica_calls: Mutex<Vec<(String, u32, u64, String)>>,
        update_calls: Mutex<Vec<(String, u64)>>,
    }

    #[async_trait]
    impl ControlPlane for MockControlPlane {
        async fn become_master(&self, control_addr: &str, shard_id: u32, epoch: u64) -> bool {
            self.become_master_calls
                .lock()
                .expect("lock")
                .push((control_addr.to_string(), shard_id, epoch));
            self.accept_become_master
        }

        async fn become_replica(
            &self,
            control_addr: &str,
            shard_id: u32,
            epoch: u64,
            master_addr: &str,
        ) -> bool {
            self.become_replica_calls
                .lock()
                .expect("lock")
                .push((
                    control_addr.to_string(),
                    shard_id,
                    epoch,
                    master_addr.to_string(),
                ));
            self.accept_become_replica
        }

        async fn update_routing_config(&self, control_addr: &str, config: ClusterRoutingConfig) -> bool {
            self.update_calls
                .lock()
                .expect("lock")
                .push((control_addr.to_string(), config.config_epoch));
            self.update_accept.contains(control_addr)
        }
    }

    fn cluster(shards: u32, read_replicas: u32, num_slots: u32) -> Cluster {
        let mut cluster = Cluster::new(
            "test-cluster",
            ClusterSpec {
                topology: ClusterTopology {
                    shards,
                    read_replicas,
                },
                image: "eosin/storage:test".to_string(),
                api_port: 3500,
                cluster_port: 4500,
                control_port: 4600,
                backlog_capacity: 4096,
                num_slots,
                storage_class_name: None,
                resources: ReplicaResources::default(),
                placement: Placement::default(),
                extra_labels: Default::default(),
                failover: FailoverPolicy {
                    heartbeat_timeout_seconds: 10,
                    cooldown_seconds: 15,
                },
            },
        );
        cluster.metadata.namespace = Some("default".to_string());
        cluster
    }

    fn status(
        role: Role,
        ready: bool,
        heartbeat_ms: u64,
        lag: u64,
        epoch: u64,
        config_epoch: u64,
        current_offset: u64,
        master_addr: &str,
    ) -> crate::proto::cluster::GetShardStatusResponse {
        crate::proto::cluster::GetShardStatusResponse {
            shard_id: 0,
            role: role as i32,
            epoch,
            applied_offset: current_offset,
            current_offset,
            last_heartbeat_unix_ms: heartbeat_ms,
            replication_lag: lag,
            ready,
            master_addr: master_addr.to_string(),
            config_epoch,
            migration_queue_len: 0,
            misplaced_tiles: 0,
        }
    }

    fn obs(
        name: &str,
        pod_ready: bool,
        control_addr: &str,
        cluster_addr: &str,
        status: Option<crate::proto::cluster::GetShardStatusResponse>,
    ) -> ReplicaObservation {
        ReplicaObservation {
            name: name.to_string(),
            node: Some("node-a".to_string()),
            pod_ready,
            control_addr: Some(control_addr.to_string()),
            cluster_addr: Some(cluster_addr.to_string()),
            status,
        }
    }

    fn default_existing_status(shards: u32) -> ClusterStatus {
        ClusterStatus {
            applied_shards: shards,
            ..Default::default()
        }
    }

    #[derive(Default)]
    struct WireState {
        become_master_requests: Mutex<Vec<BecomeMasterRequest>>,
        become_replica_requests: Mutex<Vec<BecomeReplicaRequest>>,
        update_requests: Mutex<Vec<UpdateRoutingConfigRequest>>,
        status_requests: Mutex<Vec<GetShardStatusRequest>>,
        become_master_accept: bool,
        become_replica_accept: bool,
        update_accept: bool,
        status_response: GetShardStatusResponse,
    }

    #[derive(Clone)]
    struct WireControlService {
        inner: Arc<WireState>,
    }

    #[tonic::async_trait]
    impl ControlService for WireControlService {
        async fn become_master(
            &self,
            request: Request<BecomeMasterRequest>,
        ) -> Result<Response<BecomeMasterResponse>, Status> {
            self.inner
                .become_master_requests
                .lock()
                .expect("lock")
                .push(request.into_inner());
            Ok(Response::new(BecomeMasterResponse {
                accepted: self.inner.become_master_accept,
                message: String::new(),
            }))
        }

        async fn become_replica(
            &self,
            request: Request<BecomeReplicaRequest>,
        ) -> Result<Response<BecomeReplicaResponse>, Status> {
            self.inner
                .become_replica_requests
                .lock()
                .expect("lock")
                .push(request.into_inner());
            Ok(Response::new(BecomeReplicaResponse {
                accepted: self.inner.become_replica_accept,
                message: String::new(),
            }))
        }

        async fn update_routing_config(
            &self,
            request: Request<UpdateRoutingConfigRequest>,
        ) -> Result<Response<UpdateRoutingConfigResponse>, Status> {
            self.inner
                .update_requests
                .lock()
                .expect("lock")
                .push(request.into_inner());
            Ok(Response::new(UpdateRoutingConfigResponse {
                accepted: self.inner.update_accept,
                message: String::new(),
            }))
        }

        async fn get_shard_status(
            &self,
            request: Request<GetShardStatusRequest>,
        ) -> Result<Response<GetShardStatusResponse>, Status> {
            self.inner
                .status_requests
                .lock()
                .expect("lock")
                .push(request.into_inner());
            Ok(Response::new(self.inner.status_response.clone()))
        }
    }

    async fn start_wire_server(
        state: Arc<WireState>,
    ) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let svc = WireControlService { inner: state };
        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ControlServiceServer::new(svc))
                .serve_with_shutdown(addr, async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("server should run");
        });

        tokio::time::sleep(Duration::from_millis(25)).await;
        (addr.to_string(), shutdown_tx, handle)
    }

    #[tokio::test]
    async fn promotes_replica_and_demotes_others_on_failover() {
        let cluster = cluster(1, 1, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![
                obs(
                    "test-cluster-s0-r0",
                    false,
                    "10.0.0.1:4600",
                    "10.0.0.1:4500",
                    Some(status(
                        Role::Master,
                        false,
                        now.saturating_sub(30_000),
                        0,
                        7,
                        1,
                        100,
                        "",
                    )),
                ),
                obs(
                    "test-cluster-s0-r1",
                    true,
                    "10.0.0.2:4600",
                    "10.0.0.2:4500",
                    Some(status(
                        Role::ReadReplica,
                        true,
                        now.saturating_sub(100),
                        5,
                        7,
                        1,
                        99,
                        "10.0.0.1:4500",
                    )),
                ),
            ],
        );

        let mut existing = default_existing_status(1);
        existing.shards.push(ShardStatus {
            shard_id: 0,
            epoch: 7,
            ..Default::default()
        });

        let control = MockControlPlane {
            accept_become_master: true,
            accept_become_replica: true,
            update_accept: ["10.0.0.1:4600".to_string(), "10.0.0.2:4600".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = reconcile_from_observations(&cluster, &observations, existing, &control).await;

        let shard = result.shards.first().expect("shard");
        assert_eq!(shard.epoch, 8);
        assert_eq!(shard.master.as_deref(), Some("test-cluster-s0-r1"));
        assert_eq!(
            control.become_master_calls.lock().expect("lock").len(),
            1,
            "promotion should be attempted once"
        );
        assert_eq!(
            control.become_replica_calls.lock().expect("lock").len(),
            1,
            "old master should be demoted"
        );
    }

    #[tokio::test]
    async fn rejected_promotion_keeps_previous_epoch_and_master() {
        let cluster = cluster(1, 1, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![
                obs(
                    "test-cluster-s0-r0",
                    false,
                    "10.0.0.1:4600",
                    "10.0.0.1:4500",
                    Some(status(
                        Role::Master,
                        false,
                        now.saturating_sub(25_000),
                        0,
                        3,
                        1,
                        100,
                        "",
                    )),
                ),
                obs(
                    "test-cluster-s0-r1",
                    true,
                    "10.0.0.2:4600",
                    "10.0.0.2:4500",
                    Some(status(
                        Role::ReadReplica,
                        true,
                        now.saturating_sub(200),
                        1,
                        3,
                        1,
                        99,
                        "10.0.0.1:4500",
                    )),
                ),
            ],
        );

        let mut existing = default_existing_status(1);
        existing.shards.push(ShardStatus {
            shard_id: 0,
            epoch: 3,
            master: Some("test-cluster-s0-r0".to_string()),
            ..Default::default()
        });

        let control = MockControlPlane {
            accept_become_master: false,
            accept_become_replica: true,
            update_accept: ["10.0.0.1:4600".to_string(), "10.0.0.2:4600".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = reconcile_from_observations(&cluster, &observations, existing, &control).await;
        let shard = result.shards.first().expect("shard");
        assert_eq!(shard.epoch, 3);
        assert_eq!(shard.master.as_deref(), Some("test-cluster-s0-r0"));
        assert_eq!(control.become_replica_calls.lock().expect("lock").len(), 0);
    }

    #[tokio::test]
    async fn routing_push_count_reflects_acceptances() {
        let cluster = cluster(1, 2, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![
                obs(
                    "test-cluster-s0-r0",
                    true,
                    "10.0.0.1:4600",
                    "10.0.0.1:4500",
                    Some(status(Role::Master, true, now, 0, 1, 1, 50, "")),
                ),
                obs(
                    "test-cluster-s0-r1",
                    true,
                    "10.0.0.2:4600",
                    "10.0.0.2:4500",
                    Some(status(
                        Role::ReadReplica,
                        true,
                        now,
                        0,
                        1,
                        1,
                        50,
                        "10.0.0.1:4500",
                    )),
                ),
                obs(
                    "test-cluster-s0-r2",
                    true,
                    "10.0.0.3:4600",
                    "10.0.0.3:4500",
                    Some(status(
                        Role::ReadReplica,
                        true,
                        now,
                        0,
                        1,
                        1,
                        50,
                        "10.0.0.1:4500",
                    )),
                ),
            ],
        );

        let control = MockControlPlane {
            update_accept: ["10.0.0.1:4600".to_string(), "10.0.0.3:4600".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = reconcile_from_observations(
            &cluster,
            &observations,
            default_existing_status(1),
            &control,
        )
        .await;

        let msg = result.message.expect("message");
        assert!(msg.contains("pushed_to=2"));
        assert_eq!(control.update_calls.lock().expect("lock").len(), 3);
    }

    #[tokio::test]
    async fn marks_reconciling_when_migrations_or_epoch_lag_exist() {
        let cluster = cluster(1, 0, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![obs(
                "test-cluster-s0-r0",
                true,
                "10.0.0.1:4600",
                "10.0.0.1:4500",
                Some(status(Role::Master, true, now, 0, 1, 0, 50, "")),
            )],
        );
        let mut existing = default_existing_status(1);
        existing.config_epoch = 4;

        let control = MockControlPlane {
            update_accept: ["10.0.0.1:4600".to_string()].into_iter().collect(),
            ..Default::default()
        };

        let result = reconcile_from_observations(&cluster, &observations, existing, &control).await;
        assert_eq!(result.phase, eosin_types::ClusterPhase::Reconciling);
    }

    #[tokio::test]
    async fn marks_degraded_without_master_or_ready_replicas() {
        let cluster = cluster(1, 0, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![obs(
                "test-cluster-s0-r0",
                false,
                "10.0.0.1:4600",
                "10.0.0.1:4500",
                Some(status(
                    Role::ReadReplica,
                    false,
                    now.saturating_sub(20_000),
                    0,
                    0,
                    1,
                    0,
                    "",
                )),
            )],
        );

        let control = MockControlPlane::default();
        let result = reconcile_from_observations(
            &cluster,
            &observations,
            default_existing_status(1),
            &control,
        )
        .await;

        assert_eq!(result.phase, eosin_types::ClusterPhase::Degraded);
    }

    #[tokio::test]
    async fn topology_change_bumps_target_config_epoch() {
        let cluster = cluster(2, 0, 16_384);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![obs(
                "test-cluster-s0-r0",
                true,
                "10.0.0.1:4600",
                "10.0.0.1:4500",
                Some(status(Role::Master, true, now, 0, 2, 2, 12, "")),
            )],
        );
        observations.insert(
            1,
            vec![obs(
                "test-cluster-s1-r0",
                true,
                "10.0.0.2:4600",
                "10.0.0.2:4500",
                Some(status(Role::Master, true, now, 0, 2, 2, 12, "")),
            )],
        );

        let mut existing = default_existing_status(1);
        existing.config_epoch = 9;
        let control = MockControlPlane {
            update_accept: ["10.0.0.1:4600".to_string(), "10.0.0.2:4600".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = reconcile_from_observations(&cluster, &observations, existing, &control).await;
        assert_eq!(result.target_config_epoch, 10);
        let updates = control.update_calls.lock().expect("lock");
        assert!(updates.iter().all(|(_, epoch)| *epoch == 10));
    }

    #[tokio::test]
    async fn enforces_minimum_slot_table_size_when_spec_is_smaller() {
        let cluster = cluster(2, 0, 64);
        let now = now_unix_ms();
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![obs(
                "test-cluster-s0-r0",
                true,
                "10.0.0.1:4600",
                "10.0.0.1:4500",
                Some(status(Role::Master, true, now, 0, 1, 1, 10, "")),
            )],
        );
        observations.insert(
            1,
            vec![obs(
                "test-cluster-s1-r0",
                true,
                "10.0.0.2:4600",
                "10.0.0.2:4500",
                Some(status(Role::Master, true, now, 0, 1, 1, 10, "")),
            )],
        );

        let control = MockControlPlane {
            update_accept: ["10.0.0.1:4600".to_string(), "10.0.0.2:4600".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let _ = reconcile_from_observations(
            &cluster,
            &observations,
            default_existing_status(2),
            &control,
        )
        .await;
        let updates = control.update_calls.lock().expect("lock");
        assert_eq!(updates.len(), 2);
    }

    #[tokio::test]
    async fn missing_telemetry_is_tolerated_and_cluster_is_degraded() {
        let cluster = cluster(1, 0, 16_384);
        let mut observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        observations.insert(
            0,
            vec![obs(
                "test-cluster-s0-r0",
                true,
                "10.0.0.1:4600",
                "10.0.0.1:4500",
                None,
            )],
        );

        let control = MockControlPlane {
            update_accept: ["10.0.0.1:4600".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let result = reconcile_from_observations(
            &cluster,
            &observations,
            default_existing_status(1),
            &control,
        )
        .await;

        assert_eq!(result.phase, eosin_types::ClusterPhase::Degraded);
        assert_eq!(control.update_calls.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn zero_shards_results_in_empty_ready_state() {
        let cluster = cluster(0, 0, 16_384);
        let observations: HashMap<u32, Vec<ReplicaObservation>> = HashMap::new();
        let control = MockControlPlane::default();

        let result = reconcile_from_observations(
            &cluster,
            &observations,
            default_existing_status(0),
            &control,
        )
        .await;

        assert_eq!(result.applied_shards, 0);
        assert!(result.shards.is_empty());
        assert_eq!(result.phase, eosin_types::ClusterPhase::Ready);
    }

    #[tokio::test]
    async fn grpc_get_status_round_trip_uses_expected_request() {
        let state = Arc::new(WireState {
            status_response: GetShardStatusResponse {
                shard_id: 9,
                role: Role::Master as i32,
                epoch: 77,
                applied_offset: 11,
                current_offset: 12,
                last_heartbeat_unix_ms: 123,
                replication_lag: 1,
                ready: true,
                master_addr: "10.1.1.1:4500".to_string(),
                config_epoch: 8,
                migration_queue_len: 0,
                misplaced_tiles: 0,
            },
            ..Default::default()
        });
        let (addr, shutdown, handle) = start_wire_server(state.clone()).await;

        let result = get_status(&addr, 9).await.expect("get_status ok");
        assert_eq!(result.epoch, 77);
        assert_eq!(result.config_epoch, 8);
        assert_eq!(result.role, Role::Master as i32);
        let requests = state.status_requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].shard_id, 9);

        let _ = shutdown.send(());
        let _ = handle.await;
    }

    #[tokio::test]
    async fn grpc_become_master_forwards_epoch_and_shard() {
        let state = Arc::new(WireState {
            become_master_accept: true,
            ..Default::default()
        });
        let (addr, shutdown, handle) = start_wire_server(state.clone()).await;

        let accepted = become_master(&addr, 4, 21).await;
        assert!(accepted);
        let requests = state.become_master_requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].shard_id, 4);
        assert_eq!(requests[0].epoch, 21);

        let _ = shutdown.send(());
        let _ = handle.await;
    }

    #[tokio::test]
    async fn grpc_become_replica_forwards_master_address() {
        let state = Arc::new(WireState {
            become_replica_accept: true,
            ..Default::default()
        });
        let (addr, shutdown, handle) = start_wire_server(state.clone()).await;

        let accepted = become_replica(&addr, 3, 19, "10.2.2.2:4500").await;
        assert!(accepted);
        let requests = state.become_replica_requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].shard_id, 3);
        assert_eq!(requests[0].epoch, 19);
        assert_eq!(requests[0].master_addr, "10.2.2.2:4500");

        let _ = shutdown.send(());
        let _ = handle.await;
    }

    #[tokio::test]
    async fn grpc_update_routing_config_forwards_slots_and_masters() {
        let state = Arc::new(WireState {
            update_accept: true,
            ..Default::default()
        });
        let (addr, shutdown, handle) = start_wire_server(state.clone()).await;

        let mut shard_masters = HashMap::new();
        shard_masters.insert(0, "10.0.0.1:4500".to_string());
        shard_masters.insert(1, "10.0.0.2:4500".to_string());
        let accepted = update_routing_config(
            &addr,
            ClusterRoutingConfig {
                config_epoch: 42,
                slot_to_shard: vec![0, 0, 1, 1],
                shard_masters,
            },
        )
        .await;
        assert!(accepted);

        let requests = state.update_requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        let cfg = requests[0].config.as_ref().expect("config");
        assert_eq!(cfg.config_epoch, 42);
        assert_eq!(cfg.slot_to_shard, vec![0, 0, 1, 1]);
        assert_eq!(cfg.shard_masters.get(&1).map(String::as_str), Some("10.0.0.2:4500"));

        let _ = shutdown.send(());
        let _ = handle.await;
    }

    #[tokio::test]
    async fn grpc_helpers_handle_unavailable_endpoints() {
        let unreachable = "127.0.0.1:1";
        assert!(get_status(unreachable, 0).await.is_err());
        assert!(!become_master(unreachable, 0, 1).await);
        assert!(!become_replica(unreachable, 0, 1, "127.0.0.1:2").await);
        assert!(
            !update_routing_config(
                unreachable,
                ClusterRoutingConfig {
                    config_epoch: 1,
                    slot_to_shard: vec![0],
                    shard_masters: HashMap::new(),
                },
            )
            .await
        );
    }
}
