//! Some common HTTP tile sources. Make sure you follow terms of usage of the particular source.

mod arcgis;
mod carto;
mod geoportal;
mod mapbox;
mod openstreetmap;

use crate::tile_coord::TileCoord;
pub use arcgis::ArcGisWorldMap;
pub use carto::*;
pub use geoportal::Geoportal;
use iced_core::image::Image;
pub use mapbox::{Mapbox, MapboxStyle};
pub use openstreetmap::OpenStreetMap;

#[derive(Clone)]
pub struct Attribution {
    pub text: &'static str,
    pub url: &'static str,
    pub logo_light: Option<Image>,
    pub logo_dark: Option<Image>,
}

/// Remote tile server definition, source for the [`crate::HttpTiles`].
pub trait Source: core::fmt::Debug + Send + Sync {
    fn tile_url(&self, tile_id: TileCoord) -> String;
    fn attribution(&self) -> Attribution;

    /// Size of each tile, should be a multiple of 256.
    fn tile_size(&self) -> u32 {
        256
    }

    fn max_zoom(&self) -> u8 {
        19
    }
}
