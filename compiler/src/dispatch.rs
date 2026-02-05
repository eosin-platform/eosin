use anyhow::{Context, Result};
use async_nats::jetstream;
use histion_common::streams::{ProcessSlideEvent, topics::PROCESS_SLIDE};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::args::DispatchArgs;
use crate::db::{self, DispatchResult};
use crate::s3;

/// Get current time in milliseconds since Unix epoch.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_millis() as i64
}

/// Run the dispatch command.
pub async fn run_dispatch(args: DispatchArgs) -> Result<()> {
    tracing::info!(
        bucket = %args.s3.bucket,
        prefix = %args.s3.path_prefix,
        "starting dispatch job"
    );

    // Create S3 client
    let s3_client = s3::create_s3_client(&args.s3).await?;
    tracing::info!("connected to S3");

    // Create Postgres pool
    let pg_pool = histion_common::postgres::create_pool(args.postgres.clone()).await;
    tracing::info!("connected to Postgres");

    // Initialize the database schema
    db::init_schema(&pg_pool).await?;

    // Connect to NATS
    let nats = args.nats.connect().await?;
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Create JetStream context
    let jetstream = jetstream::new(nats);

    // Ensure the stream exists
    let _stream = jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: args.stream_name.clone(),
            subjects: vec![PROCESS_SLIDE.to_string()],
            ..Default::default()
        })
        .await
        .context("failed to get or create stream")?;
    tracing::info!(stream = %args.stream_name, "connected to stream");

    // List all TIF files
    let tif_files = s3::list_tif_files(&s3_client, &args.s3.bucket, &args.s3.path_prefix).await?;
    tracing::info!(count = tif_files.len(), "found TIF files");

    let mut dispatched_count = 0;
    let mut skipped_count = 0;
    let mut failed_count = 0;

    for key in tif_files {
        let current_time = now_ms();

        // Create the event payload
        let event = ProcessSlideEvent { key: key.clone() };
        let payload = serde_json::to_vec(&event).context("failed to serialize event")?;

        // Clone jetstream for the closure
        let js = jetstream.clone();
        let payload_bytes: bytes::Bytes = payload.into();

        // Try to dispatch with publish callback
        let result = db::try_dispatch_with_publish(&pg_pool, &key, current_time, || {
            let js = js.clone();
            let payload = payload_bytes.clone();
            async move {
                let ack = js
                    .publish(PROCESS_SLIDE, payload)
                    .await
                    .context("failed to publish event")?;

                // Wait for acknowledgment
                ack.await.context("failed to get publish ack")?;
                Ok(())
            }
        })
        .await?;

        match result {
            DispatchResult::Dispatched => {
                tracing::info!(key = %key, "dispatched");
                dispatched_count += 1;
            }
            DispatchResult::AlreadyDispatched => {
                tracing::debug!(key = %key, "already dispatched, skipping");
                skipped_count += 1;
            }
            DispatchResult::PublishFailed => {
                tracing::error!(key = %key, "publish failed");
                failed_count += 1;
            }
        }
    }

    tracing::info!(
        dispatched = dispatched_count,
        skipped = skipped_count,
        failed = failed_count,
        "dispatch job complete"
    );

    Ok(())
}
