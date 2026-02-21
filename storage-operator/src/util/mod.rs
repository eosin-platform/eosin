use std::time::Duration;
pub mod metrics;
pub mod patch;

pub(crate) mod colors;

mod error;
mod merge;

pub use error::*;

/// The default interval for requeuing a managed resource.
pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(30);

/// Name of the kubernetes resource manager.
pub(crate) const MANAGER_NAME: &str = "eosin-operator";

pub fn hash_spec<T: serde::Serialize>(spec: &T) -> String {
    use sha2::{Digest, Sha256};
    let spec_bytes = serde_json::to_vec(spec).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&spec_bytes);
    let result = hasher.finalize();
    hex::encode(result)
}
