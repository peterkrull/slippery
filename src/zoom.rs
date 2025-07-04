#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("invalid zoom level")]
pub struct InvalidZoom;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom(f64);

impl TryFrom<f64> for Zoom {
    type Error = InvalidZoom;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        // The upper limit is artificial. Should it be removed altogether?
        if !(2. ..=26.).contains(&value) {
            Err(InvalidZoom)
        } else {
            Ok(Self(value))
        }
    }
}

// The reverse shouldn't be implemented, since we already have TryInto<f32>.
#[allow(clippy::from_over_into)]
impl Into<f64> for Zoom {
    fn into(self) -> f64 {
        self.0
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self(16.)
    }
}

impl Zoom {
    pub fn round(&self) -> u8 {
        self.0.round() as u8
    }

    pub fn f64(&self) -> f64 {
        self.0
    }

    pub fn f32(&self) -> f32 {
        self.0 as f32
    }

    pub fn zoom_in(&mut self) -> Result<(), InvalidZoom> {
        *self = Self::try_from(self.0 + 1.)?;
        Ok(())
    }

    pub fn zoom_out(&mut self) -> Result<(), InvalidZoom> {
        *self = Self::try_from(self.0 - 1.)?;
        Ok(())
    }

    /// Zoom using a relative value.
    pub fn zoom_by(&mut self, zoom_amount: f64) {
        if let Ok(new_self) = Self::try_from(self.0 + zoom_amount) {
            *self = new_self;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructing_zoom() {
        assert_eq!(16, Zoom::default().round());
        assert_eq!(26, Zoom::try_from(26.).unwrap().round());
        assert_eq!(InvalidZoom, Zoom::try_from(27.).unwrap_err());
    }

    #[test]
    fn test_zooming_in() {
        let mut zoom = Zoom::try_from(25.).unwrap();
        assert!(zoom.zoom_in().is_ok());
        assert_eq!(26, zoom.round());
        assert_eq!(Err(InvalidZoom), zoom.zoom_in());
    }

    #[test]
    fn test_zooming_out() {
        let mut zoom = Zoom::try_from(1.).unwrap();
        assert!(zoom.zoom_out().is_ok());
        assert_eq!(0, zoom.round());
        assert_eq!(Err(InvalidZoom), zoom.zoom_out());
    }
}
