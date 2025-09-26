use iced::{Point, Rectangle, Vector};

use crate::{Geographic, Viewpoint};

#[derive(Debug, Clone, PartialEq)]
pub struct Projector {
    pub viewpoint: Viewpoint,
    pub cursor: Option<Point>,
    pub bounds: Rectangle,
}

impl Projector {
    /// Project the cursor into the pixel space.
    pub fn cursor_into_pixel_space(&self) -> Option<Point<f64>> {
        let cursor_offset = self.cursor? - self.bounds.center();
        let cursor_offset = Vector::new(cursor_offset.x as f64, cursor_offset.y as f64);

        let center = self.viewpoint.into_pixel_space();
        Some(center + cursor_offset)
    }

    /// Project the `position` into the pixel space. This can be used to calculate
    /// distances in on-screen pixels between geographical coordinates.
    pub fn position_into_pixel_space(&self, position: Geographic) -> Point<f64> {
        position.into_pixel_space(self.viewpoint.zoom.f64())
    }

    /// Determine the geographical location of some point in pixel space. Note, this
    /// is *not* the position in the viewport.
    pub fn position_from_pixel_space(&self, point: Point<f64>) -> Geographic {
        Geographic::from_pixel_space(point, self.viewpoint.zoom.f64())
    }

    pub fn cursor_position(&self) -> Option<Geographic> {
        let cursor = self.cursor_into_pixel_space()?;
        let cursor = self.position_from_pixel_space(cursor);
        Some(cursor)
    }

    pub fn screen_position_of(&self, position: Geographic) -> Point {
        let center_pixel_space = self.viewpoint.into_pixel_space();
        let pos_pixel_space = position.into_pixel_space(self.viewpoint.zoom.f64());

        let position_offset = pos_pixel_space - center_pixel_space;
        let position_offset = Vector::new(position_offset.x as f32, position_offset.y as f32);

        self.bounds.center() + position_offset
    }

    pub fn geographical_position_of(&self, point: Point) -> Geographic {
        let center_pixel_space = self.viewpoint.into_pixel_space();

        let point_center_offset = point - self.bounds.center();
        let point_center_offset =
            Vector::new(point_center_offset.x as f64, point_center_offset.y as f64);

        let point_pixel_space = center_pixel_space + point_center_offset;

        Geographic::from_pixel_space(point_pixel_space, self.viewpoint.zoom.f64())
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Rectangle};

    use crate::{Mercator, Zoom};

    use super::Projector;

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

        let geographic_first = projector.geographical_position_of(original_point);
        let projected_point = projector.screen_position_of(geographic_first);
        let geographic_second = projector.geographical_position_of(projected_point);

        assert_eq!(original_point, projected_point);
        assert_eq!(geographic_first, geographic_second);
    }
}
