use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
    time::{Duration, Instant},
};

use iced::{Element, Point, Rectangle, Vector};
use iced_core::{
    Image, Widget,
    image::{FilterMethod, Handle},
    widget::tree::State,
};

use crate::{
    Projector, Viewpoint,
    draw_cache::DrawCache,
    position::{Geographic, Mercator},
    tile_cache::{CacheMessage, TileCache},
    tile_coord::TileCoord,
};

// At zoom level 0, any map provider will take up this many pixels.
pub const BASE_SIZE: u32 = 512;

/// A [slippy tile](https://wiki.openstreetmap.org/wiki/Slippy_map) widget
pub struct MapWidget<'a, Message, Theme, Renderer> {
    tile_cache: &'a TileCache,
    viewpoint: Viewpoint,
    visible_tiles: Vec<(TileCoord, Rectangle)>,
    mapper: fn(CacheMessage) -> Message,
    children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    on_update: Option<fn(Projector) -> Message>,
    on_click: Option<fn(Geographic) -> Message>,
    discrete_zoom_step_size: f32,
}

impl<'a, Message, Theme, Renderer> MapWidget<'a, Message, Theme, Renderer> {
    pub fn new(
        map: &'a TileCache,
        mapper: fn(CacheMessage) -> Message,
        position: Viewpoint,
    ) -> Self {
        Self {
            tile_cache: map,
            viewpoint: position,
            visible_tiles: Vec::new(),
            children: Vec::new(),
            on_update: None,
            on_click: None,
            mapper,
            discrete_zoom_step_size: 1.0,
        }
    }

    /// This message is emitted when changing the map viewpoint (position/zoom)
    pub fn on_update(self, func: fn(Projector) -> Message) -> Self {
        Self {
            on_update: Some(func),
            ..self
        }
    }

    /// This message is emitted when a location is left-clicked
    pub fn on_click(self, func: fn(Geographic) -> Message) -> Self {
        Self {
            on_click: Some(func),
            ..self
        }
    }

    /// Draw a bunch of globally placed elements
    pub fn with_children(
        self,
        children: impl IntoIterator<Item = GlobalElement<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            children: children.into_iter().collect(),
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
    pub fn flood_tiles(&self, viewport: &Rectangle, projector: &Projector) -> Vec<(TileCoord, Rectangle)> {
        // Allocate for the number of tiles to fill the screen, and then some
        let capacity = viewport.area() / (BASE_SIZE * BASE_SIZE) as f32;
        let mut tiles = HashMap::with_capacity(capacity.ceil() as usize);

        let scale_offset = (BASE_SIZE as f64 / self.tile_cache.tile_size() as f64).log2();

        let scaled_zoom = (self.viewpoint.zoom.f64() + scale_offset)
            .min(self.tile_cache.max_zoom() as f64);

        let central_tile_id = self.viewpoint.position.tile_id(scaled_zoom.round() as u8);

        // Recursively fill up the `tiles` map
        self.flood_tiles_inner(projector, viewport, central_tile_id, &mut tiles);

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
}

#[derive(Clone, Default)]
enum Movement {
    #[default]
    Idle,
    Dragging {
        mercator: Mercator,
        start_cursor: iced::Point<f32>,
        last_cursor: iced::Point<f32>,
        last_time: Instant,
        velocity: Vector,
    },
    Momentum {
        velocity: Vector,
        last_time: Instant,
    },
}

#[derive(Default)]
struct WidgetState {
    projector: Option<Projector>,
    movement: Movement,
    cursor: Option<Point>,
    zoom: Option<ZoomState>,
    draw_cache: DrawCache,
}

#[derive(Debug)]

enum ZoomState {
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

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MapWidget<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::image::Renderer<Handle = Handle> + iced_core::Renderer,
{
    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: iced::Length::Fill,
            height: iced::Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        let state = WidgetState::get_mut(&mut tree.state);
        let size = limits.max();

        let projector = Projector {
            viewpoint: self.viewpoint,
            cursor: state.cursor,
            bounds: Rectangle {
                x: 0.0,
                y: 0.0,
                width: size.width,
                height: size.height,
            },
        };

        let children = self
            .children
            .iter_mut()
            .enumerate()
            .map(|(index, child)| {
                let inner_widget = child.element.as_widget_mut();

                let position = projector.mercator_into_screen_space(child.position.as_mercator());

                let size = inner_widget
                    .layout(&mut tree.children[index], renderer, limits)
                    .size();

                inner_widget
                    .layout(&mut tree.children[index], renderer, limits)
                    .move_to(Point {
                        x: position.x - size.width / 2.0,
                        y: position.y - size.height / 2.0,
                    })
            })
            .collect();

        iced_core::layout::Node::with_children(size, children)
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
        let prev_projector = state.projector.clone();

        // Check if viewpoint or bounds changed since last time
        let mut needs_redraw = false;
        if let Some(prev_projector) = &prev_projector {
            if prev_projector.viewpoint != self.viewpoint || prev_projector.bounds != bounds {
                needs_redraw = true;
            }
        } else {
            // First run
            needs_redraw = true;
        }

        // For doing projections during the update, but also holds some
        // information about the pre-update state of the viewing area.
        let projector = state.projector.insert(Projector {
            viewpoint: self.viewpoint,
            cursor: state.cursor,
            bounds,
        });

        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(at)) => {
                if let Some(zoom) = state.zoom.as_mut() {
                    match zoom {
                        ZoomState::Continuous {
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
                                state.zoom = None;
                            }
                        }
                        ZoomState::Discrete {
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
                                state.zoom = None;
                            }
                        }
                    }

                    needs_redraw = true;
                }

                if let Movement::Momentum {
                    velocity,
                    last_time,
                } = &mut state.movement
                {
                    let delta = (*at - *last_time).as_secs_f32();
                    *last_time = *at;

                    // Apply velocity offset to viewpoint
                    let screen_delta = *velocity * delta;
                    let current_center = bounds.center();
                    let new_center_screen = current_center - screen_delta;

                    let center_mercator = projector.mercator_from_screen_space(current_center);
                    let target_mercator = projector.mercator_from_screen_space(new_center_screen);

                    let mercator_delta = target_mercator - center_mercator;

                    self.viewpoint.position = self.viewpoint.position + mercator_delta;

                    // Decay the velocity, less so at higher speeds
                    let norm_velocity = (velocity.x.powi(2) + velocity.y.powi(2)).sqrt();
                    let dynamic_tau = 0.2 + norm_velocity * 0.00005;
                    let alpha = dynamic_tau / (dynamic_tau + delta);
                    *velocity = *velocity * alpha;

                    // Low velocity cutoff to stop the momentum move
                    if velocity.x.abs() < 60.0 && velocity.y.abs() < 60.0 {
                        state.movement = Movement::Idle;
                    }

                    needs_redraw = true;
                }
            }
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::WheelScrolled { delta } if self.on_update.is_some() => {
                    let point = cursor
                        .position_over(projector.bounds)
                        .map(|p| projector.mercator_from_screen_space(p));

                    match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => {
                            let current_zoom = self.viewpoint.zoom.f64();
                            let step = self.discrete_zoom_step_size as f64;

                            // Determine target based on current state
                            let target =
                                if let Some(ZoomState::Discrete { end_zoom, .. }) = &state.zoom {
                                    // If already animating, add to the end target
                                    *end_zoom + (*y as f64) * step
                                } else {
                                    // Otherwise snap to next integer
                                    let nearest = current_zoom.round();
                                    if (nearest - current_zoom).abs() < step / 0.1 {
                                        nearest + (*y as f64) * step
                                    } else {
                                        if *y > 0.0 {
                                            current_zoom.floor() + step
                                        } else {
                                            current_zoom.ceil() - step
                                        }
                                    }
                                };

                            state.zoom = Some(ZoomState::Discrete {
                                point,
                                start_zoom: current_zoom,
                                end_zoom: target,
                                start_time: Instant::now(),
                                duration: Duration::from_millis(250),
                            });
                        }
                        iced::mouse::ScrollDelta::Pixels { y, .. } => {
                            let amount = *y as f64;
                            let now = Instant::now();

                            let mut velocity = amount;

                            // Carry over momentum if we were in Continuous mode
                            if let Some(ZoomState::Continuous {
                                start_time,
                                velocity: old_velocity,
                                ..
                            }) = state.zoom
                            {
                                let elapsed = (now - start_time).as_secs_f64();
                                let tau = 0.05;
                                let current_velocity = old_velocity * (-elapsed / tau).exp();
                                velocity += current_velocity;
                            }

                            state.zoom = Some(ZoomState::Continuous {
                                point,
                                start_time: now,
                                start_zoom: self.viewpoint.zoom.f64(),
                                velocity,
                            });
                        }
                    }

                    needs_redraw = true;
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_over(projector.bounds) {
                        state.movement = Movement::Dragging {
                            mercator: projector.mercator_from_screen_space(cursor_position),
                            start_cursor: cursor_position,
                            last_cursor: cursor_position,
                            last_time: Instant::now(),
                            velocity: Vector::new(0.0, 0.0),
                        }
                    }
                }
                iced::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left) => {
                    match state.movement {
                        Movement::Dragging {
                            start_cursor,
                            velocity,
                            last_time,
                            ..
                        } => {
                            if let Some(cursor_position) = projector.cursor {
                                if let Some(on_clicked) = self.on_click {
                                    if start_cursor == cursor_position {
                                        let position =
                                            projector.mercator_from_screen_space(cursor_position);

                                        shell.publish(on_clicked(position.as_geographic()));
                                    }
                                }
                            }

                            if (velocity.x.abs() > 10.0 || velocity.y.abs() > 10.0)
                                && last_time.elapsed().as_millis() < 50
                            {
                                state.movement = Movement::Momentum {
                                    velocity,
                                    last_time: Instant::now(),
                                };
                                shell.request_redraw();
                            } else {
                                state.movement = Movement::Idle;
                            }
                        }
                        _ => (),
                    }
                }
                iced::mouse::Event::CursorMoved { position } => {
                    state.cursor = Some(*position);

                    if let Movement::Dragging {
                        mercator,
                        last_cursor,
                        last_time,
                        velocity,
                        ..
                    } = &mut state.movement
                    {
                        if self.on_update.is_some() {
                            let cursor_position = projector.mercator_from_screen_space(*position);
                            let mercator_delta = *mercator - cursor_position;
                            self.viewpoint.position = self.viewpoint.position + mercator_delta;

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

                            needs_redraw = true;
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


        if let Some(on_update) = self.on_update {
            let new_projector = Projector {
                viewpoint: self.viewpoint,
                cursor: state.cursor,
                bounds,
            };

            let should_publish = match &prev_projector {
                Some(prev) => *prev != new_projector,
                None => true,
            };

            if should_publish {
                shell.publish(on_update(new_projector.clone()));
            }

            *projector = new_projector;
        }

        // Ensure visible tiles are calculated
        if self.visible_tiles.is_empty() {
            let flood_area = bounds.expand(32);
            self.visible_tiles = self.flood_tiles(&flood_area, projector);
        }

        if needs_redraw {
            shell.capture_event();
            shell.request_redraw();
        }

        // Construct vector of tiles that should be fetched
        let mut to_fetch = self
            .visible_tiles
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
            shell.publish((self.mapper)(CacheMessage::LoadTile { id: *tile_id }))
        }

        let mut draw_cache = DrawCache::new();
        for (tile_id, rectangle) in self.visible_tiles.iter().cloned() {
            // Helper: try to get tile from previous draw cache, otherwise check tile cache
            let get_tile = |draw_cache: &mut DrawCache, tile_id: &TileCoord| {
                draw_cache
                    .remove(tile_id)
                    .or_else(|| self.tile_cache.get_drawable(tile_id))
            };

            // Is the desired tile available, then use it.
            if let Some((handle, allocation)) = get_tile(&mut state.draw_cache, &tile_id) {
                draw_cache.insert(
                    tile_id,
                    handle,
                    rectangle,
                    allocation,
                );
                continue;
            }

            // Otherwise, ensure the tile is allocated asap!
            if self.tile_cache.should_alloc(&tile_id) {
                shell.publish((self.mapper)(CacheMessage::AllocateTile { id: tile_id }))
            }

            // Try to use all 4 immediate children as a fallback
            if let Some(children) = tile_id.children() {
                let mut num_children_available = 0;

                for child_tile_id in &children {

                    // This tile is already set to be drawn
                    if draw_cache.contains_key(child_tile_id) {
                        num_children_available += 1;
                        continue
                    }

                    if let Some((handle, allocation)) = get_tile(&mut state.draw_cache, child_tile_id) {
                        let child_rectangle = self.position_of_tile(&projector, &child_tile_id);
                        draw_cache.insert(
                            child_tile_id.clone(),
                            handle,
                            child_rectangle,
                            allocation,
                        );

                        num_children_available += 1;
                        continue
                    }
                }

                // If we found all children, skip parent fallback
                if num_children_available == 4 {
                    continue;
                }
            }

            // If there is not full child coverage, fall back to a parent tile
            let mut new_tile_id = tile_id;
            while let Some(parent_tile_id) = new_tile_id.parent() {
                new_tile_id = parent_tile_id;

                // This tile is already set to be drawn
                if draw_cache.contains_key(&new_tile_id) {
                    break;
                }

                if let Some((handle, allocation)) = get_tile(&mut state.draw_cache, &new_tile_id) {
                    let rectangle = self.position_of_tile(&projector, &new_tile_id);
                    draw_cache.insert(
                        new_tile_id,
                        handle,
                        rectangle,
                        allocation,
                    );
                    break;
                }

                // Ensure the tile is allocated asap. Even though we are also allocating
                // the intended tile, this should ensure the parent is ready as a backup
                // for other potentially missing tiles as well.
                if self.tile_cache.should_alloc(&new_tile_id) {
                    shell.publish((self.mapper)(CacheMessage::AllocateTile {
                        id: new_tile_id,
                    }))
                }
            }
        }

        core::mem::swap(&mut draw_cache, &mut state.draw_cache);
    }

    fn draw(
        &self,
        tree: &iced_core::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced_core::renderer::Style,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        let Some(state) = WidgetState::get_ref(&tree.state) else {
            return;
        };

        let bounds = layout.bounds();

        // Create new layer to ensure tiles are clipped,
        // and draw tiles in order of zoom level (lowest first)
        renderer.with_layer(bounds, |renderer| {
            for data in state.draw_cache.iter_tiles() {
                // Draw tiles that are true-sized in a separate pass later.
                // Seemingly Iced (or WGPU) does not respect draw order when mixing filter methods
                if (data.rectangle.width - self.tile_cache.tile_size() as f32).abs() < 0.01 {
                    continue;
                }

                let rect = data.rectangle.expand(0.002);

                let image = Image::new(&data.handle)
                    .snap(false)
                    .filter_method(FilterMethod::Linear);
                renderer.draw_image(image, rect, rect)
            }
        });

        renderer.with_layer(bounds, |renderer| {
            for data in state.draw_cache.iter_tiles() {
                // These images were drawn in the previous pass
                if (data.rectangle.width - self.tile_cache.tile_size() as f32).abs() >= 0.01 {
                    continue;
                }

                let rect = data.rectangle.expand(0.002);

                let image = Image::new(&data.handle)
                    .snap(true)
                    .filter_method(FilterMethod::Nearest);
                renderer.draw_image(image, rect, rect)
            }
        });

        // Draw children -> GlobalElement
        renderer.with_layer(bounds, |renderer| {
            self.children
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
                .for_each(|((global_element, tree), layout)| {
                    global_element
                        .element
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, &bounds);
                });
        });
    }

    fn children(&self) -> Vec<iced_core::widget::Tree> {
        self.children
            .iter()
            .map(|child| iced_core::widget::Tree::new(child.element.as_widget()))
            .collect()
    }

    fn diff(&self, tree: &mut iced_core::widget::Tree) {
        let children: Vec<_> = self
            .children
            .iter()
            .map(|child| child.element.as_widget())
            .collect();
        tree.diff_children(children.as_slice());
    }

    fn mouse_interaction(
        &self,
        tree: &iced_core::widget::Tree,
        _layout: iced_core::Layout<'_>,
        _cursor: iced_core::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> iced_core::mouse::Interaction {
        let state = match &tree.state {
            State::Some(any) => any
                .downcast_ref::<WidgetState>()
                .expect("Downcast widget state"),
            _ => return iced_core::mouse::Interaction::Idle,
        };

        match state.movement {
            Movement::Idle => iced_core::mouse::Interaction::Idle,
            Movement::Dragging { .. } => iced_core::mouse::Interaction::Grabbing,
            Movement::Momentum { .. } => iced_core::mouse::Interaction::Idle,
        }
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer: 'a> From<MapWidget<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::image::Renderer<Handle = Handle>,
{
    fn from(value: MapWidget<'a, Message, Theme, Renderer>) -> Self {
        Self::new(value)
    }
}

/// Like a regular [`Element`] but tied to a specific [`Geographic`] coordinate
pub struct GlobalElement<'a, Message, Theme, Renderer> {
    pub element: Element<'a, Message, Theme, Renderer>,
    pub position: Geographic,
}
