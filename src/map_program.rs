use iced::widget::{
    canvas as widget_canvas,
    canvas::{self, Frame, Geometry},
    stack,
};
use iced::{Element, Length, Rectangle};
use iced::{Point, mouse};

use crate::{
    CacheMessage, Projector, TileCache, Viewpoint, global_element::GlobalElement,
    map_layers::MapLayers, map_widget::MapWidget,
};

// ============================================================================
// MapProgram - Hybrid Widget Builder
// ============================================================================

/// Represents the result of an interaction handler.
///
/// This enum allows you to control whether the map underneath should also
/// receive the event (e.g. for panning) or if it should be captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action<Message> {
    /// No action taken. Event propagates to map.
    None,
    /// Publish a message, but let the event propagate to the map.
    Publish(Message),
    /// Publish a message and capture the event (preventing map interaction).
    Capture(Message),
}

/// A builder for creating an interactive map with custom drawing and interaction layers.
///
/// MapProgram creates a **layered widget** that composes:
/// - **Bottom**: MapWidget for actual raster tile rendering
/// - **Top**: Transparent Canvas overlay for vector drawing and interactions
///
/// This gives you the best of both worlds: real map tiles plus full canvas interaction support.
///
/// # Example
///
/// ```ignore
/// MapProgram::new(&tile_cache)
///     .on_cache(Message::Cache)
///     .with_draw_layer(|ctx, frame| {
///         let pos = ctx.to_screen(Geographic::new(48.8566, 2.3522));
///         frame.fill(&canvas::Path::circle(pos, 10.0), Color::RED);
///     })
///     .build(viewpoint)
/// ```
pub struct MapProgram<'a, Message> {
    tile_cache: &'a TileCache,

    // Required callback for cache messages
    on_cache: fn(CacheMessage) -> Message,

    // Optional callbacks
    on_update: Option<fn(Projector) -> Message>,

    // User drawing layer
    draw_layer: Option<Box<dyn Fn(&Projector, &mut Frame<iced::Renderer>) + 'a>>,

    // User interaction layer
    interact_layer: Option<Box<dyn Fn(&Projector, &canvas::Event) -> Action<Message> + 'a>>,

    // GlobalElements (markers, widgets at geographic positions)
    children: Vec<GlobalElement<'a, Message, iced::Theme, iced::Renderer>>,
}

// ============================================================================
// Builder API
// ============================================================================

impl<'a, Message: 'a> MapProgram<'a, Message> {
    /// Create a new MapProgram with the given tile cache.
    ///
    /// You must call `.on_cache()` to configure cache message handling.
    pub fn new(tile_cache: &'a TileCache) -> Self {
        Self {
            tile_cache,
            on_cache: |_| panic!("MapProgram: on_cache() must be configured"),
            on_update: None,
            draw_layer: None,
            interact_layer: None,
            children: Vec::new(),
        }
    }

    /// Set the callback for cache messages (tile loading, etc.).
    ///
    /// This is required and will panic if not set.
    pub fn on_cache(mut self, f: fn(CacheMessage) -> Message) -> Self {
        self.on_cache = f;
        self
    }

    /// Set the callback for viewpoint updates (pan, zoom).
    ///
    /// The callback receives the updated `Projector` which contains the new viewpoint.
    pub fn on_update(mut self, f: fn(Projector) -> Message) -> Self {
        self.on_update = Some(f);
        self
    }

    /// Add a custom drawing layer on top of the map tiles.
    ///
    /// The callback receives a `Projector` for coordinate conversion and a `Frame` for drawing.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_draw_layer(|projector, frame| {
    ///     let pos = projector.geographic_into_screen_space(Geographic::new(48.8566, 2.3522));
    ///     frame.fill(&canvas::Path::circle(pos, 10.0), Color::RED);
    /// })
    /// ```
    pub fn with_draw_layer<F>(mut self, f: F) -> Self
    where
        F: Fn(&Projector, &mut Frame<iced::Renderer>) + 'a,
    {
        self.draw_layer = Some(Box::new(f));
        self
    }

    /// Add a custom interaction layer for handling events.
    ///
    /// The callback receives events and can return actions via `MapInteraction`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_interaction(|event, projector| {
    ///     if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
    ///         if let Some(cursor) = projector.cursor {
    ///             let mercator = projector.mercator_from_screen_space(cursor);
    ///             return MapInteraction::Capture(Message::Clicked(mercator.as_geographic()));
    ///         }
    ///     }
    ///     MapInteraction::None  // Pass through to MapWidget for panning
    /// })
    /// ```
    pub fn with_interaction<F>(mut self, f: F) -> Self
    where
        F: Fn(&Projector, &canvas::Event) -> Action<Message> + 'a,
    {
        self.interact_layer = Some(Box::new(f));
        self
    }

    /// Add globally positioned elements (markers, widgets at geographic coordinates).
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_children(vec![
    ///     GlobalElement {
    ///         position: Geographic::new(2.3522, 48.8566),
    ///         element: Button::new("Paris").into(),
    ///     }
    /// ])
    /// ```
    pub fn with_children(
        mut self,
        children: impl IntoIterator<Item = GlobalElement<'a, Message, iced::Theme, iced::Renderer>>,
    ) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Build the final widget with the given viewpoint.
    ///
    /// Returns a layered Element with MapWidget at the bottom and Canvas overlay on top.
    pub fn build(self, viewpoint: Viewpoint) -> Element<'a, Message, iced::Theme, iced::Renderer>
    where
        Message: Clone + 'a,
    {
        // Create base map widget with actual tile rendering
        let mut map_widget = MapWidget::new(self.tile_cache, self.on_cache, viewpoint);

        // Add viewpoint update callback if provided
        if let Some(on_update) = self.on_update {
            map_widget = map_widget.on_update(on_update);
        }

        // Wrap in MapLayers for child positioning
        let layers = MapLayers::new(map_widget, viewpoint, self.children);

        // If there's a draw layer or interaction layer, add a canvas overlay
        if self.draw_layer.is_some() || self.interact_layer.is_some() {
            let overlay = widget_canvas(OverlayProgram {
                draw_fn: self.draw_layer,
                interact_fn: self.interact_layer,
                viewpoint,
            })
            .width(Length::Fill)
            .height(Length::Fill);

            stack![layers, overlay].into()
        } else {
            layers.into()
        }
    }
}

// ============================================================================
// Canvas Overlay for Drawing and Interactions
// ============================================================================

struct OverlayProgram<'a, Message> {
    draw_fn: Option<Box<dyn Fn(&Projector, &mut Frame<iced::Renderer>) + 'a>>,
    interact_fn: Option<Box<dyn Fn(&Projector, &canvas::Event) -> Action<Message> + 'a>>,
    viewpoint: Viewpoint,
}

impl<'a, Message: Clone> canvas::Program<Message> for OverlayProgram<'a, Message> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Some(ref interact_fn) = self.interact_fn {
            let projector = Projector {
                viewpoint: self.viewpoint,
                cursor: cursor.position_over(bounds),
                bounds,
            };

            match interact_fn(&projector, event) {
                Action::None => None,
                Action::Publish(msg) => Some(canvas::Action::publish(msg)),
                Action::Capture(msg) => Some(canvas::Action::publish(msg).and_capture()),
            }
        } else {
            None
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if let Some(ref draw_fn) = self.draw_fn {
            let projector = Projector {
                viewpoint: self.viewpoint,
                cursor: cursor.position_over(bounds),
                bounds: Rectangle::new(Point::ORIGIN, bounds.size()),
            };

            let mut frame = canvas::Frame::new(renderer, bounds.size());
            draw_fn(&projector, &mut frame);
            vec![frame.into_geometry()]
        } else {
            vec![]
        }
    }
}
