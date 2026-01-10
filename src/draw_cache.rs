use std::collections::HashMap;

use iced::{Rectangle};
use iced_core::image::{Allocation, Handle};

use crate::tile_coord::TileCoord;

pub(crate) struct DrawCache {
    pub(crate) maps: HashMap<u8, HashMap<(u32, u32), DrawData>>,
}

pub struct DrawData {
    pub handle: Handle,
    pub rectangle: Rectangle,
    pub allocation: Allocation,
}


impl Default for DrawCache {
    fn default() -> Self {
        DrawCache::new()
    }
}

impl DrawCache {
    pub fn new() -> Self {
        Self {
            maps: HashMap::with_capacity(2),
        }
    }

    /// Remove a tiles handle and allocation for reuse
    pub fn remove(&mut self, tile_id: &TileCoord) -> Option<(Handle, Allocation)> {
        self.maps
            .get_mut(&tile_id.zoom())
            .map(|inner| {
                inner
                    .remove(&tile_id.x_y())
                    .map(|data| (data.handle, data.allocation))
            })
            .flatten()
    }

    /// Check whether the cache contains some tile
    pub fn contains_key(&self, tile_id: &TileCoord) -> bool {
        self.maps
            .get(&tile_id.zoom())
            .is_some_and(|inner| inner.contains_key(&tile_id.x_y()))
    }

    /// Insert a tile using its Id, image handle and its screen-space rectangle
    pub fn insert(
        &mut self,
        tile_id: TileCoord,
        handle: Handle,
        rectangle: Rectangle,
        allocation: Allocation,
    ) {
        self.maps
            .entry(tile_id.zoom())
            .or_insert_with(|| HashMap::with_capacity(25))
            .insert(
                tile_id.x_y(),
                DrawData {
                    handle,
                    rectangle,
                    allocation,
                },
            );
    }

    /// Iterate through all tiles in ascending zoom order
    pub fn iter_tiles(&self) -> impl Iterator<Item = &DrawData> {
        let mut zooms: Vec<&u8> = self.maps.keys().collect();
        zooms.sort();

        zooms.into_iter().flat_map(|zoom| {
            let map = &self.maps[zoom];
            map.iter().map(move |(_, data)| data)
        })
    }
}
