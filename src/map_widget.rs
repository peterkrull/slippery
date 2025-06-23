use std::collections::{HashMap, hash_map::Entry};

use iced::{Element, Rectangle, Vector};
use iced_core::{Image, Widget, image::Handle, renderer::Quad};

use crate::{
    draw_cache::DrawCache,
    position::{Geographic, Mercator},
    tile::TileId,
    tile_cache::{CacheMessage, TileCache},
    zoom::Zoom,
};

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
    pub fn into_pixel_space(&self, tile_size: u32) -> iced::Point<f64> {
        self.position.into_pixel_space(tile_size, self.zoom.f64())
    }

    /// Get the [`Mercator`] coordinate for a position within the viewport bounds
    pub fn position_in_viewport(
        &self,
        tile_size: u32,
        position: iced::Point,
        bounds: Rectangle,
    ) -> Mercator {
        // Get cursor position relative to viewport center
        let cursor_offset = position - bounds.center();
        let cursor_offset = Vector::new(cursor_offset.x as f64, cursor_offset.y as f64);

        // Temporarily shift the viewport to be centered over the cursor
        let center_pixel_space = self.position.into_pixel_space(tile_size, self.zoom.f64());
        let adjusted_center = center_pixel_space + cursor_offset;
        Mercator::from_pixel_space(adjusted_center, tile_size, self.zoom.f64())
    }

    /// Zoom in/out of a position in the viewport. This would typically be the cursor position
    pub fn zoom_on_position(
        &mut self,
        zoom_amount: f64,
        tile_size: u32,
        position: iced::Point,
        bounds: Rectangle,
    ) {
        // Get cursor position relative to viewport center
        let cursor_offset = position - bounds.center();
        let cursor_offset = Vector::new(cursor_offset.x as f64, cursor_offset.y as f64);

        // Temporarily shift the viewport to be centered over the cursor
        let center_pixel_space = self.position.into_pixel_space(tile_size, self.zoom.f64());
        let adjusted_center = center_pixel_space + cursor_offset;
        self.position = Mercator::from_pixel_space(adjusted_center, tile_size, self.zoom.f64());

        // Apply desired zoom
        self.zoom.zoom_by(zoom_amount);

        // Shift the viewport back by the same amount after applying zoom
        let center_pixel_space = self.position.into_pixel_space(tile_size, self.zoom.f64());
        let adjusted_center = center_pixel_space - cursor_offset;
        self.position = Mercator::from_pixel_space(adjusted_center, tile_size, self.zoom.f64());
    }

    /// Zoom in/out of the center of the viewport
    pub fn zoom_on_center(&mut self, zoom_amount: f64) {
        self.zoom.zoom_by(zoom_amount);
    }
}

/// A [slippy tile](https://wiki.openstreetmap.org/wiki/Slippy_map) widget
pub struct MapWidget<'a, Message> {
    map: &'a TileCache,
    viewpoint: Viewpoint,
    prev_bounds: Rectangle,
    scale: f64,
    visible_tiles: Vec<(TileId, Rectangle)>,
    mapper: fn(CacheMessage) -> Message,
    markers: Option<&'a [Geographic]>,
    on_change: Option<fn(Viewpoint) -> Message>,
    on_click: Option<fn(Geographic) -> Message>,
    on_hover: Option<fn(Geographic) -> Message>,
}

impl<'a, Message> MapWidget<'a, Message> {
    pub fn new(
        map: &'a TileCache,
        mapper: fn(CacheMessage) -> Message,
        position: Viewpoint,
    ) -> Self {
        Self {
            map,
            viewpoint: position,
            prev_bounds: Rectangle::default(),
            scale: 1.0,
            visible_tiles: Vec::new(),
            markers: None,
            on_change: None,
            on_click: None,
            on_hover: None,
            mapper,
        }
    }

    /// This message is emitted when changing the map viewpoint (position/zoom)
    pub fn on_viewpoint_change(self, func: fn(Viewpoint) -> Message) -> Self {
        Self {
            on_change: Some(func),
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

    /// This message is emitted when the cursor hovers over different location
    pub fn on_hover(self, func: fn(Geographic) -> Message) -> Self {
        Self {
            on_hover: Some(func),
            ..self
        }
    }

    /// Draw a list of [`Geographic`] markers
    pub fn with_markers(self, markers: &'a [Geographic]) -> Self {
        Self {
            markers: Some(markers),
            ..self
        }
    }

    pub fn with_scale(self, scale: f32) -> Self {
        Self {
            scale: scale as f64,
            ..self
        }
    }

    /// Use [flood fill algorithm](https://en.wikipedia.org/wiki/Flood_fill) to determine
    /// which tiles need to be drawn..
    pub fn flood_tiles(&self, viewport: &Rectangle) -> Vec<(TileId, Rectangle)> {
        let tile_size = self.map.tile_size();

        // Allocate for the number of tiles to fill the screen, and then some
        let capacity = viewport.area() / (tile_size * tile_size) as f32;
        let mut tiles = HashMap::with_capacity(capacity.ceil() as usize);

        let zoom = self.viewpoint.zoom.f64().min(self.map.max_zoom() as f64);
        let zoom = zoom + self.scale.log2();

        let corrected_tile_size =
            tile_size as f64 * 2f64.powf(self.viewpoint.zoom.f64() - zoom.round());

        let central_tile_id = self.viewpoint.position.tile_id(zoom.round() as u8);

        let map_center = self
            .viewpoint
            .position
            .into_pixel_space(tile_size, self.viewpoint.zoom.f64());

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
        tile_id: TileId,
        map_center: iced::Point<f64>,
        corrected_tile_size: f64,
        tiles: &mut HashMap<TileId, Option<Rectangle>>,
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

enum Movement {
    Idle,
    Dragging {
        mercator: Mercator,
        cursor: iced::Point<f32>,
    },
}

struct WidgetState {
    cursor_position: Option<iced::Point>,
    movement: Movement,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for MapWidget<'a, Message>
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
        _tree: &mut iced_core::widget::Tree,
        _renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        iced_core::layout::Node::new(limits.max())
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
                    cursor_position: None,
                    movement: Movement::Idle,
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

        match event {
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::WheelScrolled { delta } if self.on_change.is_some() => {
                    let zoom_amount = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => *y as f64 * 0.5,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => *y as f64 * 0.01,
                    };

                    if let Some(cursor_position) = cursor.position_over(bounds) {
                        self.viewpoint.zoom_on_position(
                            zoom_amount,
                            self.map.tile_size(),
                            cursor_position,
                            bounds,
                        );
                    } else {
                        self.viewpoint.zoom_on_center(zoom_amount);
                    }
                }
                iced::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_over(bounds) {
                        state.movement = Movement::Dragging {
                            mercator: self.viewpoint.position_in_viewport(
                                self.map.tile_size(),
                                cursor_position,
                                bounds,
                            ),
                            cursor: cursor_position,
                        }
                    }
                }
                iced::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left) => {
                    match state.movement {
                        Movement::Dragging { mercator, cursor } => {
                            if let Some(cursor_position) = state.cursor_position {
                                let position = self.viewpoint.position_in_viewport(
                                    self.map.tile_size(),
                                    cursor_position,
                                    bounds,
                                );
                                if cursor == cursor_position && position == mercator {
                                    let position = self.viewpoint.position_in_viewport(
                                        self.map.tile_size(),
                                        cursor_position,
                                        bounds,
                                    );
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
                    state.cursor_position = Some(*position);

                    if let Some(on_hover) = self.on_hover {
                        let position = self.viewpoint.position_in_viewport(
                            self.map.tile_size(),
                            *position,
                            bounds,
                        );
                        shell.publish(on_hover(position.as_geographic()));
                    }

                    if let Movement::Dragging { mercator, .. } = state.movement {
                        if self.on_change.is_some() {
                            let cursor_position = self.viewpoint.position_in_viewport(
                                self.map.tile_size(),
                                *position,
                                bounds,
                            );
                            let mercator_diff_x = mercator.east_x() - cursor_position.east_x();
                            let mercator_diff_y = mercator.north_y() - cursor_position.north_y();

                            self.viewpoint.position = Mercator::new(
                                self.viewpoint.position.east_x() + mercator_diff_x,
                                self.viewpoint.position.north_y() + mercator_diff_y,
                            );
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        }

        let visuals_changed = self.viewpoint != initial_viewpoint || self.prev_bounds != bounds;

        if visuals_changed {
            shell.capture_event();
            shell.request_redraw();
            if let Some(on_position_change) = self.on_change {
                shell.publish(on_position_change(self.viewpoint));
            }
        }

        if visuals_changed || self.visible_tiles.is_empty() {
            let flood_area = bounds.expand(128);
            self.visible_tiles = self.flood_tiles(&flood_area);
            self.prev_bounds = bounds;
        }

        // Enqueue loading of missing tiles
        for (tile_id, _) in &self.visible_tiles {
            if self.map.should_fetch(&tile_id) {
                shell.publish((self.mapper)(CacheMessage::LoadTile { id: *tile_id }))
            }
        }
    }

    fn draw(
        &self,
        _tree: &iced_core::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced_core::renderer::Style,
        layout: iced_core::Layout<'_>,
        _cursor: iced_core::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        let bounds = layout.bounds();

        let map_center = self
            .viewpoint
            .position
            .into_pixel_space(self.map.tile_size(), self.viewpoint.zoom.f64());

        // Render all queued tiles, TODO move this to update function, cache tiles
        let mut draw_cache = DrawCache::new();
        for (tile_id, rectangle) in self.visible_tiles.iter().rev() {
            match self.map.get(&tile_id) {
                Some(tile) => {
                    draw_cache.insert(*tile_id, tile, *rectangle);
                }
                _ => {
                    let mut backup_tile_id = *tile_id;
                    while let Some(next_tile_id) = backup_tile_id.downsample() {
                        backup_tile_id = next_tile_id;
                        if let Some(tile) = self.map.get(&backup_tile_id) {
                            // This tile is already set to be drawn
                            if draw_cache.contains_key(&backup_tile_id) {
                                break;
                            }

                            // Determine the offset of this tile relative to the viewport center
                            let zoom_scale =
                                2u32.pow((tile_id.zoom() - backup_tile_id.zoom()) as u32);
                            let tile_size = rectangle.width * zoom_scale as f32;

                            let projected_position =
                                backup_tile_id.on_viewport(bounds, tile_size as f64, map_center);

                            draw_cache.insert(backup_tile_id, tile, projected_position);

                            break;
                        }
                    }
                }
            }
        }

        // Create new layer to ensure tiles are clipped,
        // and draw tiles in order of zoom level (lowest first)
        renderer.with_layer(bounds, |renderer| {
            for (_, (handle, rectangle)) in draw_cache.iter() {
                let image = Image::new(handle);
                renderer.draw_image(image, rectangle)
            }
        });

        // Draw markers - WIP
        renderer.with_layer(bounds, |renderer| {
            if let Some(markers) = &self.markers {
                for marker in *markers {
                    let position = marker
                        .as_mercator()
                        .into_pixel_space(self.map.tile_size(), self.viewpoint.zoom.f64());
                    let center = self
                        .viewpoint
                        .position
                        .into_pixel_space(self.map.tile_size(), self.viewpoint.zoom.f64());

                    let location = center - position;
                    let location =
                        bounds.center() - Vector::new(location.x as f32, location.y as f32);

                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x: location.x - 8.0,
                                y: location.y - 8.0,
                                width: 16.0,
                                height: 16.0,
                            },
                            border: iced::Border::default()
                                .rounded(8.0)
                                .width(2)
                                .color(iced::Color::from_rgb(1.0, 0.7, 0.7)),
                            shadow: iced::Shadow {
                                color: iced::Color::BLACK.scale_alpha(0.6),
                                offset: Vector::new(0.0, 2.0),
                                blur_radius: 8.0,
                            },
                            ..Quad::default()
                        },
                        iced::Color::from_rgb(1.0, 0.1, 0.1),
                    );
                }
            }
        });
    }

    fn mouse_interaction(
        &self,
        state: &iced_core::widget::Tree,
        _layout: iced_core::Layout<'_>,
        _cursor: iced_core::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> iced_core::mouse::Interaction {
        let state = match &state.state {
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

impl<'a, Message: 'a, Theme, Renderer> From<MapWidget<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::image::Renderer<Handle = Handle>,
{
    fn from(value: MapWidget<'a, Message>) -> Self {
        Self::new(value)
    }
}
