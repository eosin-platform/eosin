use anyhow::{Context, Result};
use async_nats::jetstream::{self, consumer::PullConsumer};
use deadpool_postgres::Pool;
use futures::StreamExt;
use histion_common::postgres::create_pool;
use histion_common::shutdown::shutdown_signal;
use histion_common::streams::{ProcessSlideEvent, topics::PROCESS_SLIDE};
use histion_storage::StorageClient;
use std::path::Path;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Namespace UUID for generating deterministic slide IDs from S3 keys.
/// This is a custom namespace specific to this application.
const SLIDE_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

use crate::args::ProcessArgs;
use crate::db;
use crate::meta_client::MetaClient;
use crate::s3;
use crate::tiler;

/// Run the process worker.
pub async fn run_process(args: ProcessArgs) -> Result<()> {
    // Configure rayon thread pool for parallel tile processing
    if args.tile_threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.tile_threads)
            .build_global()
            .context("failed to configure thread pool")?;
        tracing::info!(threads = args.tile_threads, "configured thread pool");
    } else {
        tracing::info!(
            threads = rayon::current_num_threads(),
            "using default thread pool"
        );
    }

    tracing::info!(
        bucket = %args.s3.bucket,
        prefix = %args.s3.path_prefix,
        download_dir = %args.download_dir,
        meta_endpoint = %args.meta_endpoint,
        storage_endpoint = %args.storage_endpoint,
        "starting process worker"
    );

    // Create postgres pool for checkpointing
    let pg_pool = create_pool(args.postgres.clone()).await;
    db::init_schema(&pg_pool).await?;
    tracing::info!("connected to postgres");

    // Create S3 client
    let s3_client = s3::create_s3_client(&args.s3).await?;
    tracing::info!("connected to S3");

    // Create meta client
    let meta_client = MetaClient::new(&args.meta_endpoint);
    tracing::info!(endpoint = %args.meta_endpoint, "meta client ready");

    // Create storage client
    let storage_client = StorageClient::connect(&args.storage_endpoint)
        .await
        .context("failed to connect to storage service")?;
    tracing::info!(endpoint = %args.storage_endpoint, "connected to storage service");

    // Connect to NATS
    let nats = args.nats.connect().await?;
    let nats_core = nats.clone(); // Keep a reference for core NATS publishing
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Create JetStream context
    let jetstream = jetstream::new(nats);

    // Get or create the stream
    let stream = jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: args.stream_name.clone(),
            subjects: vec![PROCESS_SLIDE.to_string()],
            ..Default::default()
        })
        .await
        .context("failed to get or create stream")?;
    tracing::info!(stream = %args.stream_name, "connected to stream");

    // Create or get a durable pull consumer
    let consumer: PullConsumer = stream
        .get_or_create_consumer(
            &args.consumer_name,
            jetstream::consumer::pull::Config {
                durable_name: Some(args.consumer_name.clone()),
                filter_subject: PROCESS_SLIDE.to_string(),
                ..Default::default()
            },
        )
        .await
        .context("failed to create consumer")?;
    tracing::info!(consumer = %args.consumer_name, "consumer ready");

    // Process messages with graceful shutdown
    let mut messages = consumer.messages().await?;
    tracing::info!("listening for process slide events");

    let cancel = CancellationToken::new();
    let signal_cancel = cancel.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal_cancel.cancel();
    });

    histion_common::signal_ready();

    // Clone clients for use in the loop
    let bucket = args.s3.bucket.clone();
    let download_dir = args.download_dir.clone();
    let nats_for_tiles = nats_core.clone();

    // Clear the download directory at startup. It's a mounted dir so we can't
    // remove it directly and must remove contents only.
    let download_path = Path::new(&download_dir);
    let mut entries = tokio::fs::read_dir(download_path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            tokio::fs::remove_file(path).await?;
        } else if path.is_dir() {
            tokio::fs::remove_dir_all(path).await?;
        }
    }
    tracing::info!(path = %download_dir, "cleared download directory");

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tracing::info!("shutdown signal received, stopping worker");
                break;
            }
            msg = messages.next() => {
                match msg {
                    Some(Ok(message)) => {
                        // Clone storage_client for this message processing
                        let mut storage = storage_client.clone();

                        if let Err(e) = handle_process_slide(
                            &message.payload,
                            &s3_client,
                            &bucket,
                            &download_dir,
                            &meta_client,
                            &mut storage,
                            &nats_for_tiles,
                            &pg_pool,
                            cancel.clone(),
                        ).await {
                            tracing::error!(?e, "failed to process slide");
                            // Don't ack on error - message will be redelivered
                            continue;
                        }

                        // Acknowledge the message
                        if let Err(e) = message.ack().await {
                            tracing::error!(?e, "failed to ack message");
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!(?e, "error receiving message");
                    }
                    None => {
                        tracing::warn!("message stream ended unexpectedly");
                        break;
                    }
                }
            }
        }
    }

    tracing::info!("process worker stopped");
    Ok(())
}

/// Handle a `ProcessSlideEvent`: download TIF, insert metadata, extract and upload tiles.
async fn handle_process_slide(
    payload: &[u8],
    s3_client: &aws_sdk_s3::Client,
    bucket: &str,
    download_dir: &str,
    meta_client: &MetaClient,
    storage_client: &mut StorageClient,
    nats_client: &async_nats::Client,
    pg_pool: &Pool,
    cancel: CancellationToken,
) -> Result<()> {
    let event: ProcessSlideEvent =
        serde_json::from_slice(payload).context("failed to deserialize ProcessSlideEvent")?;

    tracing::info!(key = %event.key, "processing slide");

    // Download the TIF file
    let local_path = s3::download_file(s3_client, bucket, &event.key, download_dir).await?;
    tracing::info!(key = %event.key, path = %local_path, "slide downloaded");

    // Process the slide, ensuring cleanup happens regardless of success/failure
    let result = process_downloaded_slide(
        &local_path,
        &event.key,
        meta_client,
        storage_client,
        nats_client,
        pg_pool,
        cancel,
    )
    .await;

    // Always delete the local file to free up space
    if let Err(e) = tokio::fs::remove_file(&local_path).await {
        tracing::warn!(path = %local_path, error = ?e, "failed to delete local file");
    } else {
        tracing::debug!(path = %local_path, "deleted local file");
    }

    result
}

/// Process a downloaded slide file: insert metadata first, then extract and upload tiles.
///
/// Slide metadata is inserted into the meta service first, then tiles are processed
/// from highest mip level (lowest resolution) to level 0 (full resolution).
/// This allows the slide to be viewable at low resolution while still processing.
async fn process_downloaded_slide(
    local_path: &str,
    key: &str,
    meta_client: &MetaClient,
    storage_client: &mut StorageClient,
    nats_client: &async_nats::Client,
    pg_pool: &Pool,
    cancel: CancellationToken,
) -> Result<()> {
    let path = Path::new(local_path);

    // Generate deterministic UUID from S3 key
    let slide_id = Uuid::new_v5(&SLIDE_NAMESPACE, key.as_bytes());

    // Get file size for metadata
    let file_metadata = tokio::fs::metadata(path)
        .await
        .context("failed to get file metadata")?;
    let full_size = file_metadata.len() as i64;

    // Get slide metadata first
    let metadata = tiler::get_slide_metadata(path).context("failed to extract slide metadata")?;

    // Extract filename (with extension) from the S3 key
    let filename = std::path::Path::new(key)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(key)
        .to_string();

    tracing::info!(
        key = %key,
        slide_id = %slide_id,
        width = metadata.width,
        height = metadata.height,
        levels = metadata.level_count,
        full_size = full_size,
        filename = %filename,
        "extracted slide metadata"
    );

    // Insert metadata into meta service FIRST
    // This allows the slide to be visible immediately (at low resolution)
    let slide = meta_client
        .create_slide(slide_id, metadata.width, metadata.height, key, &filename, full_size)
        .await
        .context("failed to create slide in meta service")?;

    tracing::info!(
        key = %key,
        slide_id = %slide.id,
        "slide metadata inserted, processing tiles"
    );

    // Process the slide: extract tiles and upload to storage
    // Tiles are processed from highest mip level (lowest resolution) to full resolution
    // pg_pool is used for checkpointing to allow resuming on restart
    tiler::process_slide(path, slide_id, storage_client, nats_client, meta_client, Some(pg_pool), cancel)
        .await
        .context("failed to process slide tiles")?;

    tracing::info!(
        key = %key,
        slide_id = %slide_id,
        "all tiles uploaded, processing complete"
    );

    Ok(())
}
