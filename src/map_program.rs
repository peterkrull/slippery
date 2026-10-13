use iced::widget::canvas::Program;
use iced::{Length, Rectangle};
use iced::{Point, Widget};
use iced::{
    mouse::Cursor,
    widget::canvas::{Action as CanvasAction, Canvas, Event, Frame, Geometry},
};

use crate::{
    CacheMessage, Projector, TileCache, Viewpoint, global_element::GlobalElement,
    map_layers::MapLayers, map_widget::MapWidget,
};

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
///         let pos = ctx.to_screen(Geodetic::new(48.8566, 2.3522));
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

    // User drawing layer(s)
    visual_layers: Vec<Box<dyn Fn(&Projector, &mut Frame<iced::Renderer>) + 'a>>,

    // User interaction layer(s)
    interact_layers: Vec<Box<dyn Fn(&Projector, &Cursor, &Event) -> Action<Message> + 'a>>,

    // GlobalElements (markers, widgets at geodetic positions)
    children: Vec<GlobalElement<'a, Message, iced::Theme, iced::Renderer>>,
}

impl<'a, Message: 'a> MapProgram<'a, Message> {
    /// Create a new MapProgram with the given tile cache.
    ///
    /// You must call `.on_cache()` to configure cache message handling.
    pub fn new(tile_cache: &'a TileCache) -> Self {
        Self {
            tile_cache,
            on_cache: |_| panic!("MapProgram: on_cache() must be configured"),
            on_update: None,
            visual_layers: Vec::new(),
            interact_layers: Vec::new(),
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
    /// Multiple draw layers may be added, which will be drawn in call order.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_draw_layer(|projector, frame| {
    ///     let pos = projector.geodetic_into_screen_space(Geodetic::new(48.8566, 2.3522));
    ///     frame.fill(&canvas::Path::circle(pos, 10.0), Color::RED);
    /// })
    /// ```
    pub fn with_visual_layer<F>(mut self, f: F) -> Self
    where
        F: Fn(&Projector, &mut Frame<iced::Renderer>) + 'a,
    {
        self.visual_layers.push(Box::new(f));
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
    ///             return MapInteraction::Capture(Message::Clicked(mercator.as_geodetic()));
    ///         }
    ///     }
    ///     MapInteraction::None  // Pass through to MapWidget for panning
    /// })
    /// ```
    pub fn with_interact_layer<F>(mut self, f: F) -> Self
    where
        F: Fn(&Projector, &Cursor, &Event) -> Action<Message> + 'a,
    {
        self.interact_layers.push(Box::new(f));
        self
    }

    /// Add globally positioned elements (markers, widgets at geodetic coordinates).
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_children(vec![
    ///     GlobalElement {
    ///         position: Geodetic::new(2.3522, 48.8566),
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
    pub fn build(
        self,
        viewpoint: Viewpoint,
    ) -> impl Widget<Message, iced::Theme, iced::Renderer> + 'a
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
        let base_map = MapLayers::new(map_widget, viewpoint, self.children);

        let mut overlays =
            Vec::with_capacity(self.visual_layers.len() + self.interact_layers.len());

        for interact_fn in self.interact_layers {
            let canvas = Canvas::new(InteractProgram {
                interact_fn,
                viewpoint,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed();

            overlays.push(canvas);
        }

        for visual_fn in self.visual_layers {
            let canvas = Canvas::new(VisualProgram {
                draw_fn: visual_fn,
                viewpoint,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed();

            overlays.push(canvas);
        }

        // If there's a draw layer or interaction layer, add a canvas overlay
        iced::widget::Stack::new()
            .push(base_map.boxed())
            .extend(overlays)
    }
}

struct VisualProgram<'a> {
    draw_fn: Box<dyn Fn(&Projector, &mut Frame<iced::Renderer>) + 'a>,
    viewpoint: Viewpoint,
}

impl<'a, Message: Clone> Program<Message> for VisualProgram<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> Option<CanvasAction<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let projector = Projector {
            viewpoint: self.viewpoint,
            bounds: Rectangle::new(Point::ORIGIN, bounds.size()),
        };

        let mut frame = Frame::new(renderer, bounds.size());
        (self.draw_fn)(&projector, &mut frame);

        vec![frame.into_geometry()]
    }
}

struct InteractProgram<'a, Message> {
    interact_fn: Box<dyn Fn(&Projector, &Cursor, &Event) -> Action<Message> + 'a>,
    viewpoint: Viewpoint,
}

impl<'a, Message: Clone> Program<Message> for InteractProgram<'a, Message> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<CanvasAction<Message>> {
        let projector = Projector {
            viewpoint: self.viewpoint,
            bounds,
        };

        match (self.interact_fn)(&projector, &cursor, event) {
            Action::None => None,
            Action::Publish(msg) => Some(CanvasAction::publish(msg)),
            Action::Capture(msg) => Some(CanvasAction::publish(msg).and_capture()),
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &iced::Renderer,
        _theme: &iced::Theme,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        Vec::new()
    }
}
