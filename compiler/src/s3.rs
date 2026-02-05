use anyhow::{Context, Result};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::config::Region;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::args::S3Args;

/// Create an S3 client from the provided arguments.
pub async fn create_s3_client(args: &S3Args) -> Result<S3Client> {
    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(args.region.clone()));

    if let Some(ref endpoint) = args.endpoint {
        config_loader = config_loader.endpoint_url(endpoint);
    }

    let config = config_loader.load().await;
    let client = S3Client::new(&config);

    Ok(client)
}

/// List all TIF files in the bucket with the given prefix.
pub async fn list_tif_files(
    client: &S3Client,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut request = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(prefix);

        if let Some(token) = continuation_token.take() {
            request = request.continuation_token(token);
        }

        let response = request
            .send()
            .await
            .context("failed to list S3 objects")?;

        if let Some(contents) = response.contents {
            for object in contents {
                if let Some(key) = object.key {
                    // Check if it's a .tif file (case-insensitive)
                    if key.to_lowercase().ends_with(".tif") || key.to_lowercase().ends_with(".tiff") {
                        keys.push(key);
                    }
                }
            }
        }

        if response.is_truncated.unwrap_or(false) {
            continuation_token = response.next_continuation_token;
        } else {
            break;
        }
    }

    Ok(keys)
}

/// Download a file from S3 to the local filesystem.
pub async fn download_file(
    client: &S3Client,
    bucket: &str,
    key: &str,
    dest_dir: &str,
) -> Result<String> {
    // Create the destination directory if it doesn't exist
    tokio::fs::create_dir_all(dest_dir)
        .await
        .context("failed to create download directory")?;

    // Extract filename from key
    let filename = Path::new(key)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(key);

    let dest_path = format!("{dest_dir}/{filename}");

    // Check if file already exists
    if tokio::fs::try_exists(&dest_path).await.unwrap_or(false) {
        tracing::info!(path = %dest_path, "file already exists, skipping download");
        return Ok(dest_path);
    }

    tracing::info!(bucket = %bucket, key = %key, dest = %dest_path, "downloading file from S3");

    let response = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .context("failed to get object from S3")?;

    let mut file = File::create(&dest_path)
        .await
        .context("failed to create destination file")?;

    let body = response
        .body
        .collect()
        .await
        .context("failed to read S3 object body")?;

    file.write_all(&body.into_bytes())
        .await
        .context("failed to write file")?;

    file.flush().await.context("failed to flush file")?;

    tracing::info!(path = %dest_path, "download complete");

    Ok(dest_path)
}
