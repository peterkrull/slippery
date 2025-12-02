use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
    time::Instant,
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
    map: &'a TileCache,
    viewpoint: Viewpoint,
    visible_tiles: Vec<(TileCoord, Rectangle)>,
    mapper: fn(CacheMessage) -> Message,
    children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    on_update: Option<fn(Projector) -> Message>,
    on_click: Option<fn(Geographic) -> Message>,
}

impl<'a, Message, Theme, Renderer> MapWidget<'a, Message, Theme, Renderer> {
    pub fn new(
        map: &'a TileCache,
        mapper: fn(CacheMessage) -> Message,
        position: Viewpoint,
    ) -> Self {
        Self {
            map,
            viewpoint: position,
            visible_tiles: Vec::new(),
            children: Vec::new(),
            on_update: None,
            on_click: None,
            mapper,
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

    /// Use [flood fill algorithm](https://en.wikipedia.org/wiki/Flood_fill) to determine
    /// which tiles need to be drawn..
    pub fn flood_tiles(&self, viewport: &Rectangle) -> Vec<(TileCoord, Rectangle)> {
        // Allocate for the number of tiles to fill the screen, and then some
        let capacity = viewport.area() / (BASE_SIZE * BASE_SIZE) as f32;
        let mut tiles = HashMap::with_capacity(capacity.ceil() as usize);

        let scale_offset = (BASE_SIZE as f64 / self.map.tile_size() as f64).log2();

        let scaled_zoom = self.viewpoint.zoom.f64().min(self.map.max_zoom() as f64) + scale_offset;

        // TODO This goofs up when zooming out far enough
        let corrected_tile_size =
            BASE_SIZE as f64 * 2f64.powf(self.viewpoint.zoom.f64() - scaled_zoom.round());

        let central_tile_id = self.viewpoint.position.tile_id(scaled_zoom.round() as u8);

        // Recursively fill up the `tiles` map
        self.flood_tiles_inner(viewport, central_tile_id, corrected_tile_size, &mut tiles);

        // Convert the map into a vec of id-uv pairs
        tiles
            .drain()
            .filter_map(|(id, tile)| tile.map(|tile| (id, tile)))
            .collect()
    }

    fn flood_tiles_inner(
        &self,
        viewport: &Rectangle,
        tile_id: TileCoord,
        corrected_tile_size: f64,
        tiles: &mut HashMap<TileCoord, Option<Rectangle>>,
    ) {
        // Return early if this entry has already been checked
        let Entry::Vacant(entry) = tiles.entry(tile_id) else {
            return;
        };

        // Determine the offset of this tile relative to the viewport center
        let projector = Projector {
            viewpoint: self.viewpoint,
            cursor: None,
            bounds: *viewport,
        };

        let tile_mercator = tile_id.to_mercator();
        let screen_pos = projector.mercator_into_screen_space(tile_mercator);

        let projected_position = Rectangle {
            x: screen_pos.x,
            y: screen_pos.y,
            width: corrected_tile_size as f32,
            height: corrected_tile_size as f32,
        };

        // TODO: Hacky fix for sub-pixel misalignment between tiles
        let projected_position = projected_position.expand(0.001);

        // Accept the tile if it intersects the viewport
        if viewport.intersects(&projected_position) {
            entry.insert(Some(projected_position));
        } else {
            entry.insert(None);
            return;
        }

        // Recurse using all valid neighbors
        for &neigbor_tile_id in tile_id.neighbors().iter().flatten() {
            self.flood_tiles_inner(viewport, neigbor_tile_id, corrected_tile_size, tiles);
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
}

#[derive(Debug)]
struct ZoomState {
    prev_time: Instant,
    point: Option<Mercator>,
    decay: f32,
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
            State::Some(any) => any
                .downcast_ref::<WidgetState>()
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
                    let delta = (*at - zoom.prev_time).as_secs_f32();
                    let tau = 0.05;
                    let alpha = tau / (tau + delta);

                    zoom.prev_time = *at;
                    zoom.decay *= alpha;

                    let zoom_amt = (delta * zoom.decay) as f64;

                    if zoom.decay.abs() > 0.1 {
                        if let Some(position) = zoom.point {
                            let position = projector.mercator_into_screen_space(position);
                            self.viewpoint
                                .zoom_on_point(zoom_amt, position, projector.bounds);
                        } else {
                            self.viewpoint.zoom_on_center(zoom_amt);
                        }

                        projector.viewpoint = self.viewpoint;
                    } else {
                        state.zoom = None;
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
                    let dynamic_tau = 0.2 + norm_velocity * 0.0001;
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
                    let amount = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => *y as f64 * 20.0,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => *y as f64 * 1.0,
                    };

                    let mut zoom = ZoomState {
                        prev_time: Instant::now(),
                        point: None,
                        decay: amount as f32,
                    };

                    if let Some(position) = cursor.position_over(projector.bounds) {
                        zoom.point = Some(projector.mercator_from_screen_space(position));
                    }

                    // Carry over any existing zoom momentum
                    if let Some(existing_zoom) = state.zoom.take() {
                        zoom.decay += existing_zoom.decay;
                    }

                    state.zoom = Some(zoom);

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

        // Ensure visible tiles are calculated
        if self.visible_tiles.is_empty() {
            let flood_area = bounds.expand(128);
            self.visible_tiles = self.flood_tiles(&flood_area);
        }

        if needs_redraw {
            shell.capture_event();
            shell.request_redraw();
        }

        // Construct vector of tiles that should be fetched
        let mut to_fetch = self
            .visible_tiles
            .iter()
            .filter(|(tile_id, _)| self.map.should_fetch(&tile_id))
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

            state.projector = Some(new_projector);
        }
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
        let bounds = layout.bounds();

        // Render all queued tiles, TODO move this to update function, cache tiles
        let mut draw_cache = DrawCache::new();
        for (tile_id, rectangle) in self.visible_tiles.iter() {
            match self.map.get_drawable(&tile_id) {
                Some(tile) => {
                    draw_cache.insert(*tile_id, tile.clone(), *rectangle);
                }
                _ => {
                    let mut new_tile_id = *tile_id;
                    while let Some(next_tile_id) = new_tile_id.parent() {
                        new_tile_id = next_tile_id;
                        if let Some(tile) = self.map.get_drawable(&new_tile_id) {
                            // This tile is already set to be drawn
                            if draw_cache.contains_key(&new_tile_id) {
                                break;
                            }

                            // Determine the offset of this tile relative to the viewport center
                            let zoom_scale = 2u32.pow((tile_id.zoom() - new_tile_id.zoom()) as u32);
                            let tile_size = rectangle.width * zoom_scale as f32;

                            let projector = Projector {
                                viewpoint: self.viewpoint,
                                cursor: None,
                                bounds,
                            };

                            let tile_mercator = new_tile_id.to_mercator();
                            let screen_pos = projector.mercator_into_screen_space(tile_mercator);

                            let projected_position = Rectangle {
                                x: screen_pos.x,
                                y: screen_pos.y,
                                width: tile_size,
                                height: tile_size,
                            };

                            draw_cache.insert(new_tile_id, tile.clone(), projected_position);

                            break;
                        }
                    }
                }
            }
        }

        // Create new layer to ensure tiles are clipped,
        // and draw tiles in order of zoom level (lowest first)
        renderer.with_layer(bounds, |renderer| {
            for (handle, bounds) in draw_cache.iter_tiles() {
                // Draw tiles that are true-sized in a separate pass later.
                // Seemingly Iced (or WGPU) does not respect draw order when mixing filter methods
                if (bounds.width - self.map.tile_size() as f32).abs() < 0.02 {
                    continue;
                }

                let image = Image::new(handle)
                    .snap(false)
                    .filter_method(FilterMethod::Linear);
                renderer.draw_image(image, bounds, bounds)
            }
        });

        renderer.with_layer(bounds, |renderer| {
            for (handle, bounds) in draw_cache.iter_tiles() {
                // These images were drawn in the previous pass
                if (bounds.width - self.map.tile_size() as f32).abs() >= 0.02 {
                    continue;
                }

                let image = Image::new(handle)
                    .snap(true)
                    .filter_method(FilterMethod::Nearest);
                renderer.draw_image(image, bounds, bounds)
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
