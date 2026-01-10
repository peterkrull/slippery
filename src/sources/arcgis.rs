use crate::{TileCoord, sources::Attribution};

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct ArcGisWorldMap;

impl super::Source for ArcGisWorldMap {
    fn tile_url(&self, tile_id: TileCoord) -> String {
        format!(
            "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}",
            tile_id.zoom(),
            tile_id.y(),
            tile_id.x(),
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
