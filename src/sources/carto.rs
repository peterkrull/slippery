use crate::{TileCoord, sources::Attribution};

#[derive(Debug, Clone, Copy)]
pub enum Scale {
    X1 = 1,
    X2 = 2,
    X4 = 4,
    X8 = 8,
}

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct CartoLight(pub Scale);

impl super::Source for CartoLight {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://basemaps.cartocdn.com/light_all/{}/{}/{}@{}x.png",
            tile_id.zoom(),
            tile_id.x(),
            tile_id.y(),
            self.0 as u32,
        )
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "OpenStreetMap contributors",
            url: "https://www.openstreetmap.org/copyright",
            logo_light: None,
            logo_dark: None,
        }
    }

    /// Size of each tile, should be a multiple of 256.
    fn tile_size(&self) -> u32 {
        256 * self.0 as u32
    }
}

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct CartoDark(pub Scale);

impl super::Source for CartoDark {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://basemaps.cartocdn.com/dark_all/{}/{}/{}@{}x.png",
            tile_id.zoom(),
            tile_id.x(),
            tile_id.y(),
            self.0 as u32,
        )
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "OpenStreetMap contributors",
            url: "https://www.openstreetmap.org/copyright",
            logo_light: None,
            logo_dark: None,
        }
    }

    /// Size of each tile, should be a multiple of 256.
    fn tile_size(&self) -> u32 {
        512
    }
}

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct CartoVoyager(pub Scale);

impl super::Source for CartoVoyager {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://basemaps.cartocdn.com/rastertiles/voyager/{}/{}/{}@{}x.png",
            tile_id.zoom(),
            tile_id.x(),
            tile_id.y(),
            self.0 as u32,
        )
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "OpenStreetMap contributors",
            url: "https://www.openstreetmap.org/copyright",
            logo_light: None,
            logo_dark: None,
        }
    }

    /// Size of each tile, should be a multiple of 256.
    fn tile_size(&self) -> u32 {
        512
    }
}

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct OpenTopo;

impl super::Source for OpenTopo {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://tile.opentopomap.org/{}/{}/{}.png",
            tile_id.zoom(),
            tile_id.x(),
            tile_id.y()
        )
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "OpenStreetMap contributors",
            url: "https://www.openstreetmap.org/copyright",
            logo_light: None,
            logo_dark: None,
        }
    }

    /// Size of each tile, should be a multiple of 256.
    fn tile_size(&self) -> u32 {
        256
    }
}
