use anyhow::{Context, Result, anyhow};
use async_nats::Subscriber;
use bytes::Bytes;
use owo_colors::OwoColorize;
use std::{collections::HashMap, ops::Deref, sync::Arc};
use tokio::sync::{Mutex, broadcast};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

async fn subscribe(subject: String, nats: async_nats::Client) -> Result<Subscriber> {
    nats.subscribe(subject)
        .await
        .context("Failed to subscribe to channel gen responses")
}

struct DirectWaitSubscription {
    sender: broadcast::Sender<BroadcastResult>,
    handle: Option<tokio::task::JoinHandle<()>>,
    cancel: CancellationToken,
}

pub struct DirectWaitRegistryInner {
    cancel: CancellationToken,
    nats: async_nats::Client,
    inner: Arc<Mutex<HashMap<String, DirectWaitSubscription>>>,
    redis: deadpool_redis::Pool,
}

#[derive(Clone)]
pub struct DirectWaitRegistry {
    inner: Arc<DirectWaitRegistryInner>,
}

impl Deref for DirectWaitRegistry {
    type Target = DirectWaitRegistryInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct WaitSubscription {
    pub cancel: CancellationToken,
    pub receiver: broadcast::Receiver<BroadcastResult>,
    pub first: bool,
}

async fn wait_for_reply(
    cancel: CancellationToken,
    mut subscriber: Subscriber,
    tx: broadcast::Sender<BroadcastResult>,
) {
    tokio::select! {
        _ = cancel.cancelled() => {
            let _ = tx.send(BroadcastResult::from_err("Context cancelled".to_string()));
        },
        msg = subscriber.next() => match msg {
            None => {
                eprintln!("{}", "🛑 NATS subscription closed before reply was received".red());
                let _ = tx.send(BroadcastResult::from_err("NATS subscription closed".to_string()));
            },
            Some(msg) => {
                let _ = tx.send(BroadcastResult::from_value(msg.payload));
            }
        }
    }
}

#[derive(Clone)]
pub struct BroadcastResult(Result<Bytes, String>);

impl Deref for BroadcastResult {
    type Target = Result<Bytes, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BroadcastResult {
    pub fn from_err(e: String) -> Self {
        BroadcastResult(Err(e))
    }

    pub fn from_value(res: Bytes) -> Self {
        BroadcastResult(Ok(res))
    }

    pub fn inner(self) -> Result<Bytes> {
        self.0.map_err(|e| anyhow!(e))
    }
}

impl From<BroadcastResult> for Result<Bytes> {
    fn from(value: BroadcastResult) -> Result<Bytes> {
        match value.0 {
            Ok(bytes) => Ok(bytes),
            Err(err) => Err(anyhow!(err)),
        }
    }
}

impl DirectWaitRegistry {
    pub fn new(
        nats: async_nats::Client,
        redis: deadpool_redis::Pool,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(DirectWaitRegistryInner {
                nats,
                cancel,
                redis,
                inner: Arc::new(Mutex::new(HashMap::new())),
            }),
        })
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let mut lock = self.inner.inner.lock().await;
        for (_subject, sub) in lock.iter_mut() {
            let handle = sub.handle.take().unwrap();
            handle.abort();
            let _ = handle.await;
        }
        lock.clear();
    }

    pub async fn not_dispatched(&self, subject: &str) -> Result<bool> {
        let key = rk_dispatched(subject);
        let mut conn = self
            .redis
            .get()
            .await
            .context("Failed to get Redis connection")?;
        // Redis: SET key 1 EX 600 NX
        // Reply: "OK" if it set, nil if key already exists
        let set: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(1)
            .arg("EX")
            .arg(600) // max job duration: 10 minutes
            .arg("NX")
            .query_async(&mut conn)
            .await
            .context("Failed to set dispatched key in Redis")?;
        Ok(set.is_some()) // true => not previously dispatched; false => already dispatched
    }

    /// Returns a receiver; if this is the first waiter, also returns `true`.
    pub async fn register_waiter(&self, subject: String) -> Result<WaitSubscription> {
        let mut map = self.inner.inner.lock().await;
        if let Some(sub) = map.get(&subject) {
            Ok(WaitSubscription {
                cancel: sub.cancel.clone(),
                receiver: sub.sender.subscribe(),
                first: false,
            })
        } else {
            let subscriber = subscribe(subject.clone(), self.nats.clone())
                .await
                .context("Failed to subscribe to NATS")?;
            let (tx, rx): (
                broadcast::Sender<BroadcastResult>,
                broadcast::Receiver<BroadcastResult>,
            ) = broadcast::channel(16);
            let cancel = self.cancel.child_token();
            let tx_clone = tx.clone();
            let cancel_clone = cancel.clone();
            let inner_clone = self.inner.clone();
            let subject_clone = subject.to_string();
            let handle = tokio::spawn(async move {
                let _ = wait_for_reply(cancel_clone.clone(), subscriber, tx_clone).await;
                cancel_clone.cancel();
                inner_clone.inner.lock().await.remove(&subject_clone); // clean up
            });
            let wait_sub = DirectWaitSubscription {
                sender: tx.clone(),
                handle: Some(handle),
                cancel: cancel.clone(),
            };
            let first = self
                .not_dispatched(&subject)
                .await
                .context("Failed to check if subject was already dispatched in Redis")?;
            map.insert(subject, wait_sub);
            let sub = WaitSubscription {
                cancel,
                receiver: rx,
                first,
            };
            let _ = sub.cancel;
            Ok(sub)
        }
    }
}

fn rk_dispatched(subject: &str) -> String {
    format!("wait_registry:dispatched:{}", subject)
}
