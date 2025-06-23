use super::{Attribution, Source};
use crate::tile::TileId;

/// <https://www.openstreetmap.org/about>
#[derive(Debug)]
pub struct OpenStreetMap;

impl Source for OpenStreetMap {
    fn tile_url(&self, tile_id: TileId) -> String {
        format!(
            "https://tile.openstreetmap.org/{}/{}/{}.png",
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
}
