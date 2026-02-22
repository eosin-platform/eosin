use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eosin_types::{ClusterPhase, ReplicaRole, ShardStatus};

pub const DEFAULT_NUM_SLOTS: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicaHealth {
    pub name: String,
    pub role: ReplicaRole,
    pub ready: bool,
    pub last_heartbeat_unix_ms: u64,
    pub replication_lag: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionDecision {
    pub promote: String,
    pub demote: Vec<String>,
    pub new_epoch: u64,
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

pub fn select_best_replica(
    replicas: &[ReplicaHealth],
    now_ms: u64,
    heartbeat_timeout: Duration,
) -> Option<ReplicaHealth> {
    let max_age_ms = heartbeat_timeout.as_millis() as u64;
    replicas
        .iter()
        .filter(|r| r.role == ReplicaRole::ReadReplica)
        .filter(|r| r.ready)
        .filter(|r| now_ms.saturating_sub(r.last_heartbeat_unix_ms) <= max_age_ms)
        .min_by_key(|r| (r.replication_lag.unwrap_or(u64::MAX), r.name.clone()))
        .cloned()
}

pub fn master_healthy(
    replicas: &[ReplicaHealth],
    now_ms: u64,
    heartbeat_timeout: Duration,
) -> bool {
    let max_age_ms = heartbeat_timeout.as_millis() as u64;
    replicas
        .iter()
        .find(|r| r.role == ReplicaRole::Master)
        .is_some_and(|m| m.ready && now_ms.saturating_sub(m.last_heartbeat_unix_ms) <= max_age_ms)
}

pub fn should_failover(
    _shard_status: &ShardStatus,
    replicas: &[ReplicaHealth],
    now_ms: u64,
    heartbeat_timeout: Duration,
    cooldown_active: bool,
) -> bool {
    if cooldown_active {
        return false;
    }
    !master_healthy(replicas, now_ms, heartbeat_timeout)
        && select_best_replica(replicas, now_ms, heartbeat_timeout).is_some()
}

pub fn build_promotion_decision(
    shard_status: &ShardStatus,
    replicas: &[ReplicaHealth],
    now_ms: u64,
    heartbeat_timeout: Duration,
    cooldown_active: bool,
) -> Option<PromotionDecision> {
    if !should_failover(
        shard_status,
        replicas,
        now_ms,
        heartbeat_timeout,
        cooldown_active,
    ) {
        return None;
    }
    let candidate = select_best_replica(replicas, now_ms, heartbeat_timeout)?;
    let demote = replicas
        .iter()
        .filter(|r| r.role == ReplicaRole::Master && r.name != candidate.name)
        .map(|r| r.name.clone())
        .collect();
    Some(PromotionDecision {
        promote: candidate.name,
        demote,
        new_epoch: shard_status.epoch.saturating_add(1),
    })
}

pub fn desired_pod_names(cluster_name: &str, shards: u32, replicas_per_shard: u32) -> Vec<String> {
    let mut names = Vec::new();
    for shard in 0..shards {
        for replica in 0..replicas_per_shard {
            names.push(format!("{cluster_name}-s{shard}-r{replica}"));
        }
    }
    names
}

pub fn topology_diff(desired: &[String], existing: &[String]) -> (Vec<String>, Vec<String>) {
    let desired_set: std::collections::HashSet<_> = desired.iter().cloned().collect();
    let existing_set: std::collections::HashSet<_> = existing.iter().cloned().collect();
    let mut create: Vec<String> = desired_set.difference(&existing_set).cloned().collect();
    let mut delete: Vec<String> = existing_set.difference(&desired_set).cloned().collect();
    create.sort();
    delete.sort();
    (create, delete)
}

pub fn compute_slot_to_shard(shards: u32, num_slots: usize) -> Vec<u32> {
    if shards == 0 || num_slots == 0 {
        return Vec::new();
    }
    let mut table = vec![0_u32; num_slots];
    for (slot, item) in table.iter_mut().enumerate() {
        *item = ((slot as u64 * shards as u64) / num_slots as u64) as u32;
    }
    table
}

pub fn next_config_epoch(current_epoch: u64, current_shards: u32, desired_shards: u32) -> u64 {
    if current_epoch == 0 {
        return 1;
    }
    if current_shards != desired_shards {
        return current_epoch.saturating_add(1);
    }
    current_epoch
}

pub fn determine_cluster_phase(
    shard_statuses: &[ShardStatus],
    target_epoch: u64,
    all_healthy: bool,
) -> ClusterPhase {
    if !all_healthy {
        return ClusterPhase::Degraded;
    }
    let converged = shard_statuses.iter().all(|s| {
        s.config_epoch >= target_epoch && s.migration_queue_len == 0 && s.misplaced_tiles == 0
    });
    if converged {
        ClusterPhase::Ready
    } else {
        ClusterPhase::Reconciling
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eosin_types::{ReplicaSummary, ShardStatus};

    fn replica(name: &str, role: ReplicaRole, ready: bool, hb: u64, lag: Option<u64>) -> ReplicaHealth {
        ReplicaHealth {
            name: name.to_string(),
            role,
            ready,
            last_heartbeat_unix_ms: hb,
            replication_lag: lag,
        }
    }

    fn status(epoch: u64) -> ShardStatus {
        ShardStatus {
            shard_id: 0,
            epoch,
            master: None,
            replicas: vec![ReplicaSummary::default()],
            ready_replicas: 0,
            expected_replicas: 0,
            current_offset: None,
            config_epoch: 0,
            migration_queue_len: 0,
            misplaced_tiles: 0,
            message: None,
            last_failover: None,
            cooldown_until: None,
        }
    }

    #[test]
    fn promotion_selects_smallest_lag_replica() {
        let now = 100_000;
        let replicas = vec![
            replica("m", ReplicaRole::Master, false, now - 20_000, Some(0)),
            replica("r1", ReplicaRole::ReadReplica, true, now - 1_000, Some(50)),
            replica("r2", ReplicaRole::ReadReplica, true, now - 1_000, Some(10)),
        ];
        let decision =
            build_promotion_decision(&status(3), &replicas, now, Duration::from_secs(10), false)
                .expect("decision");
        assert_eq!(decision.promote, "r2");
        assert_eq!(decision.new_epoch, 4);
    }

    #[test]
    fn no_failover_when_master_healthy() {
        let now = 100_000;
        let replicas = vec![
            replica("m", ReplicaRole::Master, true, now - 100, Some(0)),
            replica("r1", ReplicaRole::ReadReplica, true, now - 100, Some(1)),
        ];
        assert!(build_promotion_decision(&status(2), &replicas, now, Duration::from_secs(10), false).is_none());
    }

    #[test]
    fn cooldown_blocks_thrashing() {
        let now = 100_000;
        let s = status(2);
        let replicas = vec![
            replica("m", ReplicaRole::Master, false, now - 20_000, Some(0)),
            replica("r1", ReplicaRole::ReadReplica, true, now - 100, Some(1)),
        ];
        assert!(build_promotion_decision(&s, &replicas, now, Duration::from_secs(10), true).is_none());
    }

    #[test]
    fn stale_master_is_demoted_after_promotion() {
        let now = 100_000;
        let replicas = vec![
            replica("old-master", ReplicaRole::Master, true, now - 20_000, Some(0)),
            replica("candidate", ReplicaRole::ReadReplica, true, now - 100, Some(2)),
        ];
        let decision =
            build_promotion_decision(&status(9), &replicas, now, Duration::from_secs(10), false)
                .expect("decision");
        assert_eq!(decision.promote, "candidate");
        assert_eq!(decision.demote, vec!["old-master".to_string()]);
    }

    #[test]
    fn ignores_stale_replicas_even_with_low_lag() {
        let now = 100_000;
        let replicas = vec![
            replica("m", ReplicaRole::Master, false, now - 20_000, Some(0)),
            replica("stale", ReplicaRole::ReadReplica, true, now - 40_000, Some(0)),
            replica("fresh", ReplicaRole::ReadReplica, true, now - 100, Some(20)),
        ];
        let decision =
            build_promotion_decision(&status(1), &replicas, now, Duration::from_secs(10), false)
                .expect("decision");
        assert_eq!(decision.promote, "fresh");
    }

    #[test]
    fn topology_reconciliation_creates_missing_and_deletes_extra() {
        let desired = desired_pod_names("cluster", 2, 2);
        let existing = vec![
            "cluster-s0-r0".to_string(),
            "cluster-s0-r1".to_string(),
            "cluster-s9-r9".to_string(),
        ];
        let (create, delete) = topology_diff(&desired, &existing);
        assert_eq!(create, vec!["cluster-s1-r0".to_string(), "cluster-s1-r1".to_string()]);
        assert_eq!(delete, vec!["cluster-s9-r9".to_string()]);
    }

    #[test]
    fn computes_even_slot_distribution_for_scaling() {
        let map = compute_slot_to_shard(4, 16);
        assert_eq!(&map[0..4], &[0, 0, 0, 0]);
        assert_eq!(&map[4..8], &[1, 1, 1, 1]);
        assert_eq!(&map[8..12], &[2, 2, 2, 2]);
        assert_eq!(&map[12..16], &[3, 3, 3, 3]);
    }

    #[test]
    fn config_epoch_strictly_increases_on_topology_change() {
        assert_eq!(next_config_epoch(0, 0, 3), 1);
        assert_eq!(next_config_epoch(5, 3, 3), 5);
        assert_eq!(next_config_epoch(5, 3, 4), 6);
    }

    #[test]
    fn reconciling_until_all_migrations_complete() {
        let shard_a = ShardStatus {
            shard_id: 0,
            config_epoch: 5,
            migration_queue_len: 2,
            misplaced_tiles: 1,
            ..status(1)
        };
        let shard_b = ShardStatus {
            shard_id: 1,
            config_epoch: 5,
            migration_queue_len: 0,
            misplaced_tiles: 0,
            ..status(1)
        };
        assert_eq!(
            determine_cluster_phase(&[shard_a.clone(), shard_b.clone()], 5, true),
            ClusterPhase::Reconciling
        );
        let shard_a_done = ShardStatus {
            migration_queue_len: 0,
            misplaced_tiles: 0,
            ..shard_a
        };
        assert_eq!(
            determine_cluster_phase(&[shard_a_done, shard_b], 5, true),
            ClusterPhase::Ready
        );
    }

    #[test]
    fn scaling_down_removes_unused_shards() {
        let map = compute_slot_to_shard(2, 64);
        assert!(map.iter().all(|s| *s < 2));
        assert!(map.iter().any(|s| *s == 0));
        assert!(map.iter().any(|s| *s == 1));
    }
}
