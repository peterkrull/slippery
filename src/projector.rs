use iced::{Point, Rectangle, Vector};

use crate::{Geographic, Mercator, RoundCeil, Viewpoint};

/// Utility for projecting between points in screen space, pixel space or global coordinates.
///
/// Note, most methods for this use the [`Mercator`] projection. Any [`Geographic`](`crate::position::Geographic`) coordinate
/// can easily be converted into its mercator counterpart with the [`Geographic::as_mercator`](`crate::position::Geographic::as_mercator`)
/// function.
#[derive(Debug, Clone, PartialEq)]
pub struct Projector {
    pub viewpoint: Viewpoint,
    pub cursor: Option<Point>,
    pub bounds: Rectangle,
}

impl Projector {
    /// Get the pixel-space coordinate under the cursor.
    pub fn cursor_into_pixel_space(&self) -> Option<Point<f64>> {
        let cursor = self.cursor?;
        Some(self.screen_space_into_pixel_space(cursor))
    }

    /// Get the [`Mercator`] coordinate under the cursor.
    pub fn cursor_into_mercator(&self) -> Option<Mercator> {
        let cursor = self.cursor?;
        Some(self.screen_space_into_mercator(cursor))
    }

    /// Project the [`Mercator`] position into the pixel space. This can be used
    /// to calculate distances in on-screen pixels between geographical coordinates.
    ///
    /// This is distinctly different from [`Projector::mercator_into_screen_space`]
    pub fn mercator_into_pixel_space(&self, position: Mercator) -> Point<f64> {
        Mercator::into_pixel_space(&position, self.viewpoint.zoom.f64())
    }

    /// Determine the [`Mercator`] position of some point in pixel space. Note, this
    /// is *not* the position in the viewport.
    ///
    /// This is distinctly different from [`Projector::screen_space_into_mercator`]
    pub fn pixel_space_into_mercator(&self, point: Point<f64>) -> Mercator {
        Mercator::from_pixel_space(point, self.viewpoint.zoom.f64())
    }

    /// Determines the screen space coordinate of a [`Mercator`] coordinate.
    /// The screen-space coordinate system has its origin in the top-left corner
    /// of the viewport.
    ///
    /// This is distinctly different from [`Projector::mercator_into_pixel_space`].
    ///
    /// The returned [`Point`] may not be within screen bounds
    pub fn mercator_into_screen_space(&self, position: Mercator) -> Point<f32> {
        let position_pixel_space = self.mercator_into_pixel_space(position);
        self.pixel_space_into_screen_space(position_pixel_space)
    }

    /// Determines the [`Mercator`] coordinate of a point in screen-space coordinates.
    /// The screen-space coordinate system has its origin in the top-left corner
    /// of the viewport.
    ///
    /// This is distinctly different from [`Projector::pixel_space_into_mercator`].
    ///
    /// The given [`Point`] may be outside of viewport bounds
    pub fn screen_space_into_mercator(&self, point: Point<f32>) -> Mercator {
        let point_pixel_space = self.screen_space_into_pixel_space(point);
        self.pixel_space_into_mercator(point_pixel_space)
    }

    /// Converts from a screen space point representation to pixel space.
    ///
    /// - The screen space has its origin in the top-left of the viewport
    /// - The pixel space has its origin in the center of the world atlas.
    ///
    /// One unit is always the same in both, but a higher precision is required
    /// when dealing with the larger relative distances in pixel space.
    pub fn screen_space_into_pixel_space(&self, point: Point<f32>) -> Point<f64> {
        let center_pixel_space = self.viewpoint.into_pixel_space().round_ceil();

        let point_offset = point - self.bounds.center();
        let point_offset = Vector::new(point_offset.x as f64, point_offset.y as f64);

        center_pixel_space + point_offset
    }

    /// Converts from a pixel space point representation to screen space.
    ///
    /// - The screen space has its origin in the top-left of the viewport
    /// - The pixel space has its origin in the center of the world atlas.
    ///
    /// One unit is always the same in both, but a higher precision is required
    /// when dealing with the larger relative distances in pixel space.
    pub fn pixel_space_into_screen_space(&self, point: Point<f64>) -> Point<f32> {
        let center_pixel_space = self.viewpoint.into_pixel_space().round_ceil();

        let position_offset = point - center_pixel_space;
        let position_offset = Vector::new(position_offset.x as f32, position_offset.y as f32);

        self.bounds.center() + position_offset
    }

    /// Convenience method to convert a [`Geographic`] coordinate directly
    /// to screen space without manually converting to Mercator first.
    pub fn geographic_into_screen_space(&self, position: Geographic) -> Point<f32> {
        self.mercator_into_screen_space(position.as_mercator())
    }

    /// Convenience method to convert a screen space coordinate directly
    /// to [`Geographic`] without manually converting to Mercator first.
    pub fn screen_space_into_geographic(&self, point: Point<f32>) -> Geographic {
        self.screen_space_into_mercator(point).as_geographic()
    }
}

#[cfg(test)]
mod tests {
    use super::Projector;
    use crate::{Mercator, Zoom};
    use iced::{Point, Rectangle};

    #[test]
    fn inverting_position_of() {
        let projector = Projector {
            viewpoint: crate::Viewpoint {
                position: Mercator::new(0.25, -0.33),
                zoom: Zoom::try_from(10.0).unwrap(),
            },
            cursor: Default::default(),
            bounds: Rectangle {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            },
        };

        let original_point = Point::new(500.0, 300.0);

        let geographic_first = projector.screen_space_into_mercator(original_point);
        let projected_point = projector.mercator_into_screen_space(geographic_first);
        let geographic_second = projector.screen_space_into_mercator(projected_point);

        assert_eq!(original_point, projected_point);
        assert_eq!(geographic_first, geographic_second);
    }
}
