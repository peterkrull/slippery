use iced::{Point, Rectangle, Vector};

use crate::{Geographic, Mercator, Zoom};

/// The viewpoint of the [`MapWidget`] consists of a coordinate of
/// the center of the viewport, and a zoom level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewpoint {
    pub position: Mercator,
    pub zoom: Zoom,
}

impl Viewpoint {
    /// Move the viewpoint to a different location defined by the a [`Mercator`] coordinate
    pub fn move_to_mercator(&mut self, mercator: Mercator) {
        self.position = mercator;
    }

    /// Move the viewpoint to a different location defined by the a [`Geographic`] coordinate
    pub fn move_to_geographic(&mut self, geographic: Geographic) {
        self.position = geographic.as_mercator();
    }

    /// Get the viewpoint position in the pixel space representation
    pub fn into_pixel_space(&self) -> Point<f64> {
        self.position.into_pixel_space(self.zoom.f64())
    }

    /// Get the [`Mercator`] coordinate for a position within the viewport bounds
    pub fn position_in_viewport(&self, position: Point, bounds: Rectangle) -> Mercator {
        // Get cursor position relative to viewport center
        let cursor_offset = position - bounds.center();
        let cursor_offset = Vector::new(cursor_offset.x as f64, cursor_offset.y as f64);

        // Temporarily shift the viewport to be centered over the cursor
        let center_pixel_space = self.position.into_pixel_space(self.zoom.f64());
        let adjusted_center = center_pixel_space + cursor_offset;
        Mercator::from_pixel_space(adjusted_center, self.zoom.f64())
    }

    /// Zoom in/out of a position within some bounds. This would typically be the cursor position
    pub fn zoom_on_point(&mut self, zoom_amount: f64, position: Point, bounds: Rectangle) {
        // Get cursor position relative to viewport center
        let cursor_offset = position - bounds.center();
        let cursor_offset = Vector::new(cursor_offset.x as f64, cursor_offset.y as f64);

        // Temporarily shift the viewport to be centered over the cursor
        let center_pixel_space = self.position.into_pixel_space(self.zoom.f64());
        let adjusted_center = center_pixel_space + cursor_offset;
        self.position = Mercator::from_pixel_space(adjusted_center, self.zoom.f64());

        // Apply desired zoom
        self.zoom.zoom_by(zoom_amount);

        // Shift the viewport back by the same amount after applying zoom
        let center_pixel_space = self.position.into_pixel_space(self.zoom.f64());
        let adjusted_center = center_pixel_space - cursor_offset;
        self.position = Mercator::from_pixel_space(adjusted_center, self.zoom.f64());
    }

    /// Zoom in/out of the center of the viewport
    pub fn zoom_on_center(&mut self, zoom_amount: f64) {
        self.zoom.zoom_by(zoom_amount);
    }
}
