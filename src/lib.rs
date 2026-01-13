mod draw_cache;

pub mod sources;

mod global_element;
mod map_layers;
mod map_program;
mod map_widget;
mod position;
mod projector;
mod tile_cache;
mod tile_coord;
mod viewpoint;
mod zoom;

pub use global_element::GlobalElement;
use iced::Vector;
pub use map_program::{Action, MapProgram};
pub use map_widget::MapWidget;
pub use position::{Geographic, Mercator, location};
pub use projector::Projector;
pub use tile_cache::{CacheMessage, TileCache};
pub use tile_coord::TileCoord;
pub use viewpoint::Viewpoint;
pub use zoom::{InvalidZoom, Zoom};

pub(crate) trait RoundCeil {
    fn round_ceil(self) -> Self;
}

impl RoundCeil for f32 {
    fn round_ceil(self) -> Self {
        (self + 0.5).floor()
    }
}

impl RoundCeil for f64 {
    fn round_ceil(self) -> Self {
        (self + 0.5).floor()
    }
}

impl<T: RoundCeil> RoundCeil for iced::Point<T> {
    fn round_ceil(self) -> Self {
        iced::Point {
            x: self.x.round_ceil(),
            y: self.y.round_ceil(),
        }
    }
}

pub(crate) fn vec_norm(vector: &Vector) -> f32 {
    (vector.x.powi(2) + vector.y.powi(2)).sqrt()
}
