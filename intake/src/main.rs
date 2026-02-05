use anyhow::Result;
use async_nats::jetstream::{self, consumer::PullConsumer};
use clap::Parser;
use futures::StreamExt;
use histion_common::streams::{CacheMissEvent, topics::CACHE_MISS};

mod args;

use args::{Cli, Commands, ConsumerArgs};

/// Handle a cache miss event.
///
/// This is a stub that should be implemented to process cache miss events,
/// such as fetching the missing tile from cold storage and caching it.
async fn handle_cache_miss(event: CacheMissEvent) -> Result<()> {
    tracing::info!(
        id = %event.id,
        x = event.x,
        y = event.y,
        level = event.level,
        "received cache miss event"
    );

    // TODO: Implement cache miss handling logic
    // - Fetch tile from cold storage (S3)
    // - Store in warm cache
    // - Optionally notify requesters

    Ok(())
}

async fn run_consumer(args: ConsumerArgs) -> Result<()> {
    // Connect to NATS
    let nats = args.nats.connect().await?;
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Create JetStream context
    let jetstream = jetstream::new(nats);

    // Get or create the stream
    let stream = jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: args.stream_name.clone(),
            subjects: vec![CACHE_MISS.to_string()],
            ..Default::default()
        })
        .await?;
    tracing::info!(stream = %args.stream_name, "connected to stream");

    // Create or get a durable pull consumer
    let consumer: PullConsumer = stream
        .get_or_create_consumer(
            &args.consumer_name,
            jetstream::consumer::pull::Config {
                durable_name: Some(args.consumer_name.clone()),
                filter_subject: CACHE_MISS.to_string(),
                ..Default::default()
            },
        )
        .await?;
    tracing::info!(consumer = %args.consumer_name, "consumer ready");

    // Process messages
    let mut messages = consumer.messages().await?;
    tracing::info!("listening for cache miss events");

    while let Some(msg) = messages.next().await {
        match msg {
            Ok(message) => {
                // Parse the cache miss event
                match serde_json::from_slice::<CacheMissEvent>(&message.payload) {
                    Ok(event) => {
                        if let Err(e) = handle_cache_miss(event).await {
                            tracing::error!(?e, "failed to handle cache miss");
                            // Don't ack on error - message will be redelivered
                            continue;
                        }
                    }
                    Err(e) => {
                        tracing::error!(?e, "failed to parse cache miss event");
                    }
                }

                // Acknowledge the message
                if let Err(e) = message.ack().await {
                    tracing::error!(?e, "failed to ack message");
                }
            }
            Err(e) => {
                tracing::error!(?e, "error receiving message");
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Consumer(args) => run_consumer(args).await,
    }
}
