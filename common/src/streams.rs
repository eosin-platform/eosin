use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod topics {
    pub const CACHE_MISS: &str = "histion.cache.miss";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMissEvent {
    pub id: Uuid,
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

impl CacheMissEvent {
    pub fn hash(&self) -> String {
        format!("{}.{}.{}.{}", self.id, self.level, self.x, self.y)
    }
}
