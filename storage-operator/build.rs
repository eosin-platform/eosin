use kube::CustomResourceExt;
use std::fs;
use eosin_types::*;

fn main() {
    unsafe {
        std::env::set_var("PROTOC", protobuf_src::protoc());
    }
    tonic_prost_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&["../storage/proto/cluster.proto"], &["../storage/proto"])
        .unwrap();

    let _ = fs::create_dir("../crds");
    fs::write(
        "../crds/storage.eosin.io_cluster_crd.yaml",
        serde_yaml::to_string(&Cluster::crd()).unwrap(),
    )
    .unwrap();
}
