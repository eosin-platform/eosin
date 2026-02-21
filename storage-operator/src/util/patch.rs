use super::MANAGER_NAME;
use k8s_openapi::{apimachinery::pkg::apis::meta::v1::Time, jiff::Timestamp};
use kube::{
    Api, Client, Error,
    api::{Patch, PatchParams, Resource},
    core::NamespaceResourceScope,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{clone::Clone, fmt::Debug};
use eosin_types::*;

pub trait Object<S: Status> {
    /// Returns a mutable reference to the status object, initializing
    /// it with the default value if it does not exist.
    fn mut_status(&mut self) -> &mut S;
}

pub trait Status {
    /// Sets the last updated timestamp to the given value.
    fn set_last_updated(&mut self, last_updated: Time);
}

impl Object<ClusterStatus> for Cluster {
    fn mut_status(&mut self) -> &mut ClusterStatus {
        if self.status.is_some() {
            return self.status.as_mut().unwrap();
        }
        self.status = Some(Default::default());
        self.status.as_mut().unwrap()
    }
}

impl Status for ClusterStatus {
    fn set_last_updated(&mut self, last_updated: Time) {
        self.last_updated = Some(last_updated);
    }
}

/// Patch the resource's status object with the provided function.
/// The function is passed a mutable reference to the status object,
/// which is to be mutated in-place. Move closures are supported.
pub async fn patch_status<S: Status, T>(
    client: Client,
    instance: &T,
    f: impl FnOnce(&mut S),
) -> Result<T, Error>
where
    <T as Resource>::DynamicType: Default,
    T: Clone
        + Resource
        + Object<S>
        + Serialize
        + DeserializeOwned
        + Debug
        + Resource<Scope = NamespaceResourceScope>,
{
    let patch = Patch::Json::<T>({
        let mut modified = instance.clone();
        let status = modified.mut_status();
        f(status);
        status.set_last_updated(Time::from(Timestamp::now()));
        json_patch::diff(
            &serde_json::to_value(instance).unwrap(),
            &serde_json::to_value(&modified).unwrap(),
        )
    });
    let name = instance.meta().name.as_deref().unwrap();
    let namespace = instance.meta().namespace.as_deref().unwrap();
    let api: Api<T> = Api::namespaced(client, namespace);
    api.patch_status(name, &PatchParams::apply(MANAGER_NAME), &patch)
        .await
}
