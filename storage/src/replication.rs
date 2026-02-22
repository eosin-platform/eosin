use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_stream::iter;
use tonic::Status;
use uuid::Uuid;

use crate::proto::cluster::{
    BecomeMasterRequest, BecomeMasterResponse, BecomeReplicaRequest, BecomeReplicaResponse,
    ClusterRoutingConfig, FullSnapshot, GetShardStatusRequest, GetShardStatusResponse, Heartbeat,
    LogBatch, MigrateTileRequest, MigrateTileResponse, Role, SnapshotEntry, SyncEvent, SyncReject,
    SyncRequest, TileMutation, UpdateRoutingConfigRequest, UpdateRoutingConfigResponse,
    control_service_server::ControlService, replication_service_client::ReplicationServiceClient,
    replication_service_server::ReplicationService,
};

pub const NUM_SLOTS: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShardRole {
    Master,
    ReadReplica,
}

impl ShardRole {
    pub fn as_proto(self) -> i32 {
        match self {
            ShardRole::Master => Role::Master as i32,
            ShardRole::ReadReplica => Role::ReadReplica as i32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TileWrite {
    pub id: [u8; 16],
    pub x: u32,
    pub y: u32,
    pub level: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub offset: u64,
    pub write: TileWrite,
}

#[derive(Clone, Debug, Eq)]
struct TileKey {
    id: [u8; 16],
    x: u32,
    y: u32,
    level: u32,
}

impl PartialEq for TileKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.x == other.x && self.y == other.y && self.level == other.level
    }
}

impl Hash for TileKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.x.hash(state);
        self.y.hash(state);
        self.level.hash(state);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutingTable {
    pub config_epoch: u64,
    pub slot_to_shard: Vec<u32>,
    pub shard_masters: HashMap<u32, String>,
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self {
            config_epoch: 0,
            slot_to_shard: vec![0; NUM_SLOTS],
            shard_masters: HashMap::new(),
        }
    }
}

impl RoutingTable {
    #[allow(dead_code)]
    pub fn owner_for_slot(&self, slot: u16, local_shard_id: u32) -> u32 {
        self.slot_to_shard
            .get(slot as usize)
            .copied()
            .unwrap_or(local_shard_id)
    }

    pub fn owner_for_tile(&self, tile: &TileWrite, local_shard_id: u32) -> u32 {
        self.owner_for_slot(tile_slot(tile), local_shard_id)
    }

    pub fn from_proto(proto: ClusterRoutingConfig) -> Result<Self, Status> {
        if proto.slot_to_shard.len() != NUM_SLOTS {
            return Err(Status::invalid_argument(format!(
                "slot_to_shard must contain {NUM_SLOTS} entries"
            )));
        }
        Ok(Self {
            config_epoch: proto.config_epoch,
            slot_to_shard: proto.slot_to_shard,
            shard_masters: proto.shard_masters,
        })
    }

    #[allow(dead_code)]
    pub fn to_proto(&self) -> ClusterRoutingConfig {
        ClusterRoutingConfig {
            config_epoch: self.config_epoch,
            slot_to_shard: self.slot_to_shard.clone(),
            shard_masters: self.shard_masters.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ShardEngine {
    inner: Arc<ShardEngineInner>,
}

struct ShardEngineInner {
    data_root: PathBuf,
    shard_id: u32,
    backlog_capacity: usize,
    runtime: RwLock<ShardRuntime>,
    replica_worker: Mutex<Option<JoinHandle<()>>>,
    migration_worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Debug)]
struct MigrationTask {
    tile: TileWrite,
    attempts: u32,
}

#[derive(Clone, Debug)]
struct ShardRuntime {
    role: ShardRole,
    epoch: u64,
    current_offset: u64,
    applied_offset: u64,
    known_master_offset: u64,
    backlog: VecDeque<LogEntry>,
    snapshot: HashMap<TileKey, Vec<u8>>,
    master_addr: Option<String>,
    last_heartbeat: Option<SystemTime>,
    routing: RoutingTable,
    migration_queue: VecDeque<MigrationTask>,
    misplaced_tiles: u64,
}

impl ShardEngine {
    pub fn new(data_root: impl Into<PathBuf>, shard_id: u32, backlog_capacity: usize) -> Self {
        let data_root = data_root.into();
        let routing = load_routing_config(&data_root).unwrap_or_default();

        Self {
            inner: Arc::new(ShardEngineInner {
                data_root,
                shard_id,
                backlog_capacity: backlog_capacity.max(1),
                runtime: RwLock::new(ShardRuntime {
                    role: ShardRole::ReadReplica,
                    epoch: 0,
                    current_offset: 0,
                    applied_offset: 0,
                    known_master_offset: 0,
                    backlog: VecDeque::new(),
                    snapshot: HashMap::new(),
                    master_addr: None,
                    last_heartbeat: None,
                    routing,
                    migration_queue: VecDeque::new(),
                    misplaced_tiles: 0,
                }),
                replica_worker: Mutex::new(None),
                migration_worker: Mutex::new(None),
            }),
        }
    }

    pub fn tile_path(&self, id: &Uuid, level: u32, x: u32, y: u32) -> PathBuf {
        self.inner
            .data_root
            .join(id.to_string())
            .join(level.to_string())
            .join(format!("{}_{}.webp", x, y))
    }

    #[allow(dead_code)]
    pub fn routing_config_path(&self) -> PathBuf {
        routing_config_path(&self.inner.data_root)
    }

    pub async fn read_tile(
        &self,
        id: &Uuid,
        level: u32,
        x: u32,
        y: u32,
    ) -> Result<Vec<u8>, std::io::Error> {
        fs::read(self.tile_path(id, level, x, y)).await
    }

    pub async fn write_as_master(&self, write: TileWrite) -> Result<u64, Status> {
        {
            let rt = self.inner.runtime.read().await;
            if rt.role != ShardRole::Master {
                return Err(Status::failed_precondition(
                    "writes are rejected on read replicas",
                ));
            }
            if rt.epoch == 0 {
                return Err(Status::failed_precondition(
                    "writes are fenced until master epoch is assigned",
                ));
            }
            let owner = rt.routing.owner_for_tile(&write, self.inner.shard_id);
            if owner != self.inner.shard_id {
                return Err(Status::failed_precondition(format!(
                    "tile is routed to shard {owner} under config_epoch {}",
                    rt.routing.config_epoch
                )));
            }
        }

        self.persist_write(&write).await.map_err(|e| {
            tracing::error!(?e, "failed to persist tile write");
            Status::internal("failed to persist tile")
        })?;

        let mut rt = self.inner.runtime.write().await;
        rt.current_offset = rt.current_offset.saturating_add(1);
        rt.applied_offset = rt.current_offset;
        let offset = rt.current_offset;
        let key = TileKey {
            id: write.id,
            x: write.x,
            y: write.y,
            level: write.level,
        };
        rt.snapshot.insert(key, write.data.clone());
        rt.backlog.push_back(LogEntry {
            offset,
            write: write.clone(),
        });
        while rt.backlog.len() > self.inner.backlog_capacity {
            rt.backlog.pop_front();
        }
        Ok(offset)
    }

    async fn persist_write(&self, write: &TileWrite) -> Result<(), std::io::Error> {
        let id = Uuid::from_bytes(write.id);
        let path = self.tile_path(&id, write.level, write.x, write.y);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let temp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
        let result = async {
            fs::write(&temp_path, &write.data).await?;
            fs::rename(&temp_path, &path).await
        }
        .await;
        let _ = std::fs::remove_file(&temp_path);
        result
    }

    async fn apply_replication_write(&self, entry: TileMutation) -> Result<(), Status> {
        if entry.id.len() != 16 {
            return Err(Status::invalid_argument("replication entry UUID must be 16 bytes"));
        }
        let mut id = [0_u8; 16];
        id.copy_from_slice(&entry.id);
        let write = TileWrite {
            id,
            x: entry.x,
            y: entry.y,
            level: entry.level,
            data: entry.data,
        };
        self.persist_write(&write).await.map_err(|e| {
            tracing::error!(?e, "failed to apply replicated tile");
            Status::internal("failed to apply replicated write")
        })?;

        let mut rt = self.inner.runtime.write().await;
        let offset = entry.offset.max(rt.applied_offset);
        rt.applied_offset = offset;
        rt.current_offset = rt.current_offset.max(offset);
        rt.known_master_offset = rt.known_master_offset.max(entry.offset);
        rt.last_heartbeat = Some(SystemTime::now());
        rt.snapshot.insert(
            TileKey {
                id: write.id,
                x: write.x,
                y: write.y,
                level: write.level,
            },
            write.data,
        );
        Ok(())
    }

    async fn apply_full_snapshot(&self, snapshot: FullSnapshot) -> Result<(), Status> {
        let entries = snapshot.entries;
        for entry in &entries {
            if entry.id.len() != 16 {
                return Err(Status::invalid_argument("snapshot entry UUID must be 16 bytes"));
            }
            let mut id = [0_u8; 16];
            id.copy_from_slice(&entry.id);
            let write = TileWrite {
                id,
                x: entry.x,
                y: entry.y,
                level: entry.level,
                data: entry.data.clone(),
            };
            self.persist_write(&write).await.map_err(|e| {
                tracing::error!(?e, "failed to persist snapshot tile");
                Status::internal("failed to apply snapshot")
            })?;
        }

        let mut rt = self.inner.runtime.write().await;
        rt.snapshot.clear();
        for entry in entries {
            let mut id = [0_u8; 16];
            id.copy_from_slice(&entry.id);
            rt.snapshot.insert(
                TileKey {
                    id,
                    x: entry.x,
                    y: entry.y,
                    level: entry.level,
                },
                entry.data,
            );
        }
        rt.applied_offset = snapshot.snapshot_offset;
        rt.current_offset = snapshot.snapshot_offset;
        rt.known_master_offset = snapshot.snapshot_offset;
        rt.last_heartbeat = Some(SystemTime::now());
        Ok(())
    }

    pub async fn become_master(
        &self,
        req: BecomeMasterRequest,
    ) -> Result<BecomeMasterResponse, Status> {
        if req.shard_id != self.inner.shard_id {
            return Err(Status::invalid_argument("shard_id mismatch"));
        }
        if req.epoch == 0 {
            return Err(Status::invalid_argument("epoch must be > 0"));
        }

        self.stop_replica_worker().await;

        let mut rt = self.inner.runtime.write().await;
        if req.epoch < rt.epoch {
            return Ok(BecomeMasterResponse {
                accepted: false,
                message: format!("stale epoch {}; current epoch {}", req.epoch, rt.epoch),
            });
        }
        rt.role = ShardRole::Master;
        rt.epoch = req.epoch;
        rt.master_addr = None;
        drop(rt);
        self.ensure_migration_worker().await;
        Ok(BecomeMasterResponse {
            accepted: true,
            message: "promoted to master".to_string(),
        })
    }

    pub async fn become_replica(
        self,
        req: BecomeReplicaRequest,
    ) -> Result<BecomeReplicaResponse, Status> {
        if req.shard_id != self.inner.shard_id {
            return Err(Status::invalid_argument("shard_id mismatch"));
        }
        if req.epoch == 0 {
            return Err(Status::invalid_argument("epoch must be > 0"));
        }
        if req.master_addr.is_empty() {
            return Err(Status::invalid_argument("master_addr is required"));
        }

        self.stop_replica_worker().await;
        self.stop_migration_worker().await;

        {
            let mut rt = self.inner.runtime.write().await;
            if req.epoch < rt.epoch {
                return Ok(BecomeReplicaResponse {
                    accepted: false,
                    message: format!("stale epoch {}; current epoch {}", req.epoch, rt.epoch),
                });
            }
            rt.role = ShardRole::ReadReplica;
            rt.epoch = req.epoch;
            rt.master_addr = Some(req.master_addr.clone());
            rt.migration_queue.clear();
        }

        self.start_replica_worker(req.master_addr).await;

        Ok(BecomeReplicaResponse {
            accepted: true,
            message: "configured as read replica".to_string(),
        })
    }

    pub async fn install_routing_config(
        &self,
        config: ClusterRoutingConfig,
    ) -> Result<UpdateRoutingConfigResponse, Status> {
        let new_routing = RoutingTable::from_proto(config)?;
        let mut rt = self.inner.runtime.write().await;
        if new_routing.config_epoch <= rt.routing.config_epoch {
            return Ok(UpdateRoutingConfigResponse {
                accepted: false,
                message: format!(
                    "stale config epoch {}; current config epoch {}",
                    new_routing.config_epoch, rt.routing.config_epoch
                ),
            });
        }
        rt.routing = new_routing.clone();
        rt.migration_queue.clear();
        rt.misplaced_tiles = 0;
        drop(rt);

        persist_routing_config(&self.inner.data_root, &new_routing)
            .await
            .map_err(|e| Status::internal(format!("failed to persist routing config: {e}")))?;

        self.ensure_migration_worker().await;

        Ok(UpdateRoutingConfigResponse {
            accepted: true,
            message: "routing updated".to_string(),
        })
    }

    async fn stop_replica_worker(&self) {
        let mut worker = self.inner.replica_worker.lock().await;
        if let Some(task) = worker.take() {
            task.abort();
        }
    }

    async fn start_replica_worker(&self, master_addr: String) {
        let this = self.clone();
        let task = tokio::spawn(async move {
            this.replica_loop(master_addr).await;
        });
        let mut worker = self.inner.replica_worker.lock().await;
        *worker = Some(task);
    }

    async fn stop_migration_worker(&self) {
        let mut worker = self.inner.migration_worker.lock().await;
        if let Some(task) = worker.take() {
            task.abort();
        }
    }

    async fn ensure_migration_worker(&self) {
        let role = { self.inner.runtime.read().await.role };
        if role != ShardRole::Master {
            return;
        }

        let mut worker = self.inner.migration_worker.lock().await;
        if worker.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        let this = self.clone();
        *worker = Some(tokio::spawn(async move {
            this.migration_loop().await;
        }));
    }

    async fn replica_loop(&self, master_addr: String) {
        loop {
            let (epoch, last_offset, still_replica) = {
                let rt = self.inner.runtime.read().await;
                (rt.epoch, rt.applied_offset, rt.role == ShardRole::ReadReplica)
            };
            if !still_replica {
                break;
            }

            let target = format!("http://{}", master_addr);
            match ReplicationServiceClient::connect(target).await {
                Ok(mut client) => {
                    let req = SyncRequest {
                        shard_id: self.inner.shard_id,
                        epoch,
                        last_offset,
                    };
                    match client.sync(req).await {
                        Ok(response) => {
                            let mut stream = response.into_inner();
                            while let Ok(Some(event)) = stream.message().await {
                                match event.payload {
                                    Some(crate::proto::cluster::sync_event::Payload::Reject(_)) => {
                                        break;
                                    }
                                    Some(crate::proto::cluster::sync_event::Payload::FullSnapshot(snapshot)) => {
                                        let _ = self.apply_full_snapshot(snapshot).await;
                                    }
                                    Some(crate::proto::cluster::sync_event::Payload::LogBatch(batch)) => {
                                        for entry in batch.entries {
                                            let _ = self.apply_replication_write(entry).await;
                                        }
                                        let mut rt = self.inner.runtime.write().await;
                                        rt.known_master_offset =
                                            rt.known_master_offset.max(batch.current_offset);
                                        rt.last_heartbeat = Some(SystemTime::now());
                                    }
                                    Some(crate::proto::cluster::sync_event::Payload::Heartbeat(hb)) => {
                                        let mut rt = self.inner.runtime.write().await;
                                        rt.known_master_offset = hb.current_offset;
                                        rt.current_offset = rt.current_offset.max(hb.current_offset);
                                        rt.last_heartbeat = Some(SystemTime::now());
                                    }
                                    None => {}
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(?e, "replication sync request failed");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(?e, master_addr = %master_addr, "failed to connect to master for replication");
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn migration_loop(&self) {
        loop {
            let (is_master, queue_empty) = {
                let rt = self.inner.runtime.read().await;
                (rt.role == ShardRole::Master, rt.migration_queue.is_empty())
            };
            if !is_master {
                break;
            }

            if queue_empty {
                match self.scan_misplaced_tiles().await {
                    Ok(tasks) => {
                        let mut rt = self.inner.runtime.write().await;
                        rt.misplaced_tiles = tasks.len() as u64;
                        rt.migration_queue.extend(tasks);
                    }
                    Err(e) => {
                        tracing::warn!(?e, "failed to scan misplaced tiles");
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        continue;
                    }
                }
            }

            let task = {
                let mut rt = self.inner.runtime.write().await;
                rt.migration_queue.pop_front()
            };

            let Some(mut task) = task else {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            };

            match self.migrate_one(&task.tile).await {
                Ok(true) => {
                    let mut rt = self.inner.runtime.write().await;
                    rt.misplaced_tiles = rt.misplaced_tiles.saturating_sub(1);
                }
                Ok(false) | Err(_) => {
                    task.attempts = task.attempts.saturating_add(1);
                    let backoff_ms = 100_u64.saturating_mul((task.attempts as u64).min(20));
                    let mut rt = self.inner.runtime.write().await;
                    rt.migration_queue.push_back(task);
                    drop(rt);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
            }
        }
    }

    async fn scan_misplaced_tiles(&self) -> Result<Vec<MigrationTask>, std::io::Error> {
        let routing = { self.inner.runtime.read().await.routing.clone() };
        let tiles = collect_tiles(&self.inner.data_root).await?;
        let mut tasks = Vec::new();
        for tile in tiles {
            let owner = routing.owner_for_tile(&tile, self.inner.shard_id);
            if owner != self.inner.shard_id {
                tasks.push(MigrationTask { tile, attempts: 0 });
            }
        }
        Ok(tasks)
    }

    async fn migrate_one(&self, tile: &TileWrite) -> Result<bool, Status> {
        let (routing, role) = {
            let rt = self.inner.runtime.read().await;
            (rt.routing.clone(), rt.role)
        };
        if role != ShardRole::Master {
            return Ok(false);
        }

        let owner = routing.owner_for_tile(tile, self.inner.shard_id);
        if owner == self.inner.shard_id {
            return Ok(true);
        }
        let Some(master_addr) = routing.shard_masters.get(&owner).cloned() else {
            return Ok(false);
        };

        let id = Uuid::from_bytes(tile.id);
        let src_path = self.tile_path(&id, tile.level, tile.x, tile.y);
        let data = match fs::read(&src_path).await {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(e) => return Err(Status::internal(format!("failed to read source tile: {e}"))),
        };

        let mut client = ReplicationServiceClient::connect(format!("http://{master_addr}"))
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        let response = client
            .migrate_tile(MigrateTileRequest {
                source_shard_id: self.inner.shard_id,
                target_shard_id: owner,
                config_epoch: routing.config_epoch,
                id: tile.id.to_vec(),
                x: tile.x,
                y: tile.y,
                level: tile.level,
                data,
            })
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?
            .into_inner();

        if !response.accepted {
            return Ok(false);
        }

        fs::remove_file(src_path)
            .await
            .map_err(|e| Status::internal(format!("failed to delete migrated source tile: {e}")))?;
        Ok(true)
    }

    pub async fn sync_events(&self, req: SyncRequest) -> Result<Vec<SyncEvent>, Status> {
        let rt = self.inner.runtime.read().await;
        if req.shard_id != self.inner.shard_id {
            return Ok(vec![SyncEvent {
                payload: Some(crate::proto::cluster::sync_event::Payload::Reject(SyncReject {
                    reason: "shard_id mismatch".to_string(),
                })),
            }]);
        }
        if rt.role != ShardRole::Master {
            return Ok(vec![SyncEvent {
                payload: Some(crate::proto::cluster::sync_event::Payload::Reject(SyncReject {
                    reason: "node is not master".to_string(),
                })),
            }]);
        }
        if req.epoch != rt.epoch {
            return Ok(vec![SyncEvent {
                payload: Some(crate::proto::cluster::sync_event::Payload::Reject(SyncReject {
                    reason: format!("epoch mismatch requested={} current={}", req.epoch, rt.epoch),
                })),
            }]);
        }

        let mut events = Vec::new();
        let backlog_start = rt
            .backlog
            .front()
            .map(|x| x.offset)
            .unwrap_or(rt.current_offset.saturating_add(1));
        if req.last_offset < rt.current_offset {
            if req.last_offset.saturating_add(1) < backlog_start {
                let snapshot_entries = rt
                    .snapshot
                    .iter()
                    .map(|(key, data)| SnapshotEntry {
                        id: key.id.to_vec(),
                        x: key.x,
                        y: key.y,
                        level: key.level,
                        data: data.clone(),
                    })
                    .collect();
                events.push(SyncEvent {
                    payload: Some(crate::proto::cluster::sync_event::Payload::FullSnapshot(
                        FullSnapshot {
                            snapshot_offset: rt.current_offset,
                            entries: snapshot_entries,
                        },
                    )),
                });
            } else {
                let mut batch = Vec::new();
                for entry in rt.backlog.iter().filter(|e| e.offset > req.last_offset) {
                    batch.push(TileMutation {
                        id: entry.write.id.to_vec(),
                        x: entry.write.x,
                        y: entry.write.y,
                        level: entry.write.level,
                        data: entry.write.data.clone(),
                        offset: entry.offset,
                    });
                    if batch.len() >= 128 {
                        events.push(SyncEvent {
                            payload: Some(crate::proto::cluster::sync_event::Payload::LogBatch(
                                LogBatch {
                                    entries: std::mem::take(&mut batch),
                                    current_offset: rt.current_offset,
                                },
                            )),
                        });
                    }
                }
                if !batch.is_empty() {
                    events.push(SyncEvent {
                        payload: Some(crate::proto::cluster::sync_event::Payload::LogBatch(
                            LogBatch {
                                entries: batch,
                                current_offset: rt.current_offset,
                            },
                        )),
                    });
                }
            }
        }

        events.push(SyncEvent {
            payload: Some(crate::proto::cluster::sync_event::Payload::Heartbeat(
                Heartbeat {
                    current_offset: rt.current_offset,
                    epoch: rt.epoch,
                },
            )),
        });
        Ok(events)
    }

    async fn apply_migrated_tile(
        &self,
        req: MigrateTileRequest,
    ) -> Result<MigrateTileResponse, Status> {
        if req.id.len() != 16 {
            return Err(Status::invalid_argument("migration tile UUID must be 16 bytes"));
        }

        let mut id = [0_u8; 16];
        id.copy_from_slice(&req.id);
        let write = TileWrite {
            id,
            x: req.x,
            y: req.y,
            level: req.level,
            data: req.data,
        };

        let (role, routing) = {
            let rt = self.inner.runtime.read().await;
            (rt.role, rt.routing.clone())
        };
        if role != ShardRole::Master {
            return Ok(MigrateTileResponse {
                accepted: false,
                message: "target is not master".to_string(),
            });
        }
        let owner = routing.owner_for_tile(&write, self.inner.shard_id);
        if owner != self.inner.shard_id {
            return Ok(MigrateTileResponse {
                accepted: false,
                message: "tile is not routed to this shard".to_string(),
            });
        }
        if req.config_epoch < routing.config_epoch {
            return Ok(MigrateTileResponse {
                accepted: false,
                message: "stale migration config epoch".to_string(),
            });
        }

        self.persist_write(&write)
            .await
            .map_err(|e| Status::internal(format!("failed to persist migrated tile: {e}")))?;

        Ok(MigrateTileResponse {
            accepted: true,
            message: "ok".to_string(),
        })
    }

    pub async fn shard_status(
        &self,
        req: GetShardStatusRequest,
    ) -> Result<GetShardStatusResponse, Status> {
        if req.shard_id != self.inner.shard_id {
            return Err(Status::invalid_argument("shard_id mismatch"));
        }
        let rt = self.inner.runtime.read().await;
        let last_heartbeat_unix_ms = rt
            .last_heartbeat
            .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
            .map(|x| x.as_millis() as u64)
            .unwrap_or(0);
        let lag = rt.known_master_offset.saturating_sub(rt.applied_offset);
        Ok(GetShardStatusResponse {
            shard_id: self.inner.shard_id,
            role: rt.role.as_proto(),
            epoch: rt.epoch,
            applied_offset: rt.applied_offset,
            current_offset: rt.current_offset,
            last_heartbeat_unix_ms,
            replication_lag: lag,
            ready: true,
            master_addr: rt.master_addr.clone().unwrap_or_default(),
            config_epoch: rt.routing.config_epoch,
            migration_queue_len: rt.migration_queue.len() as u64,
            misplaced_tiles: rt.misplaced_tiles,
        })
    }
}

#[derive(Clone)]
pub struct ControlServiceImpl {
    pub shard: ShardEngine,
}

#[tonic::async_trait]
impl ControlService for ControlServiceImpl {
    async fn become_master(
        &self,
        request: tonic::Request<BecomeMasterRequest>,
    ) -> Result<tonic::Response<BecomeMasterResponse>, Status> {
        Ok(tonic::Response::new(
            self.shard.become_master(request.into_inner()).await?,
        ))
    }

    async fn become_replica(
        &self,
        request: tonic::Request<BecomeReplicaRequest>,
    ) -> Result<tonic::Response<BecomeReplicaResponse>, Status> {
        Ok(tonic::Response::new(
            self.shard.clone().become_replica(request.into_inner()).await?,
        ))
    }

    async fn update_routing_config(
        &self,
        request: tonic::Request<UpdateRoutingConfigRequest>,
    ) -> Result<tonic::Response<UpdateRoutingConfigResponse>, Status> {
        let Some(config) = request.into_inner().config else {
            return Err(Status::invalid_argument("config is required"));
        };
        Ok(tonic::Response::new(
            self.shard.install_routing_config(config).await?,
        ))
    }

    async fn get_shard_status(
        &self,
        request: tonic::Request<GetShardStatusRequest>,
    ) -> Result<tonic::Response<GetShardStatusResponse>, Status> {
        Ok(tonic::Response::new(
            self.shard.shard_status(request.into_inner()).await?,
        ))
    }
}

#[derive(Clone)]
pub struct ReplicationServiceImpl {
    pub shard: ShardEngine,
}

#[tonic::async_trait]
impl ReplicationService for ReplicationServiceImpl {
    type SyncStream = Pin<Box<dyn Stream<Item = Result<SyncEvent, Status>> + Send>>;

    async fn sync(
        &self,
        request: tonic::Request<SyncRequest>,
    ) -> Result<tonic::Response<Self::SyncStream>, Status> {
        let events = self.shard.sync_events(request.into_inner()).await?;
        let stream = iter(events.into_iter().map(Ok));
        Ok(tonic::Response::new(Box::pin(stream)))
    }

    async fn migrate_tile(
        &self,
        request: tonic::Request<MigrateTileRequest>,
    ) -> Result<tonic::Response<MigrateTileResponse>, Status> {
        Ok(tonic::Response::new(
            self.shard.apply_migrated_tile(request.into_inner()).await?,
        ))
    }
}

fn routing_config_path(data_root: &Path) -> PathBuf {
    data_root.join(".routing_config.json")
}

fn load_routing_config(data_root: &Path) -> Option<RoutingTable> {
    let path = routing_config_path(data_root);
    let body = std::fs::read(path).ok()?;
    serde_json::from_slice::<RoutingTable>(&body).ok()
}

async fn persist_routing_config(data_root: &Path, routing: &RoutingTable) -> Result<(), std::io::Error> {
    fs::create_dir_all(data_root).await?;
    let path = routing_config_path(data_root);
    fs::write(path, serde_json::to_vec(routing).expect("serialize routing"))
        .await
}

fn tile_slot(tile: &TileWrite) -> u16 {
    let mut bytes = Vec::with_capacity(16 + 12);
    bytes.extend_from_slice(&tile.id);
    bytes.extend_from_slice(&tile.x.to_le_bytes());
    bytes.extend_from_slice(&tile.y.to_le_bytes());
    bytes.extend_from_slice(&tile.level.to_le_bytes());
    crc16(&bytes) % NUM_SLOTS as u16
}

#[allow(dead_code)]
pub fn slot_for_tile_key(id: [u8; 16], x: u32, y: u32, level: u32) -> u16 {
    tile_slot(&TileWrite {
        id,
        x,
        y,
        level,
        data: Vec::new(),
    })
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

async fn collect_tiles(data_root: &Path) -> Result<Vec<TileWrite>, std::io::Error> {
    let mut out = Vec::new();
    let mut id_dir = match fs::read_dir(data_root).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };

    while let Some(entry) = id_dir.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id_name) = path.file_name().and_then(|x| x.to_str()) else {
            continue;
        };
        let Ok(id_uuid) = Uuid::parse_str(id_name) else {
            continue;
        };

        let mut level_dir = match fs::read_dir(&path).await {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        while let Some(level_entry) = level_dir.next_entry().await? {
            let level_path = level_entry.path();
            if !level_path.is_dir() {
                continue;
            }
            let Some(level_name) = level_path.file_name().and_then(|x| x.to_str()) else {
                continue;
            };
            let Ok(level) = level_name.parse::<u32>() else {
                continue;
            };

            let mut tiles = match fs::read_dir(&level_path).await {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            while let Some(tile_entry) = tiles.next_entry().await? {
                let tile_path = tile_entry.path();
                if !tile_path.is_file() {
                    continue;
                }
                let Some(name) = tile_path.file_name().and_then(|x| x.to_str()) else {
                    continue;
                };
                let Some((x, y)) = parse_tile_filename(name) else {
                    continue;
                };
                out.push(TileWrite {
                    id: *id_uuid.as_bytes(),
                    x,
                    y,
                    level,
                    data: Vec::new(),
                });
            }
        }
    }

    Ok(out)
}

fn parse_tile_filename(name: &str) -> Option<(u32, u32)> {
    let stem = name.strip_suffix(".webp")?;
    let (x, y) = stem.split_once('_')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::Instant;

    use tonic::transport::Server;

    use super::*;
    use crate::proto::cluster::replication_service_server::ReplicationServiceServer;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(id: [u8; 16], value: &str) -> TileWrite {
        TileWrite {
            id,
            x: 7,
            y: 9,
            level: 2,
            data: value.as_bytes().to_vec(),
        }
    }

    fn routing_with_owner(
        epoch: u64,
        slot: u16,
        owner: u32,
        masters: HashMap<u32, String>,
    ) -> ClusterRoutingConfig {
        let mut slots = vec![0_u32; NUM_SLOTS];
        slots[slot as usize] = owner;
        ClusterRoutingConfig {
            config_epoch: epoch,
            slot_to_shard: slots,
            shard_masters: masters,
        }
    }

    async fn wait_for(
        timeout: Duration,
        mut predicate: impl FnMut() -> bool,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    fn free_addr() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").to_string()
    }

    #[tokio::test]
    async fn routing_slot_is_deterministic() {
        let id = [42_u8; 16];
        let a = slot_for_tile_key(id, 1, 2, 3);
        let b = slot_for_tile_key(id, 1, 2, 3);
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn rejects_lower_or_equal_routing_epoch() {
        let dir = temp_dir();
        let engine = ShardEngine::new(dir.path(), 0, 16);
        let mut masters = HashMap::new();
        masters.insert(0, "127.0.0.1:1".to_string());
        let slot = 3_u16;

        let r1 = engine
            .install_routing_config(routing_with_owner(2, slot, 0, masters.clone()))
            .await
            .expect("install");
        assert!(r1.accepted);
        let r2 = engine
            .install_routing_config(routing_with_owner(2, slot, 0, masters.clone()))
            .await
            .expect("install");
        assert!(!r2.accepted);
        let r3 = engine
            .install_routing_config(routing_with_owner(1, slot, 0, masters))
            .await
            .expect("install");
        assert!(!r3.accepted);
    }

    #[tokio::test]
    async fn migrates_tile_and_deletes_source_after_success() {
        let source_dir = temp_dir();
        let dest_dir = temp_dir();

        let source = ShardEngine::new(source_dir.path(), 0, 16);
        let dest = ShardEngine::new(dest_dir.path(), 1, 16);

        source
            .become_master(BecomeMasterRequest { shard_id: 0, epoch: 1 })
            .await
            .expect("master");
        dest.become_master(BecomeMasterRequest { shard_id: 1, epoch: 1 })
            .await
            .expect("master");

        let addr = free_addr();
        let server_engine = dest.clone();
        let server_addr: std::net::SocketAddr = addr.parse().expect("addr parse");
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(ReplicationServiceServer::new(ReplicationServiceImpl {
                    shard: server_engine,
                }))
                .serve(server_addr)
                .await
                .expect("server");
        });

        let tile = write([1_u8; 16], "payload");
        let slot = slot_for_tile_key(tile.id, tile.x, tile.y, tile.level);

        source
            .install_routing_config(routing_with_owner(
                1,
                slot,
                0,
                HashMap::from([(0, "127.0.0.1:9".to_string())]),
            ))
            .await
            .expect("routing");
        dest.install_routing_config(routing_with_owner(
            1,
            slot,
            1,
            HashMap::from([(1, addr.clone())]),
        ))
        .await
        .expect("routing");

        source.write_as_master(tile.clone()).await.expect("write");

        source
            .install_routing_config(routing_with_owner(
                2,
                slot,
                1,
                HashMap::from([(1, addr.clone())]),
            ))
            .await
            .expect("routing");
        dest.install_routing_config(routing_with_owner(
            2,
            slot,
            1,
            HashMap::from([(1, addr.clone())]),
        ))
        .await
        .expect("routing");

        let src_path = source.tile_path(&Uuid::from_bytes(tile.id), tile.level, tile.x, tile.y);
        let dst_path = dest.tile_path(&Uuid::from_bytes(tile.id), tile.level, tile.x, tile.y);

        let migrated = wait_for(Duration::from_secs(5), || {
            !src_path.exists() && dst_path.exists()
        })
        .await;
        assert!(migrated, "tile migration timed out");

        server.abort();
    }

    #[tokio::test]
    async fn retries_until_destination_available() {
        let source_dir = temp_dir();
        let dest_dir = temp_dir();

        let source = ShardEngine::new(source_dir.path(), 0, 16);
        let dest = ShardEngine::new(dest_dir.path(), 1, 16);

        source
            .become_master(BecomeMasterRequest { shard_id: 0, epoch: 1 })
            .await
            .expect("master");
        dest.become_master(BecomeMasterRequest { shard_id: 1, epoch: 1 })
            .await
            .expect("master");

        let addr = free_addr();
        let tile = write([2_u8; 16], "payload");
        let slot = slot_for_tile_key(tile.id, tile.x, tile.y, tile.level);
        let id = Uuid::from_bytes(tile.id);

        source
            .install_routing_config(routing_with_owner(
                1,
                slot,
                0,
                HashMap::from([(0, "127.0.0.1:9".to_string())]),
            ))
            .await
            .expect("routing");
        source.write_as_master(tile.clone()).await.expect("write");

        source
            .install_routing_config(routing_with_owner(
                2,
                slot,
                1,
                HashMap::from([(1, addr.clone())]),
            ))
            .await
            .expect("routing");

        let src_path = source.tile_path(&id, tile.level, tile.x, tile.y);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(src_path.exists(), "source tile deleted before successful migration");

        dest.install_routing_config(routing_with_owner(
            2,
            slot,
            1,
            HashMap::from([(1, addr.clone())]),
        ))
        .await
        .expect("routing");

        let server_engine = dest.clone();
        let server_addr: std::net::SocketAddr = addr.parse().expect("addr parse");
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(ReplicationServiceServer::new(ReplicationServiceImpl {
                    shard: server_engine,
                }))
                .serve(server_addr)
                .await
                .expect("server");
        });

        let dst_path = dest.tile_path(&id, tile.level, tile.x, tile.y);
        let migrated = wait_for(Duration::from_secs(6), || {
            !src_path.exists() && dst_path.exists()
        })
        .await;
        assert!(migrated, "tile did not migrate after destination recovered");

        server.abort();
    }

    #[tokio::test]
    async fn writes_to_migrating_tiles_are_rejected() {
        let source_dir = temp_dir();
        let source = ShardEngine::new(source_dir.path(), 0, 16);
        source
            .become_master(BecomeMasterRequest { shard_id: 0, epoch: 1 })
            .await
            .expect("master");

        let tile = write([3_u8; 16], "payload");
        let slot = slot_for_tile_key(tile.id, tile.x, tile.y, tile.level);

        source
            .install_routing_config(routing_with_owner(
                2,
                slot,
                1,
                HashMap::from([(1, "127.0.0.1:9999".to_string())]),
            ))
            .await
            .expect("routing");

        let err = source
            .write_as_master(tile)
            .await
            .err()
            .expect("expected rejection");
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn restart_resumes_migration_from_persisted_routing() {
        let source_dir = temp_dir();
        let dest_dir = temp_dir();

        let dest = ShardEngine::new(dest_dir.path(), 1, 16);
        dest.become_master(BecomeMasterRequest { shard_id: 1, epoch: 5 })
            .await
            .expect("master");

        let addr = free_addr();
        let server_engine = dest.clone();
        let server_addr: std::net::SocketAddr = addr.parse().expect("addr parse");
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(ReplicationServiceServer::new(ReplicationServiceImpl {
                    shard: server_engine,
                }))
                .serve(server_addr)
                .await
                .expect("server");
        });

        let tile = write([9_u8; 16], "persisted");
        let id = Uuid::from_bytes(tile.id);
        let source_path = source_dir
            .path()
            .join(id.to_string())
            .join(tile.level.to_string())
            .join(format!("{}_{}.webp", tile.x, tile.y));
        std::fs::create_dir_all(source_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&source_path, &tile.data).expect("write tile");

        let slot = slot_for_tile_key(tile.id, tile.x, tile.y, tile.level);
        let routing = RoutingTable {
            config_epoch: 10,
            slot_to_shard: {
                let mut slots = vec![0_u32; NUM_SLOTS];
                slots[slot as usize] = 1;
                slots
            },
            shard_masters: HashMap::from([(1, addr.clone())]),
        };
        std::fs::write(
            source_dir.path().join(".routing_config.json"),
            serde_json::to_vec(&routing).expect("serialize"),
        )
        .expect("persist routing");

        let source = ShardEngine::new(source_dir.path(), 0, 16);
        source
            .become_master(BecomeMasterRequest { shard_id: 0, epoch: 6 })
            .await
            .expect("master");

        dest.install_routing_config(routing.to_proto())
            .await
            .expect("dest config");

        let dst_path = dest.tile_path(&id, tile.level, tile.x, tile.y);
        let migrated = wait_for(Duration::from_secs(5), || {
            !source_path.exists() && dst_path.exists()
        })
        .await;
        assert!(migrated, "migration did not resume after restart");

        server.abort();
    }
}
