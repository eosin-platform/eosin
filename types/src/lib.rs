use k8s_openapi::{apimachinery::pkg::apis::meta::v1::{Condition, Time}};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Topology description: how many shards and read replicas per shard.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ClusterTopology {
    /// Number of logical shards.
    pub shards: u32,
    /// Number of read replicas per shard.
    pub read_replicas: u32,
}

/// Resource requests/limits for a single replica.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ReplicaResources {
    /// e.g. "500m"
    pub cpu: Option<String>,
    /// e.g. "1Gi"
    pub memory: Option<String>,
    /// e.g. "10Gi"
    pub storage: Option<String>,
}

/// Pod placement hints so you can do anti-affinity / zones later.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct Placement {
    /// Hard requirement: node selector.
    pub node_selector: Option<std::collections::BTreeMap<String, String>>,
    /// Node taint tolerations.
    pub tolerations: Option<std::collections::BTreeMap<String, String>>,
    /// Soft preference: topology spread key, etc.
    pub topology_key: Option<String>,
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
    /// Shard + replica layout.
    pub topology: ClusterTopology,

    /// Container image for shard replicas.
    pub image: String,

    /// gRPC port exposed by shard replicas.
    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    /// ClusterClass for the underlying volumes (if using StatefulSets/PVCs).
    pub storage_class_name: Option<String>,

    /// Default resources per replica.
    #[serde(default)]
    pub resources: ReplicaResources,

    /// Placement hints (anti-affinity, zones, etc.)
    #[serde(default)]
    pub placement: Placement,

    /// Optional additional labels applied to all child resources.
    #[serde(default)]
    pub extra_labels: std::collections::BTreeMap<String, String>,
}

fn default_grpc_port() -> u16 {
    50051
}

/// Role of a replica within a shard.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum ReplicaRole {
    Master,
    ReadReplica,
}

impl Default for ReplicaRole {
    fn default() -> Self {
        ReplicaRole::Master
    }
}

/// Per-replica status: what the gRPC cluster is telling you.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ReplicaStatus {
    /// Pod name or StatefulSet ordinal.
    pub name: String,
    pub role: ReplicaRole,
    pub ready: bool,
    /// Node it’s scheduled on (handy for debugging placement).
    pub node: Option<String>,
    /// Last heartbeat received over gRPC from this replica.
    pub last_heartbeat: Option<Time>,
}


/// Status object for the [`Cluster`] resource.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct ClusterStatus {
    /// A short description of the [`Cluster`] resource's current state.
    pub phase: ClusterPhase,

    /// A human-readable message indicating details about why the
    /// [`Cluster`] is in this phase.
    pub message: Option<String>,

    /// Timestamp of when the [`ClusterStatus`] object was last updated.
    #[serde(rename = "lastUpdated")]
    pub last_updated: Option<Time>,

    /// Standard Kubernetes conditions (Available, Progressing, Degraded…)
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

/// A short description of the [`Cluster`] resource's current state.
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

#[derive(
    CustomResource,
    Serialize,
    Deserialize,
    Clone,
    Debug,
    PartialEq,
    JsonSchema,
    Default,
)]
#[kube(
    group = "storage.eosin.io",
    version = "v1",
    kind = "Shard",
    plural = "shards",
    status = "ShardStatus",
    namespaced
)]
#[kube(derive = "Default")]
#[kube(
    printcolumn = "{\"jsonPath\": \".spec.shardId\", \"name\": \"SHARD\", \"type\": \"integer\" }"
)]
#[kube(
    printcolumn = "{\"jsonPath\": \".spec.replicas\", \"name\": \"REPLICAS\", \"type\": \"integer\" }"
)]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.phase\", \"name\": \"PHASE\", \"type\": \"string\" }"
)]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.lastUpdated\", \"name\": \"AGE\", \"type\": \"date\" }"
)]
pub struct ShardSpec {
    /// Parent Cluster name.
    pub cluster: String,

    /// Shard index within the Cluster (0..shards-1).
    #[serde(rename = "shardId")]
    pub shard_id: u32,

    /// Total replicas in this shard (1 master + N read replicas).
    pub replicas: u32,

    /// Optional image override (defaults to Cluster.spec.image).
    pub image: Option<String>,

    /// Optional gRPC port override (defaults to Cluster.spec.grpc_port).
    pub grpc_port: Option<u16>,

    /// Optional per-replica resource override (defaults to Cluster.spec.resources).
    #[serde(default)]
    pub resources: Option<ReplicaResources>,

    /// Optional per-shard placement override.
    #[serde(default)]
    pub placement: Option<Placement>,

    /// Which replica index is *intended* to be master for this shard (e.g. 0, 1, 2…).
    #[serde(rename = "desiredMaster")]
    pub desired_master: Option<u32>,
}

/// Phase of a Shard's lifecycle.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, JsonSchema, Default)]
pub enum ShardPhase {
    #[default]
    Pending,
    Reconciling,
    Ready,
    Degraded,
    Error,
}

impl FromStr for ShardPhase {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(ShardPhase::Pending),
            "Reconciling" => Ok(ShardPhase::Reconciling),
            "Ready" => Ok(ShardPhase::Ready),
            "Degraded" => Ok(ShardPhase::Degraded),
            "Error" => Ok(ShardPhase::Error),
            _ => Err(()),
        }
    }
}

impl fmt::Display for ShardPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShardPhase::Pending => write!(f, "Pending"),
            ShardPhase::Reconciling => write!(f, "Reconciling"),
            ShardPhase::Ready => write!(f, "Ready"),
            ShardPhase::Degraded => write!(f, "Degraded"),
            ShardPhase::Error => write!(f, "Error"),
        }
    }
}

/// Status for a single Shard.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct ShardStatus {
    /// High-level lifecycle phase for this shard.
    pub phase: ShardPhase,

    /// Name of the current master replica (e.g. "cluster-s0-r1").
    pub master: Option<String>,

    /// Summary of all replicas in this shard.
    #[serde(default)]
    pub replicas: Vec<ReplicaStatus>,

    /// Number of replicas that are currently Ready.
    pub ready_replicas: u32,

    /// Expected replica count (e.g. 1 + read_replicas).
    pub expected_replicas: u32,

    /// Timestamp of the last status update for this shard.
    #[serde(rename = "lastUpdated")]
    pub last_updated: Option<Time>,

    /// Standard Kubernetes conditions.
    #[serde(default)]
    pub conditions: Vec<Condition>,
}