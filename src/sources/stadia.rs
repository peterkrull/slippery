use crate::{TileCoord, sources::Attribution};

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct StadiaBright;

impl super::Source for StadiaBright {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://tiles-eu.stadiamaps.com/tiles/osm_bright/{}/{}/{}.png",
            tile_id.zoom(),
            tile_id.x(),
            tile_id.y(),
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
