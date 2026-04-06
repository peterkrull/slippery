use core::f32;
use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
    time::{Duration, Instant},
};

use iced::touch::Finger;
use iced_core::{
    Element, Image, Point, Rectangle, Shell, Vector, Widget,
    image::{Allocation, FilterMethod, Handle},
    widget::tree::State,
};

use crate::{
    Projector, Viewpoint, Zoom,
    draw_cache::DrawCache,
    position::Mercator,
    tile_cache::{CacheMessage, TileCache},
    tile_coord::TileCoord,
};

// At zoom level 0, any map provider will take up this many pixels.
pub const BASE_SIZE: u32 = 512;

const TOUCH_SMOOTHING_TAU: f32 = 0.03;
const TOUCH_PAN_MOMENTUM_THRESHOLD: f32 = 10.0;
const TOUCH_ZOOM_MOMENTUM_THRESHOLD: f64 = 0.12;
const TOUCH_PINCH_ZOOM_GAIN: f64 = 1.0;
const TOUCH_PINCH_RELEASE_GRACE: Duration = Duration::from_millis(50);
const TOUCH_MOMENTUM_MAX_GAP: Duration = Duration::from_millis(50);

/// A [slippy tile](https://wiki.openstreetmap.org/wiki/Slippy_map) widget
pub struct MapWidget<'a, Message> {
    tile_cache: &'a TileCache,
    viewpoint: Viewpoint,
    cache_message: fn(CacheMessage) -> Message,
    on_update: Option<Box<dyn Fn(Projector) -> Message + 'a>>,
    discrete_zoom_step_size: f32,
    discrete_zoom_step_duration: Duration,
}

impl<'a, Message> MapWidget<'a, Message> {
    pub fn new(
        tile_cache: &'a TileCache,
        cache_message: fn(CacheMessage) -> Message,
        viewpoint: Viewpoint,
    ) -> Self {
        Self {
            tile_cache,
            viewpoint,
            on_update: None,
            cache_message,
            discrete_zoom_step_size: 1.0,
            discrete_zoom_step_duration: Duration::from_millis(250),
        }
    }

    /// This message is emitted when changing the map viewpoint (position/zoom)
    pub fn on_update(self, func: impl Fn(Projector) -> Message + 'a) -> Self {
        Self {
            on_update: Some(Box::new(func)),
            ..self
        }
    }

    pub fn position_of_tile(&self, projector: &Projector, tile_id: &TileCoord) -> Rectangle {
        let tile_size = self.tile_cache.tile_size() as f64;
        let scale_offset = (BASE_SIZE as f64 / tile_size).log2();

        let scale = 2.0_f64.powf(self.viewpoint.zoom.f64() - tile_id.zoom() as f64);
        let size = (tile_size * 2.0_f64.powf(scale_offset) * scale) as f32;

        let tile_mercator = tile_id.to_mercator();
        let screen_pos = projector.mercator_into_screen_space(tile_mercator);

        Rectangle {
            x: screen_pos.x,
            y: screen_pos.y,
            width: size,
            height: size,
        }
    }

    /// Use [flood fill algorithm](https://en.wikipedia.org/wiki/Flood_fill) to determine
    /// which tiles need to be drawn..
    pub fn flood_tiles(&self, projector: &Projector) -> Vec<(TileCoord, Rectangle)> {
        // Slightly expand the bounds to load in tiles which may be panned to
        let viewport = projector.bounds.expand(32);

        // Allocate for the number of tiles to fill the screen, and then some
        let capacity = viewport.area() / self.tile_cache.tile_size().pow(2) as f32;
        let mut tiles = HashMap::with_capacity(capacity.ceil() as usize);

        // This ensures tilesets of different sizes
        let scale_offset = (BASE_SIZE as f64 / self.tile_cache.tile_size() as f64).log2();

        let scaled_zoom =
            (self.viewpoint.zoom.f64() + scale_offset).min(self.tile_cache.max_zoom() as f64);

        let central_tile_id = self.viewpoint.position.tile_id(scaled_zoom.round() as u8);

        // Recursively fill up the `tiles` map
        self.flood_tiles_inner(projector, &viewport, central_tile_id, &mut tiles);

        // Convert the map into a vec of id-uv pairs
        tiles
            .drain()
            .filter_map(|(id, tile)| tile.map(|tile| (id, tile)))
            .collect()
    }

    fn flood_tiles_inner(
        &self,
        projector: &Projector,
        viewport: &Rectangle,
        tile_id: TileCoord,
        tiles: &mut HashMap<TileCoord, Option<Rectangle>>,
    ) {
        // Return early if this entry has already been checked
        let Entry::Vacant(entry) = tiles.entry(tile_id) else {
            return;
        };

        let rectangle = self.position_of_tile(&projector, &tile_id);

        // Accept the tile if it intersects the viewport
        if viewport.intersects(&rectangle) {
            entry.insert(Some(rectangle));

            // Recurse using all valid neighbors
            for &neigbor_tile_id in tile_id.neighbors().iter().flatten() {
                self.flood_tiles_inner(projector, viewport, neigbor_tile_id, tiles);
            }
        } else {
            entry.insert(None);
        }
    }

    fn fallback_to_children(
        &self,
        old_draw_cache: &mut DrawCache,
        draw_cache: &mut DrawCache,
        tile_id: TileCoord,
        projector: &Projector,
    ) -> bool {
        if let Some(children) = tile_id.children() {
            let mut num_children_available = 0;

            for child_tile_id in &children {
                if let Some((handle, allocation)) =
                    self.get_drawable_tile(old_draw_cache, child_tile_id)
                {
                    let child_rectangle = self.position_of_tile(projector, child_tile_id);
                    draw_cache.insert(*child_tile_id, handle, child_rectangle, allocation);

                    num_children_available += 1;
                }
            }

            // If we found all children, skip parent fallback
            return num_children_available == 4;
        }

        false
    }

    fn fallback_to_ancestor(
        &self,
        old_draw_cache: &mut DrawCache,
        draw_cache: &mut DrawCache,
        tile_id: &TileCoord,
        projector: &Projector,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        // If there is not full child coverage, fall back to a parent tile
        let mut new_tile_id = *tile_id;
        while let Some(parent_tile_id) = new_tile_id.parent() {
            new_tile_id = parent_tile_id;

            // This tile is already set to be drawn
            if draw_cache.contains_key(&new_tile_id) {
                break;
            }

            if let Some((handle, allocation)) = self.get_drawable_tile(old_draw_cache, &new_tile_id)
            {
                let rectangle = self.position_of_tile(&projector, &new_tile_id);
                draw_cache.insert(new_tile_id, handle, rectangle, allocation);
                return true;
            }

            // Ensure the tile is allocated. Even though we are also allocating
            // the intended tile, this should ensure the parent is ready as a backup
            // for other potentially missing tiles as well.
            if self.tile_cache.should_alloc(&new_tile_id) {
                shell.publish((self.cache_message)(CacheMessage::Allocate {
                    id: new_tile_id,
                }))
            }
        }

        false
    }

    fn get_drawable_tile(
        &self,
        old_draw_cache: &mut DrawCache,
        tile_id: &TileCoord,
    ) -> Option<(Handle, Allocation)> {
        old_draw_cache
            .remove(tile_id)
            .or_else(|| self.tile_cache.get_drawable(tile_id))
    }

    fn event_cursor_moved(
        &mut self,
        state: &mut WidgetState,
        needs_redraw: &mut bool,
        projector: &Projector,
        position: &Point,
    ) {
        state.cursor = Some(*position);

        if let PanMove::AutoPan { .. } = state.pan_move {
            *needs_redraw = true;
        }

        if let ZoomMove::AutoZoom { .. } = state.zoom_move {
            *needs_redraw = true;
        }

        if let PanMove::Dragging {
            drag_mercator,
            last_cursor,
            last_time,
            velocity,
            ..
        } = &mut state.pan_move
        {
            if self.on_update.is_some() {
                let cursor_position = projector.screen_space_into_mercator(*position);

                // Add the difference in drag start position and cursor position
                self.viewpoint
                    .position
                    .add_sub(*drag_mercator, cursor_position);

                // Calculate velocity
                let now = Instant::now();
                let delta_time = (now - *last_time).as_secs_f32();
                if delta_time > 0.0 {
                    let delta_pos = *position - *last_cursor;
                    let current_velocity = Vector::new(delta_pos.x, delta_pos.y) / delta_time;

                    // Smooth velocity vector slightly
                    let tau = 0.02;
                    let alpha = tau / (tau + delta_time);
                    *velocity = *velocity * alpha + current_velocity * (1.0 - alpha);

                    *last_cursor = *position;
                    *last_time = now;
                }
            }
        }
    }
}

#[derive(Default)]
struct WidgetState {
    pan_move: PanMove,
    zoom_move: ZoomMove,
    cursor: Option<Point>,
    draw_cache: DrawCache,
    touch: TouchState,
}

#[derive(Default)]
struct TouchState {
    fingers: HashMap<Finger, FingerState>,
    second_finger_left: Option<Instant>,
    last_centroid: Option<Point<f32>>,
    last_pinch_distance: Option<f32>,
    smoothed_pan_velocity: Vector<f32>,
    smoothed_pinch_velocity: f64,
    pinch_release_velocity: Option<f64>,
    pinch_release_point: Option<Mercator>,
    last_motion: Option<Instant>,
}

impl TouchState {
    fn centroid(&self) -> Option<Point<f32>> {
        if self.fingers.is_empty() {
            return None;
        }

        let (sum_x, sum_y) = self.fingers.values().fold((0.0, 0.0), |(sx, sy), finger| {
            (sx + finger.position.x, sy + finger.position.y)
        });

        let count = self.fingers.len() as f32;
        Some(Point::new(sum_x / count, sum_y / count))
    }

    fn average_velocity(&self) -> Option<Vector<f32>> {
        if self.fingers.is_empty() {
            return None;
        }

        let sum_velocity = self
            .fingers
            .values()
            .fold(Vector::ZERO, |accum, finger| accum + finger.velocity);

        Some(sum_velocity / self.fingers.len() as f32)
    }

    fn pinch_distance(&self) -> Option<f32> {
        if self.fingers.len() < 2 {
            return None;
        }

        let centroid = self.centroid()?;
        let radius_sum: f32 = self
            .fingers
            .values()
            .map(|finger| finger.position.distance(centroid))
            .sum();

        Some(radius_sum / self.fingers.len() as f32)
    }

    fn clear_after_release(&mut self) {
        self.second_finger_left = None;
        self.last_centroid = None;
        self.last_pinch_distance = None;
        self.smoothed_pan_velocity = Vector::ZERO;
        self.smoothed_pinch_velocity = 0.0;
        self.pinch_release_velocity = None;
        self.pinch_release_point = None;
        self.last_motion = None;
    }
}

struct FingerState {
    position: Point<f32>,
    velocity: Vector<f32>,
    last_time: Instant,
}

impl FingerState {
    pub fn new(position: Point<f32>) -> Self {
        Self {
            position,
            velocity: Vector::ZERO,
            last_time: Instant::now(),
        }
    }
}

#[derive(Clone, Default)]
enum PanMove {
    #[default]
    Idle,
    Dragging {
        drag_mercator: Mercator,
        last_cursor: iced::Point<f32>,
        last_time: Instant,
        velocity: Vector,
    },
    Momentum {
        velocity: Vector,
        last_time: Instant,
    },
    AutoPan {
        origin: iced::Point,
        last_time: Instant,
    },
}

#[derive(Debug, Default)]
enum ZoomMove {
    #[default]
    Idle,
    Continuous {
        point: Option<Mercator>,
        start_time: Instant,
        start_zoom: f64,
        velocity: f64,
    },
    Discrete {
        point: Option<Mercator>,
        start_time: Instant,
        start_zoom: f64,
        end_zoom: f64,
        duration: Duration,
    },
    AutoZoom {
        origin: iced::Point,
        last_time: Instant,
    },
}

impl WidgetState {
    /// Get a mutable reference to the widget state,
    /// initializing it, if it is not already.
    pub fn get_mut(state: &mut State) -> &mut WidgetState {
        match state {
            State::None => {
                *state = State::new(WidgetState::default());
                state.downcast_mut::<WidgetState>()
            }
            State::Some(any) => any
                .downcast_mut::<WidgetState>()
                .expect("Widget state of incorrect type"),
        }
    }

    pub fn get_ref(state: &State) -> Option<&WidgetState> {
        match state {
            State::None => None,
            State::Some(any) => any.downcast_ref::<WidgetState>(),
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for MapWidget<'a, Message>
where
    Renderer: iced_core::image::Renderer<Handle = Handle>
        + iced_core::Renderer
        + iced_graphics::geometry::Renderer,
{
    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: iced::Length::Fill,
            height: iced::Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut iced_core::widget::Tree,
        _renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        let size = limits.max();

        iced_core::layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        event: &iced::Event,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut iced_core::Shell<'_, Message>,
        _viewport: &iced::Rectangle,
    ) {
        let state = WidgetState::get_mut(&mut tree.state);
        let bounds = layout.bounds();

        // Check if viewpoint or bounds changed since last time
        let mut needs_redraw = false;

        // For doing projections during the update, but also holds some
        // information about the pre-update state of the viewing area.
        let projector = Projector {
            viewpoint: self.viewpoint,
            bounds,
        };

        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(at)) => {
                match &mut state.zoom_move {
                    ZoomMove::Idle => {}
                    ZoomMove::Continuous {
                        point,
                        start_time,
                        start_zoom,
                        velocity,
                    } => {
                        let elapsed = (*at - *start_time).as_secs_f64();
                        let tau = 0.05;

                        // Analytic position: x(t) = x0 + v0 * tau * (1 - e^(-t/tau))
                        let target_zoom =
                            *start_zoom + *velocity * tau * (1.0 - (-elapsed / tau).exp());
                        let zoom_amt = target_zoom - self.viewpoint.zoom.f64();

                        if let Some(position) = point {
                            let position = projector.mercator_into_screen_space(*position);
                            self.viewpoint
                                .zoom_on_point(zoom_amt, position, projector.bounds);
                        } else {
                            self.viewpoint.zoom_on_center(zoom_amt);
                        }

                        // v(t) = v0 * e^(-t/tau)
                        let current_velocity = *velocity * (-elapsed / tau).exp();
                        if current_velocity.abs() < 0.1 {
                            state.zoom_move = ZoomMove::Idle;
                        }

                        needs_redraw = true;
                    }
                    ZoomMove::Discrete {
                        point,
                        start_zoom,
                        end_zoom,
                        start_time,
                        duration,
                    } => {
                        let elapsed = *at - *start_time;
                        let t = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);

                        // Ease out cubic
                        let ease = 1.0 - (1.0 - t).powi(3);

                        let current = *start_zoom + (*end_zoom - *start_zoom) * ease;
                        let zoom_amt = current - self.viewpoint.zoom.f64();

                        if let Some(position) = point {
                            let position = projector.mercator_into_screen_space(*position);
                            self.viewpoint
                                .zoom_on_point(zoom_amt, position, projector.bounds);
                        } else {
                            self.viewpoint.zoom_on_center(zoom_amt);
                        }

                        if t >= 1.0 {
                            state.zoom_move = ZoomMove::Idle;
                        }

                        needs_redraw = true;
                    }
                    ZoomMove::AutoZoom { origin, last_time } => {
                        let now = *at;
                        let delta = (now - *last_time).as_secs_f64();
                        *last_time = now;

                        if let Some(cursor) = state.cursor {
                            let offset = cursor - *origin;
                            let distance = offset.y as f64; // Vertical distance determines speed

                            if distance.abs() > 5.0 {
                                // Scaling factor: 50 pixels = 1 zoom level per second
                                // Up (negative distance) = Zoom In (positive velocity)
                                let velocity = -distance / 50.0;
                                let zoom_change = velocity * delta;

                                self.viewpoint.zoom_on_center(zoom_change);
                                needs_redraw = true;
                            }
                        }
                    }
                }

                if let PanMove::AutoPan { origin, last_time } = &mut state.pan_move {
                    let now = *at;
                    let delta = (now - *last_time).as_secs_f32();
                    *last_time = now;

                    if let Some(cursor) = state.cursor {
                        let offset = cursor - *origin;
                        let velocity = offset * 5.0;

                        if velocity.x.abs() > 1.0 || velocity.y.abs() > 1.0 {
                            let screen_delta = velocity * delta;

                            let current_center = bounds.center();
                            let target_screen = current_center + screen_delta;

                            let center_mercator =
                                projector.screen_space_into_mercator(current_center);
                            let target_mercator =
                                projector.screen_space_into_mercator(target_screen);

                            self.viewpoint
                                .position
                                .add_sub(target_mercator, center_mercator);

                            needs_redraw = true;
                        }
                    }
                }

                if let PanMove::Momentum {
                    velocity,
                    last_time,
                } = &mut state.pan_move
                {
                    let delta = (*at - *last_time).as_secs_f32();
                    *last_time = *at;

                    // Apply velocity offset to viewpoint
                    let screen_delta = *velocity * delta;
                    let current_center = bounds.center();
                    let new_center_screen = current_center - screen_delta;

                    let center_mercator = projector.screen_space_into_mercator(current_center);
                    let target_mercator = projector.screen_space_into_mercator(new_center_screen);

                    self.viewpoint
                        .position
                        .add_sub(target_mercator, center_mercator);

                    // Decay the velocity, less so at higher speeds
                    let norm_velocity = (velocity.x.powi(2) + velocity.y.powi(2)).sqrt();
                    let dynamic_tau = 0.2 + norm_velocity * 0.00005;
                    let alpha = dynamic_tau / (dynamic_tau + delta);
                    *velocity = *velocity * alpha;

                    // Low velocity cutoff to stop the momentum move
                    if norm_velocity < 50.0 {
                        state.pan_move = PanMove::Idle;
                    }

                    needs_redraw = true;
                }
            }
            iced::Event::Touch(event) => match event {
                iced::touch::Event::FingerPressed { id, position } => {
                    if matches!(
                        state.pan_move,
                        PanMove::Momentum { .. } | PanMove::AutoPan { .. }
                    ) {
                        state.pan_move = PanMove::Idle;
                    }

                    if matches!(
                        state.zoom_move,
                        ZoomMove::Continuous { .. }
                            | ZoomMove::Discrete { .. }
                            | ZoomMove::AutoZoom { .. }
                    ) {
                        state.zoom_move = ZoomMove::Idle;
                    }

                    if state.touch.fingers.is_empty() {
                        state.touch.smoothed_pan_velocity = Vector::ZERO;
                        state.touch.smoothed_pinch_velocity = 0.0;
                        state.touch.last_motion = None;
                    }

                    state.touch.fingers.insert(*id, FingerState::new(*position));
                    state.touch.second_finger_left = None;
                    state.touch.pinch_release_velocity = None;
                    state.touch.pinch_release_point = None;
                    state.touch.last_centroid = state.touch.centroid();
                    state.touch.last_pinch_distance = state.touch.pinch_distance();
                    shell.capture_event();
                }
                iced::touch::Event::FingerMoved { id, position } => {
                    let now = Instant::now();
                    let mut delta_time = 0.0f32;

                    match state.touch.fingers.get_mut(&id) {
                        Some(finger_state) => {
                            delta_time = (now - finger_state.last_time).as_secs_f32();
                            let delta_pos = *position - finger_state.position;
                            finger_state.position = *position;
                            finger_state.last_time = now;

                            if delta_time > 0.0 {
                                let raw_velocity = delta_pos / delta_time;
                                let alpha =
                                    TOUCH_SMOOTHING_TAU / (TOUCH_SMOOTHING_TAU + delta_time);
                                finger_state.velocity =
                                    finger_state.velocity * alpha + raw_velocity * (1.0 - alpha);
                            }
                        }
                        None => {
                            log::warn!("FingerMoved event on non-existent finger");
                            state.touch.fingers.insert(*id, FingerState::new(*position));
                        }
                    }

                    if let Some(centroid) = state.touch.centroid() {
                        if let Some(last_centroid) = state.touch.last_centroid {
                            if self.on_update.is_some() {
                                let old_mercator =
                                    projector.screen_space_into_mercator(last_centroid);
                                let new_mercator = projector.screen_space_into_mercator(centroid);
                                self.viewpoint.position.add_sub(old_mercator, new_mercator);
                                needs_redraw = true;
                            }
                        }

                        state.touch.last_centroid = Some(centroid);
                    }

                    if delta_time > 0.0 {
                        state.touch.last_motion = Some(now);

                        if let Some(avg_velocity) = state.touch.average_velocity() {
                            let alpha = TOUCH_SMOOTHING_TAU / (TOUCH_SMOOTHING_TAU + delta_time);
                            state.touch.smoothed_pan_velocity = state.touch.smoothed_pan_velocity
                                * alpha
                                + avg_velocity * (1.0 - alpha);
                        }
                    }

                    if state.touch.fingers.len() >= 2 {
                        if let (Some(centroid), Some(pinch_distance)) =
                            (state.touch.last_centroid, state.touch.pinch_distance())
                        {
                            if let Some(last_distance) = state.touch.last_pinch_distance {
                                if delta_time > 0.0 && last_distance > 0.0 && pinch_distance > 0.0 {
                                    let scale = pinch_distance / last_distance;

                                    if scale.is_finite() && scale > 0.0 {
                                        let zoom_delta =
                                            scale.log2() as f64 * TOUCH_PINCH_ZOOM_GAIN;

                                        if self.on_update.is_some()
                                            && zoom_delta.abs() > f64::EPSILON
                                        {
                                            self.viewpoint.zoom_on_point(
                                                zoom_delta,
                                                centroid,
                                                projector.bounds,
                                            );
                                            needs_redraw = true;
                                        }

                                        let raw_zoom_velocity = zoom_delta / delta_time as f64;
                                        let tau = TOUCH_SMOOTHING_TAU as f64;
                                        let alpha = tau / (tau + delta_time as f64);
                                        state.touch.smoothed_pinch_velocity =
                                            state.touch.smoothed_pinch_velocity * alpha
                                                + raw_zoom_velocity * (1.0 - alpha);
                                    }
                                }
                            }

                            state.touch.last_pinch_distance = Some(pinch_distance);
                            state.touch.pinch_release_point =
                                Some(projector.screen_space_into_mercator(centroid));
                        }
                    } else {
                        state.touch.last_pinch_distance = None;
                        state.touch.smoothed_pinch_velocity = 0.0;
                    }

                    shell.capture_event();
                }
                iced::touch::Event::FingerLifted { id, .. }
                | iced::touch::Event::FingerLost { id, .. } => {
                    let now = Instant::now();

                    if state.touch.fingers.len() >= 2 {
                        state.touch.pinch_release_velocity =
                            Some(state.touch.smoothed_pinch_velocity);

                        if let Some(centroid) = state.touch.centroid() {
                            state.touch.pinch_release_point =
                                Some(projector.screen_space_into_mercator(centroid));
                        }
                    }

                    state.touch.fingers.remove(id);

                    if state.touch.fingers.len() == 1 {
                        state.touch.second_finger_left = Some(now);
                        state.touch.last_centroid = state.touch.centroid();
                        state.touch.last_pinch_distance = None;
                        state.touch.smoothed_pinch_velocity = 0.0;
                    } else if state.touch.fingers.is_empty() {
                        let moved_recently = state
                            .touch
                            .last_motion
                            .is_some_and(|last| now.duration_since(last) <= TOUCH_MOMENTUM_MAX_GAP);

                        if moved_recently
                            && (state.touch.smoothed_pan_velocity.x.abs()
                                > TOUCH_PAN_MOMENTUM_THRESHOLD
                                || state.touch.smoothed_pan_velocity.y.abs()
                                    > TOUCH_PAN_MOMENTUM_THRESHOLD)
                        {
                            state.pan_move = PanMove::Momentum {
                                velocity: state.touch.smoothed_pan_velocity,
                                last_time: now,
                            };
                            needs_redraw = true;
                        }

                        let allow_zoom_momentum =
                            state.touch.second_finger_left.is_some_and(|left| {
                                now.duration_since(left) <= TOUCH_PINCH_RELEASE_GRACE
                            });

                        if moved_recently && allow_zoom_momentum {
                            if let (Some(velocity), Some(point)) = (
                                state.touch.pinch_release_velocity,
                                state.touch.pinch_release_point,
                            ) {
                                if velocity.abs() > TOUCH_ZOOM_MOMENTUM_THRESHOLD {
                                    state.zoom_move = ZoomMove::Continuous {
                                        point: Some(point),
                                        start_time: now,
                                        start_zoom: self.viewpoint.zoom.f64(),
                                        velocity,
                                    };
                                    needs_redraw = true;
                                }
                            }
                        }

                        state.touch.clear_after_release();
                    } else {
                        state.touch.second_finger_left = None;
                        state.touch.last_centroid = state.touch.centroid();
                        state.touch.last_pinch_distance = state.touch.pinch_distance();
                    }

                    shell.capture_event();
                }
            },
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::WheelScrolled { delta } if self.on_update.is_some() => {
                    let point = cursor
                        .position_over(projector.bounds)
                        .map(|p| projector.screen_space_into_mercator(p));

                    if point.is_some() {
                        shell.capture_event();
                    } else {
                        return;
                    }

                    match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => {
                            let current_zoom = self.viewpoint.zoom.f64();
                            let step = self.discrete_zoom_step_size as f64;

                            // Determine target based on current state
                            let target =
                                if let ZoomMove::Discrete { end_zoom, .. } = &state.zoom_move {
                                    *end_zoom + (*y as f64) * step
                                } else {
                                    let nearest = (current_zoom / step).round() * step;
                                    nearest + (*y as f64) * step
                                }
                                .clamp(Zoom::MIN.f64(), Zoom::MAX.f64());

                            state.zoom_move = ZoomMove::Discrete {
                                point,
                                start_zoom: current_zoom,
                                end_zoom: target,
                                start_time: Instant::now(),
                                duration: self.discrete_zoom_step_duration,
                            };
                        }
                        iced::mouse::ScrollDelta::Pixels { y, .. } => {
                            let mut velocity = *y as f64;
                            let now = Instant::now();

                            // Carry over momentum if we were in Continuous mode
                            if let ZoomMove::Continuous {
                                start_time,
                                velocity: old_velocity,
                                ..
                            } = state.zoom_move
                            {
                                let elapsed = (now - start_time).as_secs_f64();
                                let tau = 0.05;
                                let current_velocity = old_velocity * (-elapsed / tau).exp();
                                velocity += current_velocity;
                            }

                            state.zoom_move = ZoomMove::Continuous {
                                point,
                                start_time: now,
                                start_zoom: self.viewpoint.zoom.f64(),
                                velocity,
                            };
                        }
                    }

                    needs_redraw = true;
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) => {
                    match state.pan_move {
                        PanMove::AutoPan { .. } => {
                            state.pan_move = PanMove::Idle;
                            shell.capture_event();
                        }
                        _ => {
                            if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                                state.pan_move = PanMove::Dragging {
                                    drag_mercator: projector
                                        .screen_space_into_mercator(cursor_position),
                                    last_cursor: cursor_position,
                                    last_time: Instant::now(),
                                    velocity: Vector::new(0.0, 0.0),
                                }
                            }
                        }
                    }

                    match state.zoom_move {
                        ZoomMove::AutoZoom { .. } => {
                            state.zoom_move = ZoomMove::Idle;
                            shell.capture_event();
                        }
                        _ => (),
                    }
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Middle) => {
                    match state.zoom_move {
                        ZoomMove::AutoZoom { .. } => {
                            state.zoom_move = ZoomMove::Idle;
                        }
                        _ => {
                            if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                                state.zoom_move = ZoomMove::AutoZoom {
                                    origin: cursor_position,
                                    last_time: Instant::now(),
                                };
                            }
                        }
                    }
                    shell.capture_event();
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Right) => {
                    match state.pan_move {
                        PanMove::AutoPan { .. } => {
                            state.pan_move = PanMove::Idle;
                        }
                        _ => {
                            if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                                state.pan_move = PanMove::AutoPan {
                                    origin: cursor_position,
                                    last_time: Instant::now(),
                                };
                            }
                        }
                    }
                    shell.capture_event();
                }
                iced::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left) => {
                    match state.pan_move {
                        PanMove::Dragging {
                            velocity,
                            last_time,
                            ..
                        } => {
                            if (velocity.x.abs() > 10.0 || velocity.y.abs() > 10.0)
                                && last_time.elapsed().as_millis() < 50
                            {
                                state.pan_move = PanMove::Momentum {
                                    velocity,
                                    last_time: Instant::now(),
                                };
                            } else {
                                state.pan_move = PanMove::Idle;
                            }

                            needs_redraw = true;
                        }
                        _ => (),
                    }
                }
                iced::mouse::Event::CursorMoved { position } => {
                    self.event_cursor_moved(state, &mut needs_redraw, &projector, position);
                }
                iced::mouse::Event::CursorLeft => {
                    state.cursor = None;
                }
                _ => (),
            },
            _ => (),
        }

        let new_projector = Projector {
            viewpoint: self.viewpoint,
            bounds,
        };

        if projector.viewpoint != self.viewpoint || needs_redraw {
            if let Some(on_update) = &self.on_update {
                shell.publish(on_update(new_projector.clone()));
            }
            shell.capture_event();
            shell.request_redraw();
        }

        // TODO: Limit this so that it only runs just before the draw call

        let visible_tiles = self.flood_tiles(&new_projector);

        // Construct vector of tiles that should be fetched
        let mut to_fetch = visible_tiles
            .iter()
            .filter(|(tile_id, _)| self.tile_cache.should_load(&tile_id))
            .collect::<Vec<_>>();

        // Sort them in order of distance to cursor (if available) or viewport center
        to_fetch.sort_by(|(_, rect1), (_, rect2)| {
            let center = state.cursor.unwrap_or_else(|| bounds.center());
            let dist1 = center.distance(rect1.center());
            let dist2 = center.distance(rect2.center());
            dist1.partial_cmp(&dist2).unwrap_or(Ordering::Equal)
        });

        // Enqueue loading of missing tiles with shell
        for (tile_id, _) in to_fetch {
            shell.publish((self.cache_message)(CacheMessage::Load { id: *tile_id }))
        }

        let mut new_draw_cache = DrawCache::new();
        for (tile_id, rectangle) in visible_tiles.into_iter() {
            // Is the desired tile available, then use it.
            if let Some((handle, allocation)) =
                self.get_drawable_tile(&mut state.draw_cache, &tile_id)
            {
                new_draw_cache.insert(tile_id, handle, rectangle, allocation);
                continue;
            }

            // Otherwise, ensure the tile is allocated on the GPU asap!
            if self.tile_cache.should_alloc(&tile_id) {
                shell.publish((self.cache_message)(CacheMessage::Allocate { id: tile_id }))
            }

            // Try to use four children as a fallback (too fine resolution)
            if self.fallback_to_children(
                &mut state.draw_cache,
                &mut new_draw_cache,
                tile_id,
                &projector,
            ) {
                continue;
            }

            // Otherwise find an available ancestor (too course resolution)
            if self.fallback_to_ancestor(
                &mut state.draw_cache,
                &mut new_draw_cache,
                &tile_id,
                &projector,
                shell,
            ) {
                continue;
            }
        }

        // Swap in the new cache, dropping all unused allocations from the old one
        core::mem::swap(&mut new_draw_cache, &mut state.draw_cache);
    }

    fn draw(
        &self,
        tree: &iced_core::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced_core::renderer::Style,
        layout: iced_core::Layout<'_>,
        _cursor: iced_core::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        if let Some(state) = WidgetState::get_ref(&tree.state) {
            renderer.with_layer(layout.bounds(), |renderer| {
                for data in state.draw_cache.iter_tiles() {
                    let image = Image::new(&data.handle).filter_method(FilterMethod::Linear);
                    renderer.draw_image(image, data.rectangle, layout.bounds())
                }
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &iced_core::widget::Tree,
        _layout: iced_core::Layout<'_>,
        _cursor: iced_core::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> iced_core::mouse::Interaction {
        let state = WidgetState::get_ref(&tree.state)
            .expect("mouse_intercation called prior to initializing widget state");

        use iced_core::mouse::Interaction;

        // The dragging pan move takes precedent
        match state.pan_move {
            PanMove::Dragging { .. } => return Interaction::Grabbing,
            PanMove::AutoPan { .. } => return Interaction::Crosshair,
            _ => (),
        };

        // Then zooming should have the appropriate cursor
        match state.zoom_move {
            ZoomMove::AutoZoom { .. } => return Interaction::ResizingVertically,
            ZoomMove::Discrete {
                start_zoom,
                end_zoom,
                ..
            } => {
                return if (start_zoom - end_zoom).is_sign_positive() {
                    Interaction::ZoomOut
                } else {
                    Interaction::ZoomIn
                };
            }
            ZoomMove::Continuous { velocity, .. } => {
                return if velocity.is_sign_positive() {
                    Interaction::ZoomOut
                } else {
                    Interaction::ZoomIn
                };
            }
            _ => (),
        };

        Interaction::Idle
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer: 'a> From<MapWidget<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::image::Renderer<Handle = Handle> + iced_graphics::geometry::Renderer,
{
    fn from(value: MapWidget<'a, Message>) -> Self {
        Self::new(value)
    }
}
