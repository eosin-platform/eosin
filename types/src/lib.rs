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
    kind = "Storage",
    plural = "storages",
    derive = "PartialEq",
    status = "StorageStatus",
    namespaced
)]
#[kube(derive = "Default")]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.phase\", \"name\": \"PHASE\", \"type\": \"string\" }"
)]
#[kube(
    printcolumn = "{\"jsonPath\": \".status.lastUpdated\", \"name\": \"AGE\", \"type\": \"date\" }"
)]
pub struct StorageSpec {
    /// Shard + replica layout.
    pub topology: ClusterTopology,

    /// Container image for shard replicas.
    pub image: String,

    /// gRPC port exposed by shard replicas.
    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    /// StorageClass for the underlying volumes (if using StatefulSets/PVCs).
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

/// Status for a single shard.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ShardStatus {
    pub shard_id: u32,
    /// Name of the master replica (pod name).
    pub master: Option<String>,
    /// Expected replica count = 1 (master) + `spec.topology.read_replicas`.
    pub expected_replicas: u32,
    pub ready_replicas: u32,
    pub replicas: Vec<ReplicaStatus>,
}

/// Status object for the [`Storage`] resource.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default, JsonSchema)]
pub struct StorageStatus {
    /// A short description of the [`Storage`] resource's current state.
    pub phase: StoragePhase,

    /// A human-readable message indicating details about why the
    /// [`Storage`] is in this phase.
    pub message: Option<String>,

    /// Timestamp of when the [`StorageStatus`] object was last updated.
    #[serde(rename = "lastUpdated")]
    pub last_updated: Option<Time>,

    /// Per-shard state so you can see how the cluster is doing.
    #[serde(default)]
    pub shards: Vec<ShardStatus>,

    /// Standard Kubernetes conditions (Available, Progressing, Degraded…)
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

/// A short description of the [`Storage`] resource's current state.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, JsonSchema, Default)]
pub enum StoragePhase {
    #[default]
    Pending,
    Reconciling,
    Ready,
    Degraded,
    Error,
}

impl FromStr for StoragePhase {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(StoragePhase::Pending),
            "Reconciling" => Ok(StoragePhase::Reconciling),
            "Ready" => Ok(StoragePhase::Ready),
            "Degraded" => Ok(StoragePhase::Degraded),
            "Error" => Ok(StoragePhase::Error),
            _ => Err(()),
        }
    }
}

impl fmt::Display for StoragePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoragePhase::Pending => write!(f, "Pending"),
            StoragePhase::Reconciling => write!(f, "Reconciling"),
            StoragePhase::Ready => write!(f, "Ready"),
            StoragePhase::Degraded => write!(f, "Degraded"),
            StoragePhase::Error => write!(f, "Error"),
        }
    }
}
