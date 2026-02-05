use std::collections::BTreeMap;

use anyhow::{Result, ensure};

pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub level: u32,
    pub send_count: u32,
    pub last_sent: i64, // ms since epoch
}

type TileKey = u64;

impl Tile {
    pub fn index(&self) -> Result<TileKey> {
        const X_BITS: u64 = 20;
        const Y_BITS: u64 = 20;
        const LEVEL_BITS: u64 = 20;
        const X_MASK: u64 = (1 << X_BITS) - 1;
        const Y_MASK: u64 = (1 << Y_BITS) - 1;
        const LEVEL_MASK: u64 = (1 << LEVEL_BITS) - 1;
        let x = self.x as u64;
        let y = self.y as u64;
        let level = self.level as u64;
        ensure!(x <= X_MASK, "x out of range for index packing");
        ensure!(y <= Y_MASK, "y out of range for index packing");
        ensure!(level <= LEVEL_MASK, "level out of range for index packing");
        let index = x | (y << X_BITS) | (level << (X_BITS + Y_BITS));
        Ok(index)
    }
}

pub struct ClientViewport {
    sent: BTreeMap<TileKey, Tile>,
}

impl ClientViewport {
    pub fn new() -> Self {
        Self {
            sent: BTreeMap::new(),
        }
    }

    fn tile_sent(&mut self, tile: Tile) -> Result<()> {
        let index = tile.index()?;
        if let Some(existing) = self.sent.get_mut(&index) {
            existing.send_count += 1;
            existing.last_sent = tile.last_sent;
            return Ok(());
        }
        self.sent.insert(index, tile);
        Ok(())
    }
}
