use std::collections::HashMap;

use iced::{Animation, Rectangle};
use iced_core::image::{Allocation, Handle};

use crate::tile_coord::TileCoord;

pub(crate) struct DrawCache {
    pub(crate) maps: HashMap<u8, HashMap<(u32, u32), DrawData>>,
}

pub struct DrawData {
    pub handle: Handle,
    pub rectangle: Rectangle,
    pub allocation: Allocation,
    pub state: State,
}

enum State {
    Remove,
    Active(Animation<bool>),
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

    /// Clean up any tile which is no longer viewable at all.
    pub fn retain_intersections(&mut self, bounds: &Rectangle) {
        for (_, tiles) in self.maps.iter_mut() {
            tiles.retain(|_, draw_data| draw_data.rectangle.intersects(&bounds));
        }

        self.maps.retain(|_, map|!map.is_empty());
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
    pub fn get_mut(&mut self, tile_id: &TileCoord) -> Option<&mut DrawData> {
        self.maps
            .get_mut(&tile_id.zoom())
            .map(|inner| inner.get_mut(&tile_id.x_y()))
            .flatten()
    }

    /// Check whether the cache contains some tile
    pub fn contains_key(&mut self, tile_id: &TileCoord) -> bool {
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
        animation: Animation<bool>,
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
                    state: State::Active(animation)
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
