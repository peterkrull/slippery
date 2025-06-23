use iced::{Rectangle, Vector};

use crate::position::total_tiles;

/// Identifies the tile in the tile grid.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct TileId {
    /// X number of the tile.
    x: u32,

    /// Y number of the tile.
    y: u32,

    /// Zoom level, where 0 means no zoom.
    /// See: <https://wiki.openstreetmap.org/wiki/Zoom_levels>
    zoom: u8,
}

impl TileId {
    /// The lowest-quality zoom level
    pub const ZERO: Self = TileId {
        x: 0,
        y: 0,
        zoom: 0,
    };

    pub fn new(x: u32, y: u32, zoom: u8) -> Self {
        let num_tiles = 2u32.pow(zoom as u32);
        TileId {
            x: x.clamp(0, num_tiles - 1),
            y: y.clamp(0, num_tiles - 1),
            zoom,
        }
    }

    pub fn x(&self) -> u32 {
        self.x
    }

    pub fn y(&self) -> u32 {
        self.y
    }

    pub fn x_y(&self) -> (u32, u32) {
        (self.x, self.y)
    }

    pub fn zoom(&self) -> u8 {
        self.zoom
    }

    /// Tile position (in pixels) on the "World bitmap".
    pub fn project(&self, tile_size: f64) -> iced::Point<f64> {
        let total_tiles = 2u32.pow(self.zoom as u32);
        iced::Point::new(
            (self.x as f64 - total_tiles as f64 / 2.0) * tile_size,
            (self.y as f64 - total_tiles as f64 / 2.0) * tile_size,
        )
    }

    pub fn on_viewport(
        &self,
        viewport: Rectangle,
        tile_size: f64,
        position: iced::Point<f64>,
    ) -> Rectangle {
        // Determine the offset of this tile relative to the viewport center
        let tile_bitmap_position = self.project(tile_size);
        let tile_center_offset = tile_bitmap_position - position;
        let tile_center_offset =
            Vector::new(tile_center_offset.x as f32, tile_center_offset.y as f32);

        // The absolute on-screen position of the top-left corner
        let tile_screen_position = viewport.center() + tile_center_offset;

        Rectangle {
            x: tile_screen_position.x as f32,
            y: tile_screen_position.y as f32,
            width: tile_size as f32,
            height: tile_size as f32,
        }
    }

    // Obtain a lower-zoom level TileID that covers this tile.
    // Useful for filling in not-yet loaded tiles.
    pub fn downsample(&self) -> Option<TileId> {
        Some(TileId {
            x: self.x / 2,
            y: self.y / 2,
            zoom: self.zoom.checked_sub(1)?,
        })
    }

    pub fn east(&self) -> Option<TileId> {
        (self.x < total_tiles(self.zoom) - 1).then(|| TileId {
            x: self.x + 1,
            y: self.y,
            zoom: self.zoom,
        })
    }

    pub fn west(&self) -> Option<TileId> {
        Some(TileId {
            x: self.x.checked_sub(1)?,
            y: self.y,
            zoom: self.zoom,
        })
    }

    pub fn north(&self) -> Option<TileId> {
        Some(TileId {
            x: self.x,
            y: self.y.checked_sub(1)?,
            zoom: self.zoom,
        })
    }

    pub fn south(&self) -> Option<TileId> {
        (self.y < total_tiles(self.zoom) - 1).then(|| TileId {
            x: self.x,
            y: self.y + 1,
            zoom: self.zoom,
        })
    }

    pub fn neighbors(&self) -> [Option<TileId>; 4] {
        [self.north(), self.east(), self.south(), self.west()]
    }

    pub fn valid(&self) -> bool {
        self.x < total_tiles(self.zoom) && self.y < total_tiles(self.zoom)
    }
}
