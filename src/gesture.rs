//! Touch gesture recognition for the map widget.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use iced::{Point, Vector, touch::Finger};

/// Distance the gesture centroid must move, in pixels, before panning engages.
const PAN_ENGAGE_DISTANCE: f32 = 8.0;
/// Zoom change, in levels, before pinch zooming engages.
const ZOOM_ENGAGE_DELTA: f64 = 0.1;
/// Rate at which pinch movement scales zoom, relative to the raw spread ratio.
const PINCH_ZOOM_GAIN: f64 = 1.0;
/// Pan velocity, in px/s, above which a released gesture continues with momentum.
pub const PAN_MOMENTUM_THRESHOLD: f32 = 10.0;
/// Zoom velocity, in levels/s, above which a released pinch continues with momentum.
pub const ZOOM_MOMENTUM_THRESHOLD: f64 = 0.1;
/// Window of gesture history kept around for momentum computations.
pub const MOMENTUM_WINDOW: Duration = Duration::from_millis(50);

/// Screen-space deltas to apply to the viewpoint, measured by a [`GestureTracker`].
#[derive(Debug, Default)]
pub struct Motion {
    /// Translation to pan the map content by.
    pub pan: Option<Vector>,
    /// Zoom change, anchored at a screen position.
    pub zoom: Option<ZoomDelta>,
}

/// A zoom change, anchored at a screen position.
#[derive(Debug)]
pub struct ZoomDelta {
    /// Change in zoom levels.
    pub levels: f64,
    /// Screen position to keep fixed while zooming.
    pub anchor: Point,
}

/// Momentum handed off by a [`GestureTracker`] when a gesture ends.
#[derive(Debug, Default)]
pub struct Release {
    /// Pan velocity in px/s.
    pub pan: Option<Vector>,
    /// Zoom velocity, anchored at a screen position.
    pub zoom: Option<ZoomMomentum>,
}

/// Zoom momentum, anchored at a screen position.
#[derive(Debug)]
pub struct ZoomMomentum {
    /// Zoom levels per second.
    pub velocity: f64,
    /// Screen position to keep fixed while zooming.
    pub anchor: Point,
}

/// Recognizes pan and pinch-zoom gestures from touch input.
///
/// The tracker is pure: it knows nothing about the map and only deals in
/// screen-space values, making it straightforward to test.
///
/// Gestures follow a few rules, which keep transitions between finger counts
/// seamless:
/// - The two first fingers on the map form a pair which drives panning (by its
///   centroid) and pinch zooming (by its spread). Extra fingers joining or
///   leaving, e.g. a resting palm, are ignored until the pair itself changes.
/// - Whenever the fingers driving the gesture change, the gesture silently
///   re-anchors: nothing moves, but it must first re-engage, moving further
///   than [`PAN_ENGAGE_DISTANCE`] or [`ZOOM_ENGAGE_DELTA`]. This absorbs the
///   jitter of fingers joining or leaving without any input being dropped.
/// - A short history of centroid and spread positions is kept, from which
///   release velocities are measured, handing off momentum to the map.
///
/// Tap recognition could later be added by tracking per-finger press
/// positions and times, which the [`GestureTracker::press`] entry point
/// would carry.
#[derive(Debug, Default)]
pub struct GestureTracker {
    /// Fingers currently on the map, by position.
    fingers: HashMap<Finger, Point>,
    /// The two fingers driving the gesture, while two or more are down.
    pair: Option<(Finger, Finger)>,
    /// Centroid and spread the current gesture engaged from.
    anchor_centroid: Option<Point>,
    anchor_spread: Option<f32>,
    /// Centroid and spread of the last move, for incremental deltas.
    last_centroid: Option<Point>,
    last_spread: Option<f32>,
    pan_engaged: bool,
    zoom_engaged: bool,
    pan_samples: VecDeque<Sample<Point>>,
    zoom_samples: VecDeque<Sample<f32>>,
}

#[derive(Debug)]
struct Sample<T> {
    value: T,
    at: Instant,
}

impl GestureTracker {
    /// Whether any finger is currently on the map.
    pub fn is_active(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// A finger was pressed onto the map.
    pub fn press(&mut self, finger: Finger, position: Point) {
        let before = self.gesture_fingers();
        self.fingers.insert(finger, position);
        self.reconcile_pair();
        self.reanchor_if_changed(before);
    }

    /// A finger on the map moved, returning the deltas to apply.
    ///
    /// Returns `None` if the finger is not on the map.
    pub fn moved(&mut self, finger: Finger, position: Point, now: Instant) -> Option<Motion> {
        let on_map = self.fingers.get_mut(&finger)?;
        *on_map = position;

        // Fingers outside the pair, e.g. a resting palm, only need their
        // position tracked
        if !self.pair.is_none_or(|(a, b)| a == finger || b == finger) {
            return Some(Motion::default());
        }

        let (Some(centroid), spread) = self.measure() else {
            return Some(Motion::default());
        };

        self.pan_samples.push_back(Sample {
            value: centroid,
            at: now,
        });
        Self::prune(&mut self.pan_samples, now);
        if let Some(spread) = spread {
            self.zoom_samples.push_back(Sample {
                value: spread,
                at: now,
            });
            Self::prune(&mut self.zoom_samples, now);
        }

        // Engage once the gesture leaves the neighborhood it anchored in
        if !self.pan_engaged
            && self
                .anchor_centroid
                .is_some_and(|anchor| centroid.distance(anchor) > PAN_ENGAGE_DISTANCE)
        {
            self.pan_engaged = true;
        }
        if !self.zoom_engaged
            && spread
                .zip(self.anchor_spread)
                .and_then(|(spread, anchor)| Self::pinch_zoom(spread, anchor))
                .is_some_and(|zoom| zoom.abs() > ZOOM_ENGAGE_DELTA)
        {
            self.zoom_engaged = true;
        }

        // Deltas are incremental, keeping each move small and free of drift
        let mut motion = Motion::default();

        if self.pan_engaged {
            motion.pan = self.last_centroid.map(|last| centroid - last);
        }
        if self.zoom_engaged {
            motion.zoom = spread
                .zip(self.last_spread)
                .and_then(|(spread, last)| Self::pinch_zoom(spread, last))
                .map(|levels| ZoomDelta {
                    levels: levels * PINCH_ZOOM_GAIN,
                    anchor: centroid,
                });
        }

        self.last_centroid = Some(centroid);
        self.last_spread = spread;

        Some(motion)
    }

    /// A finger was lifted off the map, returning any momentum to hand off.
    ///
    /// Returns `None` if the finger is not on the map.
    pub fn released(&mut self, finger: Finger, now: Instant) -> Option<Release> {
        let before = self.gesture_fingers();
        self.fingers.remove(&finger)?;

        let mut release = Release::default();

        // The pinch ends when either of its fingers lifts, handing off its zoom momentum
        if self.pair.is_some_and(|(a, b)| a == finger || b == finger) {
            release.zoom = self.zoom_momentum(now);
        }

        // The gesture ends when the last finger lifts, handing off its pan momentum
        if self.fingers.is_empty() {
            release.pan = self.pan_momentum(now);
        }

        self.reconcile_pair();
        self.reanchor_if_changed(before);

        Some(release)
    }

    /// A finger was lost off the map, e.g. by the system stealing the gesture.
    ///
    /// A lost gesture never hands off momentum. Returns whether the finger
    /// was on the map.
    pub fn cancelled(&mut self, finger: Finger) -> bool {
        let before = self.gesture_fingers();
        let on_map = self.fingers.remove(&finger).is_some();

        self.reconcile_pair();
        self.reanchor_if_changed(before);

        on_map
    }

    /// The centroid and spread of the fingers driving the gesture.
    fn measure(&self) -> (Option<Point>, Option<f32>) {
        match self.pair {
            Some((a, b)) => {
                let (Some(a), Some(b)) = (self.fingers.get(&a), self.fingers.get(&b)) else {
                    return (None, None);
                };

                let centroid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                (Some(centroid), Some(a.distance(*b)))
            }
            None => (self.fingers.values().next().copied(), None),
        }
    }

    /// The fingers driving the gesture: the pair while it exists, or every
    /// finger otherwise.
    fn gesture_fingers(&self) -> (Option<Finger>, Option<Finger>) {
        match self.pair {
            Some((a, b)) => (Some(a), Some(b)),
            None => (self.fingers.keys().next().copied(), None),
        }
    }

    /// Ensure a pair of fingers drives the gesture whenever two or more are
    /// down, preferring a surviving pair member when the pair breaks.
    fn reconcile_pair(&mut self) {
        self.pair = match self.pair {
            Some((a, b)) if self.fingers.contains_key(&a) && self.fingers.contains_key(&b) => {
                Some((a, b))
            }
            Some((a, b)) => {
                let survivor = [a, b]
                    .into_iter()
                    .find(|finger| self.fingers.contains_key(finger));
                self.latch_pair(survivor)
            }
            None => self.latch_pair(None),
        };
    }

    /// Latch a pair of fingers onto the map, preferring the given finger.
    fn latch_pair(&self, preferred: Option<Finger>) -> Option<(Finger, Finger)> {
        if self.fingers.len() < 2 {
            return None;
        }

        let mut ids: Vec<_> = self.fingers.keys().map(|finger| finger.0).collect();
        ids.sort_unstable();

        // Prefer a surviving pair member, pairing it with another finger
        if let Some(preferred) = preferred.filter(|finger| self.fingers.contains_key(finger)) {
            let other = ids.iter().copied().find(|&id| id != preferred.0)?;
            return Some((preferred, Finger(other)));
        }

        Some((Finger(ids[0]), Finger(ids[1])))
    }

    /// Re-anchor the gesture if the fingers driving it changed, disengaging
    /// it and discarding its history.
    fn reanchor_if_changed(&mut self, before: (Option<Finger>, Option<Finger>)) {
        if before == self.gesture_fingers() {
            return;
        }

        let (centroid, spread) = self.measure();
        self.anchor_centroid = centroid;
        self.anchor_spread = spread;
        self.last_centroid = centroid;
        self.last_spread = spread;
        self.pan_engaged = false;
        self.zoom_engaged = false;
        self.pan_samples.clear();
        self.zoom_samples.clear();
    }

    /// Zoom change between two pinch spreads, in levels. Degenerate spreads
    /// have no meaningful zoom.
    fn pinch_zoom(spread: f32, reference: f32) -> Option<f64> {
        (spread > 0.0 && reference > 0.0).then(|| ((spread / reference) as f64).log2())
    }

    /// Pan momentum of an engaged gesture, measured across its recent history.
    fn pan_momentum(&mut self, now: Instant) -> Option<Vector> {
        if !self.pan_engaged {
            return None;
        }

        Self::prune(&mut self.pan_samples, now);

        let (first, last) = (self.pan_samples.front()?, self.pan_samples.back()?);
        let duration = last.at.duration_since(first.at).as_secs_f32();
        if duration <= 0.0 {
            return None;
        }

        let velocity = (last.value - first.value) / duration;
        let speed = (velocity.x.powi(2) + velocity.y.powi(2)).sqrt();

        (speed > PAN_MOMENTUM_THRESHOLD).then_some(velocity)
    }

    /// Zoom momentum of an engaged pinch, measured across its recent history.
    fn zoom_momentum(&mut self, now: Instant) -> Option<ZoomMomentum> {
        if !self.zoom_engaged {
            return None;
        }

        Self::prune(&mut self.zoom_samples, now);

        let (first, last) = (self.zoom_samples.front()?, self.zoom_samples.back()?);
        let duration = last.at.duration_since(first.at).as_secs_f64();
        if duration <= 0.0 {
            return None;
        }

        let velocity = Self::pinch_zoom(last.value, first.value)? / duration;
        if velocity.abs() > ZOOM_MOMENTUM_THRESHOLD {
            return Some(ZoomMomentum {
                velocity,
                anchor: self.last_centroid?,
            });
        }

        None
    }

    /// Drop all but the most recent sample older than [`MOMENTUM_WINDOW`].
    fn prune<T>(samples: &mut VecDeque<Sample<T>>, now: Instant) {
        while samples.len() > 1 && now.duration_since(samples.front().unwrap().at) > MOMENTUM_WINDOW
        {
            samples.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed clock, so that gesture histories are fully controlled.
    struct Clock {
        start: Instant,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                start: Instant::now(),
            }
        }

        fn at(&self, millis: u64) -> Instant {
            self.start + Duration::from_millis(millis)
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
    }

    #[test]
    fn pan_engages_after_threshold() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        tracker.press(finger, Point::new(100.0, 100.0));

        // Small movements stay within the engagement threshold
        let motion = tracker
            .moved(finger, Point::new(104.0, 100.0), clock.at(16))
            .unwrap();
        assert!(motion.pan.is_none());

        // Moving further engages panning, applying the last movement
        let motion = tracker
            .moved(finger, Point::new(110.0, 100.0), clock.at(32))
            .unwrap();
        let pan = motion.pan.unwrap();
        close(pan.x as f64, 6.0);
        close(pan.y as f64, 0.0);
    }

    #[test]
    fn pan_fling_hands_off_momentum() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        tracker.press(finger, Point::new(100.0, 100.0));
        tracker
            .moved(finger, Point::new(120.0, 100.0), clock.at(16))
            .unwrap();
        tracker
            .moved(finger, Point::new(140.0, 100.0), clock.at(32))
            .unwrap();
        tracker
            .moved(finger, Point::new(160.0, 100.0), clock.at(48))
            .unwrap();

        // Velocity is measured across the momentum window
        let release = tracker.released(finger, clock.at(48)).unwrap();
        let velocity = release.pan.unwrap();
        close(velocity.x as f64, 1250.0);
        close(velocity.y as f64, 0.0);
    }

    #[test]
    fn stale_pan_gains_no_momentum() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        tracker.press(finger, Point::new(100.0, 100.0));
        tracker
            .moved(finger, Point::new(150.0, 100.0), clock.at(16))
            .unwrap();

        // Holding still long enough invalidates the gesture history
        let release = tracker.released(finger, clock.at(300)).unwrap();
        assert!(release.pan.is_none());
    }

    #[test]
    fn unengaged_pan_gains_no_momentum() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        tracker.press(finger, Point::new(100.0, 100.0));

        // Jitter stays within the engagement threshold
        tracker
            .moved(finger, Point::new(103.0, 100.0), clock.at(16))
            .unwrap();
        tracker
            .moved(finger, Point::new(101.0, 100.0), clock.at(32))
            .unwrap();

        let release = tracker.released(finger, clock.at(48)).unwrap();
        assert!(release.pan.is_none());
    }

    #[test]
    fn pinch_zoom_engages_after_threshold() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));

        // Small spread changes stay within the engagement threshold
        let motion = tracker
            .moved(Finger(0), Point::new(101.0, 100.0), clock.at(16))
            .unwrap();
        assert!(motion.zoom.is_none());

        // Pinching out engages zoom, applying the last incremental change
        let motion = tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(32))
            .unwrap();
        let zoom = motion.zoom.unwrap();
        close(zoom.levels, ((220.0f32 / 199.0f32) as f64).log2());
        close(zoom.anchor.x as f64, (80.0 + 300.0) / 2.0);
        close(zoom.anchor.y as f64, 100.0);
    }

    #[test]
    fn pinch_translates_and_zooms_together() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));

        // Pinching out while dragging the pair along
        let motion = tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(16))
            .unwrap();
        let pan = motion.pan.unwrap();
        close(pan.x as f64, -10.0);
        close(pan.y as f64, 0.0);
        let zoom = motion.zoom.unwrap();
        close(zoom.levels, ((220.0f32 / 200.0f32) as f64).log2());

        let motion = tracker
            .moved(Finger(1), Point::new(360.0, 100.0), clock.at(32))
            .unwrap();
        let pan = motion.pan.unwrap();
        close(pan.x as f64, 30.0);
        close(pan.y as f64, 0.0);
        let zoom = motion.zoom.unwrap();
        close(zoom.levels, ((280.0f32 / 220.0f32) as f64).log2());
    }

    #[test]
    fn extra_fingers_are_ignored() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));
        tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(16))
            .unwrap();

        // A third finger joining does not disturb the pinch
        tracker.press(Finger(2), Point::new(500.0, 500.0));
        let motion = tracker
            .moved(Finger(2), Point::new(600.0, 600.0), clock.at(32))
            .unwrap();
        assert!(motion.pan.is_none() && motion.zoom.is_none());

        // A third finger leaving does not re-anchor the gesture
        let release = tracker.released(Finger(2), clock.at(48)).unwrap();
        assert!(release.pan.is_none() && release.zoom.is_none());

        let motion = tracker
            .moved(Finger(1), Point::new(360.0, 100.0), clock.at(64))
            .unwrap();
        let zoom = motion.zoom.unwrap();
        close(zoom.levels, ((280.0f32 / 220.0f32) as f64).log2());
    }

    #[test]
    fn pinch_release_hands_off_zoom_momentum() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));
        tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(16))
            .unwrap();
        tracker
            .moved(Finger(1), Point::new(360.0, 100.0), clock.at(32))
            .unwrap();

        // Lifting one pinch finger ends the pinch and hands off zoom momentum
        let release = tracker.released(Finger(0), clock.at(48)).unwrap();
        let zoom = release.zoom.unwrap();
        close(zoom.velocity, ((280.0f32 / 220.0f32) as f64).log2() / 0.016);
        close(zoom.anchor.x as f64, (80.0 + 360.0) / 2.0);

        // Lifting the last finger hands off no further momentum
        let release = tracker.released(Finger(1), clock.at(64)).unwrap();
        assert!(release.pan.is_none() && release.zoom.is_none());
        assert!(!tracker.is_active());
    }

    #[test]
    fn pan_continues_after_pinch_ends() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));
        tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(16))
            .unwrap();
        tracker
            .moved(Finger(1), Point::new(360.0, 100.0), clock.at(32))
            .unwrap();

        // The pinch ends, but the remaining finger can re-engage panning
        tracker.released(Finger(0), clock.at(48)).unwrap();
        let motion = tracker
            .moved(Finger(1), Point::new(380.0, 120.0), clock.at(64))
            .unwrap();
        let pan = motion.pan.unwrap();
        close(pan.x as f64, 20.0);
        close(pan.y as f64, 20.0);

        // ... and fling the map on release
        tracker
            .moved(Finger(1), Point::new(400.0, 120.0), clock.at(80))
            .unwrap();
        let release = tracker.released(Finger(1), clock.at(96)).unwrap();
        let velocity = release.pan.unwrap();
        close(velocity.x as f64, 1250.0);
        close(velocity.y as f64, 0.0);
    }

    #[test]
    fn pinch_replaces_latched_fingers() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();

        tracker.press(Finger(0), Point::new(100.0, 100.0));
        tracker.press(Finger(1), Point::new(300.0, 100.0));
        tracker.press(Finger(2), Point::new(300.0, 300.0));

        // The pair zooms from its spread
        tracker
            .moved(Finger(0), Point::new(80.0, 100.0), clock.at(16))
            .unwrap();

        // When a pair finger lifts, the surviving member pairs up with the
        // remaining finger, re-anchoring the gesture
        tracker.released(Finger(0), clock.at(32)).unwrap();
        let motion = tracker
            .moved(Finger(1), Point::new(320.0, 100.0), clock.at(48))
            .unwrap();

        // The new pair has just re-anchored, and moved too little to zoom
        assert!(motion.zoom.is_none());
        let pan = motion.pan.unwrap();
        close(pan.x as f64, 10.0);
        close(pan.y as f64, 0.0);
    }

    #[test]
    fn cancelled_gesture_gains_no_momentum() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        tracker.press(finger, Point::new(100.0, 100.0));
        tracker
            .moved(finger, Point::new(150.0, 100.0), clock.at(16))
            .unwrap();

        // A stolen gesture never hands off momentum
        assert!(tracker.cancelled(finger));
        assert!(!tracker.is_active());
        assert!(tracker.released(finger, clock.at(32)).is_none());
    }

    #[test]
    fn unknown_fingers_are_ignored() {
        let clock = Clock::new();
        let mut tracker = GestureTracker::default();
        let finger = Finger(0);

        // Fingers which were never pressed do nothing
        assert!(
            tracker
                .moved(finger, Point::new(100.0, 100.0), clock.at(0))
                .is_none()
        );
        assert!(tracker.released(finger, clock.at(16)).is_none());
        assert!(!tracker.cancelled(finger));
        assert!(!tracker.is_active());
    }
}
