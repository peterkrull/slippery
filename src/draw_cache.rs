use std::collections::HashMap;

use iced::Rectangle;
use iced_core::image::Handle;

use crate::tile_coord::TileCoord;

pub(crate) struct DrawCache<'a> {
    maps: HashMap<u8, HashMap<(u32, u32), (&'a Handle, Rectangle)>>,
}

impl<'a> DrawCache<'a> {
    pub fn new() -> Self {
        Self {
            maps: HashMap::with_capacity(4),
        }
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
        value: &'a Handle,
        rectangle: Rectangle,
    ) -> Option<(&'a Handle, Rectangle)> {
        self.maps
            .entry(tile_id.zoom())
            .or_insert_with(|| HashMap::with_capacity(25))
            .insert(tile_id.x_y(), (value, rectangle))
    }

    /// Iterate through all tiles in ascending zoom order
    pub fn iter_tiles(&self) -> impl Iterator<Item = (&'a Handle, Rectangle)> {
        let mut zooms: Vec<&u8> = self.maps.keys().collect();
        zooms.sort();

        zooms.into_iter().flat_map(|zoom| {
            let map = &self.maps[zoom];
            map.iter().map(move |(_, v)| *v)
        })
    }
}
