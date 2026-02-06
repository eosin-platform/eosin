//! Priority work queue for tile fetching.
//!
//! This module provides a priority queue that ensures coarse tiles (higher mip
//! level index) are processed before fine tiles. This is critical for progressive
//! loading: users should see a blurry overview immediately, then detail fills in.
//!
//! The queue uses a binary heap internally. When workers pull work items, they
//! receive the highest-priority (coarsest) tile first.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::viewport::RetrieveTileWork;

/// Wrapper that orders work items by mip level (coarse first).
///
/// Higher level index = coarser tile = higher priority.
struct PrioritizedWork {
    work: RetrieveTileWork,
    /// Sequence number to maintain FIFO order among same-priority items.
    seq: u64,
}

impl PartialEq for PrioritizedWork {
    fn eq(&self, other: &Self) -> bool {
        self.work.meta.level == other.work.meta.level && self.seq == other.seq
    }
}

impl Eq for PrioritizedWork {}

impl PartialOrd for PrioritizedWork {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedWork {
    fn cmp(&self, other: &Self) -> Ordering {
        // Primary sort: higher level (coarser) = higher priority
        match self.work.meta.level.cmp(&other.work.meta.level) {
            Ordering::Equal => {
                // Secondary sort: lower seq (earlier) = higher priority
                // Note: reversed because BinaryHeap is a max-heap
                other.seq.cmp(&self.seq)
            }
            ord => ord,
        }
    }
}

/// Shared state for the priority queue.
struct Inner {
    heap: BinaryHeap<PrioritizedWork>,
    next_seq: u64,
    closed: bool,
}

/// Priority queue for tile work items.
///
/// Multiple senders can push work, and multiple workers can pop work.
/// Coarse tiles (higher level index) are always dequeued before fine tiles.
#[derive(Clone)]
pub struct PriorityWorkQueue {
    inner: Arc<Mutex<Inner>>,
    notify: Arc<Notify>,
}

impl PriorityWorkQueue {
    /// Create a new priority work queue.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                heap: BinaryHeap::new(),
                next_seq: 0,
                closed: false,
            })),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Push a work item onto the queue.
    ///
    /// Returns `Err` if the queue has been closed.
    pub fn push(&self, work: RetrieveTileWork) -> Result<(), RetrieveTileWork> {
        let mut inner = self.inner.lock();
        if inner.closed {
            return Err(work);
        }
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.heap.push(PrioritizedWork { work, seq });
        drop(inner);
        self.notify.notify_one();
        Ok(())
    }

    /// Pop the highest-priority work item, waiting if the queue is empty.
    ///
    /// Returns `None` if the queue is closed and empty.
    pub async fn pop(&self) -> Option<RetrieveTileWork> {
        loop {
            // Fast path: try to pop without waiting
            {
                let mut inner = self.inner.lock();
                if let Some(pw) = inner.heap.pop() {
                    return Some(pw.work);
                }
                if inner.closed {
                    return None;
                }
            }
            // Wait for notification
            self.notify.notified().await;
        }
    }

    /// Close the queue, preventing new pushes and waking all waiting poppers.
    pub fn close(&self) {
        let mut inner = self.inner.lock();
        inner.closed = true;
        drop(inner);
        // Wake all waiters so they can exit
        self.notify.notify_waiters();
    }

    /// Get the current number of items in the queue.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.lock().heap.len()
    }

    /// Check if the queue is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().heap.is_empty()
    }
}

impl Default for PriorityWorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::{ImageDesc, TileMeta, Viewport};
    use bytes::Bytes;
    use parking_lot::RwLock;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock as TokioRwLock;
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    fn make_work(level: u32) -> RetrieveTileWork {
        let (tx, _rx) = async_channel::bounded::<Bytes>(1);
        RetrieveTileWork {
            slide_id: Uuid::nil(),
            slot: 0,
            cancel: CancellationToken::new(),
            tx,
            meta: TileMeta { x: 0, y: 0, level },
            client_ip: None,
            viewport: Arc::new(TokioRwLock::new(Some(Viewport {
                x: 0.0,
                y: 0.0,
                width: 800,
                height: 600,
                zoom: 1.0,
            }))),
            image: ImageDesc {
                id: Uuid::nil(),
                width: 1000,
                height: 1000,
                levels: 5,
            },
            dpi: 96.0,
            sent: Arc::new(RwLock::new(FxHashMap::default())),
        }
    }

    #[tokio::test]
    async fn test_priority_order() {
        let queue = PriorityWorkQueue::new();

        // Push fine tiles first
        assert!(queue.push(make_work(0)).is_ok());
        assert!(queue.push(make_work(1)).is_ok());
        assert!(queue.push(make_work(2)).is_ok());

        // Push coarse tiles last
        assert!(queue.push(make_work(4)).is_ok());
        assert!(queue.push(make_work(3)).is_ok());

        // Should pop coarse first
        assert_eq!(queue.pop().await.unwrap().meta.level, 4);
        assert_eq!(queue.pop().await.unwrap().meta.level, 3);
        assert_eq!(queue.pop().await.unwrap().meta.level, 2);
        assert_eq!(queue.pop().await.unwrap().meta.level, 1);
        assert_eq!(queue.pop().await.unwrap().meta.level, 0);
    }

    #[tokio::test]
    async fn test_fifo_within_level() {
        let queue = PriorityWorkQueue::new();

        // Push multiple items at same level
        for x in 0..5 {
            let mut work = make_work(2);
            work.meta.x = x;
            assert!(queue.push(work).is_ok());
        }

        // Should pop in FIFO order within the same level
        for x in 0..5 {
            assert_eq!(queue.pop().await.unwrap().meta.x, x);
        }
    }
}
