use core::f32;
use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
    time::{Duration, Instant},
};

use iced_core::{
    Element, Point, Rectangle, Vector, Widget,
    image::Handle,
    widget::tree::State,
};
use iced::widget::canvas;

use crate::{
    Projector, Viewpoint, Zoom,
    draw_cache::DrawCache,
    position::Mercator,
    tile_cache::{CacheMessage, TileCache},
    tile_coord::TileCoord,
    vec_norm,
};

// At zoom level 0, any map provider will take up this many pixels.
pub const BASE_SIZE: u32 = 512;

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

    pub fn tile_pixel_layout(&self, tile_id: &TileCoord) -> (Point<f64>, f32) {
        let tile_size = self.tile_cache.tile_size() as f64;
        let scale_offset = (BASE_SIZE as f64 / tile_size).log2();

        let scale = 2.0_f64.powf(self.viewpoint.zoom.f64() - tile_id.zoom() as f64);
        let size = (tile_size * 2.0_f64.powf(scale_offset) * scale) as f32;

        let tl = tile_id.to_mercator();
        (
            tl.into_pixel_space(self.viewpoint.zoom.f64()),
            size,
        )
    }

    /// Use [flood fill algorithm](https://en.wikipedia.org/wiki/Flood_fill) to determine
    /// which tiles need to be drawn..
    pub fn flood_tiles(&self, projector: &Projector) -> Vec<(TileCoord, Point<f64>, f32)> {
        // Project Screen AABB to Pixel Space AABB
        // We use this to test for tile visibility
        let bounds = projector.bounds.expand(32.0);
        let tl = projector.screen_space_into_pixel_space(Point::new(bounds.x, bounds.y));
        let tr = projector.screen_space_into_pixel_space(Point::new(bounds.x + bounds.width, bounds.y));
        let bl = projector.screen_space_into_pixel_space(Point::new(bounds.x, bounds.y + bounds.height));
        let br = projector
            .screen_space_into_pixel_space(Point::new(bounds.x + bounds.width, bounds.y + bounds.height));

        let min_x = tl.x.min(tr.x).min(bl.x).min(br.x);
        let max_x = tl.x.max(tr.x).max(bl.x).max(br.x);
        let min_y = tl.y.min(tr.y).min(bl.y).min(br.y);
        let max_y = tl.y.max(tr.y).max(bl.y).max(br.y);

        let pixel_viewport = (min_x, min_y, max_x, max_y);

        // Allocate for the number of tiles to fill the screen, and then some
        let capacity = bounds.area() / self.tile_cache.tile_size().pow(2) as f32;
        let mut tiles = HashMap::with_capacity(capacity.ceil() as usize);

        // This ensures tilesets of different sizes
        let scale_offset = (BASE_SIZE as f64 / self.tile_cache.tile_size() as f64).log2();

        let scaled_zoom =
            (self.viewpoint.zoom.f64() + scale_offset).min(self.tile_cache.max_zoom() as f64);

        let central_tile_id = self.viewpoint.position.tile_id(scaled_zoom.round() as u8);

        // Recursively fill up the `tiles` map
        self.flood_tiles_inner(projector, pixel_viewport, central_tile_id, &mut tiles);

        // Convert the map into a vec of id-uv pairs
        tiles
            .drain()
            .filter_map(|(id, tile)| tile.map(|(pt, sz)| (id, pt, sz)))
            .collect()
    }

    fn flood_tiles_inner(
        &self,
        projector: &Projector,
        viewport: (f64, f64, f64, f64),
        tile_id: TileCoord,
        tiles: &mut HashMap<TileCoord, Option<(Point<f64>, f32)>>,
    ) {
        // Return early if this entry has already been checked
        let Entry::Vacant(entry) = tiles.entry(tile_id) else {
            return;
        };

        let (top_left, size) = self.tile_pixel_layout(&tile_id);
        let half_size = (size / 2.0) as f64;
        let center = top_left + Vector::new(half_size, half_size);

        // Tile AABB in Pixel Space
        let t_min_x = center.x - half_size;
        let t_max_x = center.x + half_size;
        let t_min_y = center.y - half_size;
        let t_max_y = center.y + half_size;
        
        // Intersection check
        let (v_min_x, v_min_y, v_max_x, v_max_y) = viewport;
        let intersects = t_min_x < v_max_x
            && t_max_x > v_min_x
            && t_min_y < v_max_y
            && t_max_y > v_min_y;

        // Accept the tile if it intersects the viewport
        if intersects {
            entry.insert(Some((center, size)));

            // Recurse using all valid neighbors
            for &neigbor_tile_id in tile_id.neighbors().iter().flatten() {
                self.flood_tiles_inner(projector, viewport, neigbor_tile_id, tiles);
            }
        } else {
            entry.insert(None);
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
}

#[derive(Clone, Default)]
enum RotationMove {
    #[default]
    Idle,
    Rotating {
        last_cursor_x: f32,
    },
}

struct WidgetState {
    pan_move: PanMove,
    zoom_move: ZoomMove,
    rotation_move: RotationMove,
    cursor: Option<Point>,
    draw_cache: DrawCache,
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            pan_move: Default::default(),
            zoom_move: Default::default(),
            rotation_move: Default::default(),
            cursor: None,
            draw_cache: Default::default(),
        }
    }
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
}

impl WidgetState {
    /// Get a mutable reference to the widget state,
    /// initializing it, if it is not already.
    pub fn get_mut(state: &mut State) -> &mut Self {
        match state {
            State::None => {
                *state = State::new(Self::default());
                state.downcast_mut::<Self>()
            }
            State::Some(any) => any
                .downcast_mut::<Self>()
                .expect("Widget state of incorrect type"),
        }
    }

    pub fn get_ref(state: &State) -> Option<&Self> {
        match state {
            State::None => None,
            State::Some(any) => any.downcast_ref::<Self>(),
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for MapWidget<'a, Message>
where
    Renderer: iced_core::image::Renderer<Handle = Handle>
        + iced_core::Renderer
        + iced_graphics::geometry::Renderer
        + 'static,
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
        _clipboard: &mut dyn iced_core::Clipboard,
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
            cursor: state.cursor,
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
                    let norm_velocity = vec_norm(velocity);
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
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Right) => {
                    if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                        shell.capture_event();
                        state.rotation_move = RotationMove::Rotating {
                            last_cursor_x: cursor_position.x,
                        };
                    }
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                        state.pan_move = PanMove::Dragging {
                            drag_mercator: projector.screen_space_into_mercator(cursor_position),
                            last_cursor: cursor_position,
                            last_time: Instant::now(),
                            velocity: Vector::new(0.0, 0.0),
                        }
                    }
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
                iced::mouse::Event::ButtonReleased(iced_core::mouse::Button::Right) => {
                    match state.rotation_move {
                        RotationMove::Rotating { .. } => {
                            state.rotation_move = RotationMove::Idle;
                            shell.capture_event();
                        }
                        _ => (),
                    }
                }
                iced::mouse::Event::CursorMoved { position } => {
                    state.cursor = Some(*position);

                    if let RotationMove::Rotating { last_cursor_x } = &mut state.rotation_move {
                        if self.on_update.is_some() {
                            let delta = position.x - *last_cursor_x;

                            // Sensitivity: 100 pixels = 45 degrees
                            let sensitivity = 45.0f64.to_radians() / 200.0;
                            self.viewpoint.rotation -= delta as f64 * sensitivity;

                            *last_cursor_x = position.x;
                            needs_redraw = true;
                        }
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
                                let current_velocity =
                                    Vector::new(delta_pos.x, delta_pos.y) / delta_time;

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
                iced::mouse::Event::CursorLeft => {
                    state.cursor = None;
                }
                _ => (),
            },
            _ => (),
        }

        let new_projector = Projector {
            viewpoint: self.viewpoint,
            cursor: state.cursor,
            bounds,
        };

        if self.viewpoint != projector.viewpoint {
            if let Some(on_update) = &self.on_update {
                shell.publish(on_update(new_projector.clone()));
            }
            shell.capture_event();
        }

        if needs_redraw {
            shell.request_redraw();
        }

        // TODO: Limit this so that it only runs just before the draw call

        let visible_tiles = self.flood_tiles(&new_projector);

        // Construct vector of tiles that should be fetched
        let mut to_fetch = visible_tiles
            .iter()
            .filter(|(tile_id, _, _)| self.tile_cache.should_load(&tile_id))
            .collect::<Vec<_>>();

        // Sort them in order of distance to cursor (if available) or viewport center
        let ref_point_screen = state.cursor.unwrap_or_else(|| bounds.center());
        let ref_point_pixel = new_projector.screen_space_into_pixel_space(ref_point_screen);
        to_fetch.sort_by(|(_, center1, _), (_, center2, _)| {
            let dist1 = ref_point_pixel.distance(*center1);
            let dist2 = ref_point_pixel.distance(*center2);
            dist1.partial_cmp(&dist2).unwrap_or(Ordering::Equal)
        });

        // Enqueue loading of missing tiles with shell
        for (tile_id, _, _) in to_fetch {
            shell.publish((self.cache_message)(CacheMessage::Load {
                id: *tile_id,
            }))
        }

        let mut new_draw_cache = DrawCache::new();

        // Helper closure to consolidate sourcing
        let mut get_drawable = |tile_id: &TileCoord| {
            if let Some(res) = state.draw_cache.remove(tile_id) {
                return Some(res);
            }
            if let Some(res) = self.tile_cache.get_drawable(tile_id) {
                return Some(res); // New tile found
            }
            None
        };

        for (tile_id, center, size) in visible_tiles.into_iter() {
            // Is the desired tile available, then use it.
            if let Some((handle, allocation)) = get_drawable(&tile_id) {
                new_draw_cache.insert(tile_id, handle, center, size, allocation);
                continue;
            }

            // Otherwise, ensure the tile is allocated on the GPU asap!
            if self.tile_cache.should_alloc(&tile_id) {
                shell.publish((self.cache_message)(CacheMessage::Allocate {
                    id: tile_id,
                }))
            }

            // Try to use four children as a fallback (too fine resolution)
            let mut has_children = false;
            if let Some(children) = tile_id.children() {
                let mut num_children_available = 0;

                for child_tile_id in &children {
                    if let Some((handle, allocation)) = get_drawable(child_tile_id) {
                        let (child_tl, child_size) = self.tile_pixel_layout(child_tile_id);
                        let child_center = child_tl + Vector::new(child_size as f64 / 2.0, child_size as f64 / 2.0);
                        new_draw_cache.insert(
                            *child_tile_id,
                            handle,
                            child_center,
                            child_size,
                            allocation,
                        );

                        num_children_available += 1;
                    }
                }

                if num_children_available == 4 {
                    has_children = true;
                }
            }

            if has_children {
                continue;
            }

            // Otherwise find an available ancestor (too course resolution)
            let mut parent_tile_id = tile_id;
            while let Some(pid) = parent_tile_id.parent() {
                parent_tile_id = pid;

                // This tile is already set to be drawn
                if new_draw_cache.contains_key(&parent_tile_id) {
                    break;
                }

                if let Some((handle, allocation)) = get_drawable(&parent_tile_id) {
                    let (tl, size) = self.tile_pixel_layout(&parent_tile_id);
                    let center = tl + Vector::new(size as f64 / 2.0, size as f64 / 2.0);
                    new_draw_cache.insert(parent_tile_id, handle, center, size, allocation);
                    break;
                }

                // Ensure the tile is allocated
                if self.tile_cache.should_alloc(&parent_tile_id) {
                    shell.publish((self.cache_message)(CacheMessage::Allocate {
                        id: parent_tile_id,
                    }))
                }
            }
        }

        if new_draw_cache != state.draw_cache {
            shell.request_redraw();
        }

        // Swap in the new cache, dropping all unused allocations from the old one
        core::mem::swap(&mut new_draw_cache, &mut state.draw_cache);

        let num_tiles = state.draw_cache.iter_tiles().count();
        if num_tiles > 100 {
            println!("Drawing {num_tiles} tiles")
        }
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
        let state = WidgetState::get_ref(&tree.state)
            .expect("draw called prior to initializing widget state");

        let bounds = layout.bounds();

        // Projector for this frame
        let projector = Projector {
            viewpoint: self.viewpoint,
            cursor: state.cursor,
            bounds,
        };

        let mut frame = canvas::Frame::new(renderer, bounds.size());

        renderer.with_layer(bounds, |renderer|{

            for data in state.draw_cache.iter_tiles() {
                let center = data.center;
                let size = data.size;

                let half_size = size / 2.0;

                // Project Pixel Space -> Screen Space
                let screen_pos = projector.pixel_space_into_screen_space(center);

                let relative_x = screen_pos.x - bounds.x;
                let relative_y = screen_pos.y - bounds.y;
                let relative_pos = Point::new(relative_x, relative_y);

                let dest = Rectangle::new(
                        relative_pos - Vector::new(half_size, half_size),
                        iced::Size::new(size, size),
                    ).expand(1e-2);
                let image = canvas::Image::new(data.handle.clone())
                    .rotation(-self.viewpoint.rotation as f32);

                frame.draw_image(dest, image);
            }

            let geometry = frame.into_geometry();
            let translation = Vector::new(bounds.x, bounds.y);

            renderer.with_translation(translation, |renderer| {
                renderer.draw_geometry(geometry);
            });
        });

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
            _ => (),
        };

        if let RotationMove::Rotating { .. } = state.rotation_move {
            return Interaction::Grabbing;
        }

        // Then zooming should have the appropriate cursor
        match state.zoom_move {
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
    Renderer: iced_core::image::Renderer<Handle = Handle> + iced_graphics::geometry::Renderer + 'static,
{
    fn from(value: MapWidget<'a, Message>) -> Self {
        Self::new(value)
    }
}
