use uuid::Uuid;

pub fn shard_for_id(id: Uuid, _x: u32, _y: u32, _level: u32) -> u32 {
    // Use the first 4 bytes of the UUID for shard distribution
    // This provides uniform distribution for random UUIDs (v4)
    let bytes = id.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
