use anyhow::{Context, Result};
use async_nats::jetstream::{self, consumer::PullConsumer};
use futures::StreamExt;
use histion_common::shutdown::shutdown_signal;
use histion_common::streams::{ProcessSlideEvent, topics::PROCESS_SLIDE};
use histion_storage::StorageClient;
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::args::ProcessArgs;
use crate::meta_client::MetaClient;
use crate::s3;
use crate::tiler;

/// Run the process worker.
pub async fn run_process(args: ProcessArgs) -> Result<()> {
    tracing::info!(
        bucket = %args.s3.bucket,
        prefix = %args.s3.path_prefix,
        download_dir = %args.download_dir,
        meta_endpoint = %args.meta_endpoint,
        storage_endpoint = %args.storage_endpoint,
        "starting process worker"
    );

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
) -> Result<()> {
    let event: ProcessSlideEvent =
        serde_json::from_slice(payload).context("failed to deserialize ProcessSlideEvent")?;

    tracing::info!(key = %event.key, "processing slide");

    // Download the TIF file
    let local_path = s3::download_file(s3_client, bucket, &event.key, download_dir).await?;
    tracing::info!(key = %event.key, path = %local_path, "slide downloaded");

    // Process the slide, ensuring cleanup happens regardless of success/failure
    let result =
        process_downloaded_slide(&local_path, &event.key, meta_client, storage_client).await;

    // Always delete the local file to free up space
    if let Err(e) = tokio::fs::remove_file(&local_path).await {
        tracing::warn!(path = %local_path, error = ?e, "failed to delete local file");
    } else {
        tracing::debug!(path = %local_path, "deleted local file");
    }

    result
}

/// Process a downloaded slide file: extract metadata, insert into meta service, tile and upload.
async fn process_downloaded_slide(
    local_path: &str,
    key: &str,
    meta_client: &MetaClient,
    storage_client: &mut StorageClient,
) -> Result<()> {
    let path = Path::new(local_path);

    // Get slide metadata first
    let metadata = tiler::get_slide_metadata(path).context("failed to extract slide metadata")?;

    tracing::info!(
        key = %key,
        width = metadata.width,
        height = metadata.height,
        levels = metadata.level_count,
        "extracted slide metadata"
    );

    // Insert metadata into meta service
    let slide = meta_client
        .create_slide(metadata.width, metadata.height, key)
        .await
        .context("failed to create slide in meta service")?;

    tracing::info!(
        key = %key,
        slide_id = %slide.id,
        "slide metadata inserted"
    );

    // Process the slide: extract tiles and upload to storage
    tiler::process_slide(path, slide.id, storage_client)
        .await
        .context("failed to process slide tiles")?;

    tracing::info!(
        key = %key,
        slide_id = %slide.id,
        "slide processing complete"
    );

    Ok(())
}
