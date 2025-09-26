use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
};

use iced::{Element, Point, Rectangle};
use iced_core::{
    Image, Widget,
    image::{FilterMethod, Handle},
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
    on_viewpoint: Option<fn(Viewpoint) -> Message>,
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
            on_viewpoint: None,
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

    /// This message is emitted when a location is left-clicked
    pub fn on_viewpoint(self, func: fn(Viewpoint) -> Message) -> Self {
        Self {
            on_viewpoint: Some(func),
            ..self
        }
    }

    /// Draw a list of [`Geographic`] markers
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

        let map_center = self
            .viewpoint
            .position
            .into_pixel_space(self.viewpoint.zoom.f64());

        // Recursively fill up the `tiles` map
        self.flood_tiles_inner(
            viewport,
            central_tile_id,
            map_center,
            corrected_tile_size,
            &mut tiles,
        );

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
        map_center: iced::Point<f64>,
        corrected_tile_size: f64,
        tiles: &mut HashMap<TileCoord, Option<Rectangle>>,
    ) {
        // Return early if this entry has already been checked
        let Entry::Vacant(entry) = tiles.entry(tile_id) else {
            return;
        };

        // Determine the offset of this tile relative to the viewport center
        let projected_position = tile_id.on_viewport(*viewport, corrected_tile_size, map_center);

        // Accept the tile if it intersects the viewport
        if viewport.intersects(&projected_position) {
            entry.insert(Some(projected_position));
        } else {
            entry.insert(None);
            return;
        }

        // Recurse using all valid neighbors
        for &neigbor_tile_id in tile_id.neighbors().iter().flatten() {
            self.flood_tiles_inner(
                viewport,
                neigbor_tile_id,
                map_center,
                corrected_tile_size,
                tiles,
            );
        }
    }
}

#[derive(Clone)]
enum Movement {
    Idle,
    Dragging {
        mercator: Mercator,
        cursor: iced::Point<f32>,
    },
}

#[derive(Clone)]
struct WidgetState {
    cursor: Option<iced::Point>,
    movement: Movement,
    prev_bounds: Rectangle,
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
        &self,
        tree: &mut iced_core::widget::Tree,
        renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        let state = match &mut tree.state {
            iced_core::widget::tree::State::None => {
                tree.state = iced_core::widget::tree::State::new(WidgetState {
                    cursor: None,
                    movement: Movement::Idle,
                    prev_bounds: Rectangle::default(),
                });

                let iced_core::widget::tree::State::Some(any) = &mut tree.state else {
                    panic!("Must happen")
                };
                any.downcast_mut::<WidgetState>()
                    .expect("Downcast widget state")
            }
            iced_core::widget::tree::State::Some(any) => any
                .downcast_mut::<WidgetState>()
                .expect("Downcast widget state"),
        };

        let state = state.clone();

        let bounds = limits.max();

        let children = self
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let inner_widget = child.element.as_widget();

                let position = Projector {
                    viewpoint: self.viewpoint,
                    cursor: state.cursor,
                    bounds: Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: bounds.width,
                        height: bounds.height,
                    },
                }
                .screen_position_of(child.position);

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

        iced_core::layout::Node::with_children(bounds, children)
    }

    /// Processes a runtime [`Event`].
    ///
    /// By default, it does nothing.
    fn update(
        &mut self,
        state: &mut iced_core::widget::Tree,
        event: &iced::Event,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn iced_core::Clipboard,
        shell: &mut iced_core::Shell<'_, Message>,
        _viewport: &iced::Rectangle,
    ) {
        let bounds = layout.bounds();
        let initial_viewpoint = self.viewpoint;

        let state = match &mut state.state {
            iced_core::widget::tree::State::None => {
                state.state = iced_core::widget::tree::State::new(WidgetState {
                    cursor: None,
                    movement: Movement::Idle,
                    prev_bounds: Rectangle::default(),
                });

                let iced_core::widget::tree::State::Some(any) = &mut state.state else {
                    panic!("Must happen")
                };
                any.downcast_mut::<WidgetState>()
                    .expect("Downcast widget state")
            }
            iced_core::widget::tree::State::Some(any) => any
                .downcast_mut::<WidgetState>()
                .expect("Downcast widget state"),
        };

        let projector = Projector {
            viewpoint: self.viewpoint,
            cursor: state.cursor,
            bounds: layout.bounds(),
        };

        match event {
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::WheelScrolled { delta } if self.on_update.is_some() => {
                    let amount = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => *y as f64 * 0.5,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => *y as f64 * 0.01,
                    };

                    if let Some(position) = cursor.position_over(bounds) {
                        self.viewpoint.zoom_on_point(amount, position, bounds);
                    } else {
                        self.viewpoint.zoom_on_center(amount);
                    }
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_over(bounds) {
                        state.movement = Movement::Dragging {
                            mercator: self.viewpoint.position_in_viewport(cursor_position, bounds),
                            cursor: cursor_position,
                        }
                    }
                }
                iced::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left) => {
                    match state.movement {
                        Movement::Dragging { mercator, cursor } => {
                            if let Some(cursor_position) = state.cursor {
                                let position =
                                    self.viewpoint.position_in_viewport(cursor_position, bounds);
                                if cursor == cursor_position && position == mercator {
                                    let position = self
                                        .viewpoint
                                        .position_in_viewport(cursor_position, bounds);
                                    if let Some(on_clicked) = self.on_click {
                                        shell.publish(on_clicked(position.as_geographic()));
                                    }
                                }
                            }
                        }
                        _ => (),
                    }

                    // Temporary WIP
                    state.movement = Movement::Idle
                }
                iced::mouse::Event::CursorMoved { position } => {
                    state.cursor = Some(*position);

                    if let Movement::Dragging { mercator, .. } = state.movement {
                        if self.on_update.is_some() {
                            let cursor_position =
                                self.viewpoint.position_in_viewport(*position, bounds);
                            let mercator_diff_x = mercator.east_x() - cursor_position.east_x();
                            let mercator_diff_y = mercator.north_y() - cursor_position.north_y();

                            self.viewpoint.position = Mercator::new(
                                self.viewpoint.position.east_x() + mercator_diff_x,
                                self.viewpoint.position.north_y() + mercator_diff_y,
                            );
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
            let projector = Projector {
                viewpoint: self.viewpoint,
                cursor: state.cursor,
                bounds: layout.bounds(),
            };

            shell.publish(on_update(projector));
        }

        let visuals_changed = self.viewpoint != initial_viewpoint || state.prev_bounds != bounds;

        state.prev_bounds = bounds;

        if visuals_changed {
            shell.capture_event();
            shell.request_redraw();
            if let Some(on_viewpoint) = self.on_viewpoint {
                shell.publish(on_viewpoint(self.viewpoint));
            }
        }

        if visuals_changed || self.visible_tiles.is_empty() {
            let flood_area = bounds.expand(128);
            self.visible_tiles = self.flood_tiles(&flood_area);
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

        let map_center = self.viewpoint.into_pixel_space();

        // Render all queued tiles, TODO move this to update function, cache tiles
        let mut draw_cache = DrawCache::new();
        for (tile_id, rectangle) in self.visible_tiles.iter() {
            match self.map.get(&tile_id) {
                Some(tile) => {
                    draw_cache.insert(*tile_id, tile, *rectangle);
                }
                _ => {
                    let mut new_tile_id = *tile_id;
                    while let Some(next_tile_id) = new_tile_id.parent() {
                        new_tile_id = next_tile_id;
                        if let Some(tile) = self.map.get(&new_tile_id) {
                            // This tile is already set to be drawn
                            if draw_cache.contains_key(&new_tile_id) {
                                break;
                            }

                            // Determine the offset of this tile relative to the viewport center
                            let zoom_scale = 2u32.pow((tile_id.zoom() - new_tile_id.zoom()) as u32);
                            let tile_size = rectangle.width * zoom_scale as f32;

                            let projected_position =
                                new_tile_id.on_viewport(bounds, tile_size as f64, map_center);

                            draw_cache.insert(new_tile_id, tile, projected_position);

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
                // Draw tiles that are true-sized in a separate pass later
                // Seemingly Iced (or WGPU) does not respect draw order when mixing filter methods
                if (bounds.width - self.map.tile_size() as f32).abs() < f32::EPSILON {
                    continue;
                }

                let image = Image::new(handle)
                    .snap(true)
                    .filter_method(FilterMethod::Linear);
                renderer.draw_image(image, bounds)
            }
        });

        renderer.with_layer(bounds, |renderer| {
            for (handle, bounds) in draw_cache.iter_tiles() {
                // These images were drawn in the previous pass
                if (bounds.width - self.map.tile_size() as f32).abs() >= f32::EPSILON {
                    continue;
                }

                let image = Image::new(handle)
                    .snap(true)
                    .filter_method(FilterMethod::Nearest);
                renderer.draw_image(image, bounds)
            }
        });

        // Draw children
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
            iced_core::widget::tree::State::Some(any) => any
                .downcast_ref::<WidgetState>()
                .expect("Downcast widget state"),
            _ => return iced_core::mouse::Interaction::Idle,
        };

        match state.movement {
            Movement::Idle => iced_core::mouse::Interaction::Idle,
            Movement::Dragging { .. } => iced_core::mouse::Interaction::Grabbing,
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

pub struct GlobalElement<'a, Message, Theme, Renderer> {
    pub element: Element<'a, Message, Theme, Renderer>,
    pub position: Geographic,
}
