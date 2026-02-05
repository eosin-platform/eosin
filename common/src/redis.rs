use crate::args::RedisArgs;
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use deadpool_redis::{Config as RedisPoolConfig, Pool};
use owo_colors::OwoColorize;
use redis::{AsyncCommands, Client, aio::PubSub};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

pub async fn init_redis(args: &crate::args::RedisArgs) -> Pool {
    println!(
        "{}{}",
        "🔌 Connecting to Redis • url=".green(),
        args.url_redacted().green().dimmed(),
    );
    let pool = RedisPoolConfig::from_url(args.url())
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool");
    pool.get()
        .await
        .expect("Failed to connect to Redis")
        .ping::<String>()
        .await
        .expect("Failed to ping Redis");
    pool
}

pub async fn init_pubsub(args: &crate::args::RedisArgs) -> PubSub {
    Client::open(args.url())
        .expect("Failed to create Redis client")
        .get_async_pubsub()
        .await
        .expect("Failed to create Redis PubSub")
}

pub async fn listen_for_work(
    cancel: CancellationToken,
    redis_args: RedisArgs,
    tx: tokio::sync::broadcast::Sender<Bytes>,
    topic: &str,
) -> Result<()> {
    loop {
        if cancel.is_cancelled() {
            bail!("Context cancelled");
        }
        let mut pubsub = init_pubsub(&redis_args).await;
        pubsub
            .subscribe(topic)
            .await
            .context("Failed to subscribe to imagegen work topic")?;
        let mut messages = pubsub.on_message();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => bail!("Context cancelled"),
                value = messages.next() => match value {
                    None => break,
                    Some(msg) => {
                        let value = msg.get_payload::<Vec<u8>>()?.into();
                        tx.send(value).ok();
                    }
                }
            }
        }
    }
}
