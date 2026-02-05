fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use bundled protoc
    // SAFETY: This is safe in a build script context where we control the environment
    unsafe {
        std::env::set_var("PROTOC", protobuf_src::protoc());
    }
    
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["proto/storage.proto", "proto/cluster.proto"],
            &["proto/"],
        )?;
    Ok(())
}
