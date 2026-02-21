use kube::CustomResourceExt;
use std::fs;
use eosin_types::*;

fn main() {
    let _ = fs::create_dir("../crds");
    fs::write(
        "../crds/storage.eosin.io_cluster_crd.yaml",
        serde_yaml::to_string(&Cluster::crd()).unwrap(),
    )
    .unwrap();
}
