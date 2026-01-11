//! Project the lat/lon coordinates into a 2D x/y using the Web Mercator.
//! <https://en.wikipedia.org/wiki/Web_Mercator_projection>
//! <https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames>
//! <https://www.netzwolf.info/osm/tilebrowser.html?lat=51.157800&lon=6.865500&zoom=14>

use crate::{map_widget::BASE_SIZE, tile_coord::TileCoord};
use std::f64::consts::PI;

// zoom level   tile coverage  number of tiles  tile size(*) in degrees
// 0            1 tile         1 tile           360° x 170.1022°
// 1            2 × 2 tiles    4 tiles          180° x 85.0511°
// 2            4 × 4 tiles    16 tiles         90° x [variable]

pub(crate) fn total_tiles(zoom: u8) -> u32 {
    2u32.pow(zoom as u32)
}

/// A position on the 2D mercator map projection.
/// Values range from `[-1 .. =1]` in both
/// x (east positive) and y (north positive) directions
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mercator {
    x: f64,
    y: f64,
}

impl Mercator {
    pub const fn new(east: f64, north: f64) -> Self {
        Self {
            x: east.clamp(-1., 1.),
            y: north.clamp(-1., 1.),
        }
    }

    pub fn as_geographic(&self) -> Geographic {
        Geographic::new(
            (self.x * PI).to_degrees(),
            -(self.y * PI).sinh().atan().to_degrees(),
        )
    }

    /// Add the first argument and subtract the second.
    /// Useful for adding the difference: `add - sub`
    pub fn add_sub(&mut self, add: Self, sub: Self) {
        *self = Mercator::new(
            add.east_x() - sub.east_x() + self.east_x(),
            add.north_y() - sub.north_y() + self.north_y(),
        );
    }

    pub fn east_x(&self) -> f64 {
        self.x
    }

    pub fn north_y(&self) -> f64 {
        self.y
    }

    pub fn from_pixel_space(point: iced::Point<f64>, zoom: f64) -> Self {
        let pixels_half_width = 2f64.powf(zoom - 1.0) * (BASE_SIZE as f64);
        Self::new(point.x / pixels_half_width, point.y / pixels_half_width)
    }

    pub fn into_pixel_space(&self, zoom: f64) -> iced::Point<f64> {
        let pixels_half_width = 2f64.powf(zoom - 1.0) * (BASE_SIZE as f64);
        iced::Point::new(
            self.east_x() * pixels_half_width,
            self.north_y() * pixels_half_width,
        )
    }

    /// Determine the tile id for a given position and zoom level,
    pub fn tile_id(&self, zoom: u8) -> TileCoord {
        let x = (self.east_x() + 1.0) / 2.0;
        let y = (self.north_y() + 1.0) / 2.0;

        // Map that into a big bitmap made out of web tiles.
        let number_of_tiles = 2u32.pow(zoom as u32);
        let x = (x * number_of_tiles as f64).floor() as u32;
        let y = (y * number_of_tiles as f64).floor() as u32;

        TileCoord::new(x, y, zoom)
    }
}

/// A position on a sphere consisting of longitude
/// and latitude components, ranging from `[-180 .. =180]`
/// and `[-90 .. =90]` respectively.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geographic {
    lon: f64,
    lat: f64,
}

impl Geographic {
    pub const fn new(lon: f64, lat: f64) -> Self {
        Self {
            lon: lon.clamp(-180., 180.),
            lat: lat.clamp(-90., 90.),
        }
    }

    pub fn from_pixel_space(point: iced::Point<f64>, zoom: f64) -> Self {
        Mercator::from_pixel_space(point, zoom).as_geographic()
    }

    pub fn into_pixel_space(&self, zoom: f64) -> iced::Point<f64> {
        self.as_mercator().into_pixel_space(zoom)
    }

    pub fn as_mercator(&self) -> Mercator {
        Mercator::new(
            self.lon.to_radians() / PI,
            -self.lat.to_radians().tan().asinh() / PI,
        )
    }

    pub fn longitude(&self) -> f64 {
        self.lon
    }

    pub fn latitude(&self) -> f64 {
        self.lat
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;

    #[test]
    fn mercator_to_geographic2() {
        assert_eq!(
            Geographic::new(0.0, 0.0).as_mercator(),
            Mercator::new(0.0, 0.0)
        );
        assert_eq!(
            Geographic::new(0.0, 0.0),
            Mercator::new(0.0, 0.0).as_geographic()
        );

        approx::assert_relative_eq!(
            Geographic::new(90.0, 0.0).as_mercator().east_x(),
            Mercator::new(0.5, 0.0).east_x()
        );

        approx::assert_relative_eq!(
            Geographic::new(-180.0, 0.0).as_mercator().east_x(),
            Mercator::new(-1.0, 0.0).east_x(),
        );
    }

    #[test]
    fn pixel_space_conversion() {
        let position = Mercator::new(1.0, 1.0);
        let pixel_space = position.into_pixel_space(1.);
        let converted = Mercator::from_pixel_space(pixel_space, 1.);

        assert_eq!(position, converted);

        assert_eq!(pixel_space, iced::Point::new(256.0, 256.0))
    }
}
