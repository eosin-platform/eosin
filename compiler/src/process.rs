use anyhow::{Context, Result};
use async_nats::jetstream::{self, consumer::PullConsumer};
use futures::StreamExt;
use histion_common::shutdown::shutdown_signal;
use histion_common::streams::{ProcessSlideEvent, topics::PROCESS_SLIDE};
use tokio_util::sync::CancellationToken;

use crate::args::ProcessArgs;
use crate::s3;

/// Run the process worker.
pub async fn run_process(args: ProcessArgs) -> Result<()> {
    tracing::info!(
        bucket = %args.s3.bucket,
        prefix = %args.s3.path_prefix,
        download_dir = %args.download_dir,
        "starting process worker"
    );

    // Create S3 client
    let s3_client = s3::create_s3_client(&args.s3).await?;
    tracing::info!("connected to S3");

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

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("shutdown signal received, stopping worker");
                break;
            }
            msg = messages.next() => {
                match msg {
                    Some(Ok(message)) => {
                        if let Err(e) = handle_process_slide(
                            &message.payload,
                            &s3_client,
                            &args.s3.bucket,
                            &args.download_dir,
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

/// Handle a ProcessSlideEvent by downloading the TIF file.
async fn handle_process_slide(
    payload: &[u8],
    s3_client: &aws_sdk_s3::Client,
    bucket: &str,
    download_dir: &str,
) -> Result<()> {
    let event: ProcessSlideEvent =
        serde_json::from_slice(payload).context("failed to deserialize ProcessSlideEvent")?;

    tracing::info!(key = %event.key, "processing slide");

    // Download the TIF file
    let local_path = s3::download_file(s3_client, bucket, &event.key, download_dir).await?;

    tracing::info!(key = %event.key, path = %local_path, "slide downloaded successfully");

    // TODO: Additional processing can be added here
    // - Parse the TIF file
    // - Extract tiles
    // - Insert into storage backend

    Ok(())
}
