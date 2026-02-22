use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ClusterTopology {
    pub shards: u32,
    pub read_replicas: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ReplicaResources {
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub storage: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct Placement {
    pub node_selector: Option<std::collections::BTreeMap<String, String>>,
    pub tolerations: Option<std::collections::BTreeMap<String, String>>,
    pub topology_key: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct FailoverPolicy {
    #[serde(default = "default_heartbeat_timeout_seconds")]
    pub heartbeat_timeout_seconds: u64,
    #[serde(default = "default_failover_cooldown_seconds")]
    pub cooldown_seconds: u64,
}

impl Default for FailoverPolicy {
    fn default() -> Self {
        Self {
            heartbeat_timeout_seconds: default_heartbeat_timeout_seconds(),
            cooldown_seconds: default_failover_cooldown_seconds(),
        }
    }
}

fn default_api_port() -> u16 {
    3500
}

fn default_cluster_port() -> u16 {
    4500
}

fn default_control_port() -> u16 {
    4600
}

fn default_backlog_capacity() -> u32 {
    4096
}

fn default_num_slots() -> u32 {
    16384
}

fn default_heartbeat_timeout_seconds() -> u64 {
    10
}

fn default_failover_cooldown_seconds() -> u64 {
    15
}

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "storage.eosin.io",
    version = "v1",
    kind = "Cluster",
    plural = "clusters",
    derive = "PartialEq",
    status = "ClusterStatus",
    namespaced
)]
#[kube(derive = "Default")]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.phase\", \"name\": \"PHASE\", \"type\": \"string\" }"
)]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.lastUpdated\", \"name\": \"AGE\", \"type\": \"date\" }"
)]
pub struct ClusterSpec {
    pub topology: ClusterTopology,
    pub image: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_cluster_port")]
    pub cluster_port: u16,
    #[serde(default = "default_control_port")]
    pub control_port: u16,
    #[serde(default = "default_backlog_capacity")]
    pub backlog_capacity: u32,
    #[serde(default = "default_num_slots")]
    pub num_slots: u32,
    pub storage_class_name: Option<String>,
    #[serde(default)]
    pub resources: ReplicaResources,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default)]
    pub extra_labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub failover: FailoverPolicy,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, JsonSchema)]
pub enum ReplicaRole {
    Master,
    ReadReplica,
}

impl Default for ReplicaRole {
    fn default() -> Self {
        Self::ReadReplica
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct ReplicaSummary {
    pub name: String,
    pub role: ReplicaRole,
    pub ready: bool,
    pub node: Option<String>,
    pub last_heartbeat: Option<Time>,
    pub epoch: Option<u64>,
    pub applied_offset: Option<u64>,
    pub current_offset: Option<u64>,
    pub replication_lag: Option<u64>,
    pub master_addr: Option<String>,
    pub config_epoch: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct ShardStatus {
    pub shard_id: u32,
    pub epoch: u64,
    pub master: Option<String>,
    pub replicas: Vec<ReplicaSummary>,
    pub ready_replicas: u32,
    pub expected_replicas: u32,
    pub current_offset: Option<u64>,
    pub config_epoch: u64,
    pub migration_queue_len: u64,
    pub misplaced_tiles: u64,
    pub message: Option<String>,
    pub last_failover: Option<Time>,
    pub cooldown_until: Option<Time>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct ClusterStatus {
    pub phase: ClusterPhase,
    pub config_epoch: u64,
    pub applied_shards: u32,
    pub message: Option<String>,
    #[serde(rename = "lastUpdated")]
    pub last_updated: Option<Time>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub shards: Vec<ShardStatus>,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, JsonSchema, Default)]
pub enum ClusterPhase {
    #[default]
    Pending,
    Reconciling,
    Ready,
    Degraded,
    Error,
}

impl FromStr for ClusterPhase {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(ClusterPhase::Pending),
            "Reconciling" => Ok(ClusterPhase::Reconciling),
            "Ready" => Ok(ClusterPhase::Ready),
            "Degraded" => Ok(ClusterPhase::Degraded),
            "Error" => Ok(ClusterPhase::Error),
            _ => Err(()),
        }
    }
}

impl fmt::Display for ClusterPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClusterPhase::Pending => write!(f, "Pending"),
            ClusterPhase::Reconciling => write!(f, "Reconciling"),
            ClusterPhase::Ready => write!(f, "Ready"),
            ClusterPhase::Degraded => write!(f, "Degraded"),
            ClusterPhase::Error => write!(f, "Error"),
        }
    }
}