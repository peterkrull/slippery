use iced::mouse;
use iced::widget::canvas::{self, Path, Stroke};
use iced::{self, Color, Element, Task};
use slippery::{
    CacheMessage, Geographic, MapProgram, Projector, TileCache, TileCoord, Viewpoint, Zoom,
    sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery - Interactive Triangle")
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    MapProjector(Projector),
    HoverVertex(Option<usize>),
    StartDrag(usize),
    EndDrag,
    MoveVertex(Geographic),
}

struct Application {
    cache: TileCache,
    viewpoint: Viewpoint,
    vertices: Vec<Geographic>,
    hovered_vertex: Option<usize>,
    dragging_vertex: Option<usize>,
}

const PARIS: Geographic = Geographic::new(2.3522, 48.8566);
const LONDON: Geographic = Geographic::new(-0.1278, 51.5074);
const BRUSSELS: Geographic = Geographic::new(4.3517, 50.8503);

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint {
                    position: Geographic::new(10.0, 50.0).as_mercator(),
                    zoom: Zoom::try_from(4.).unwrap(),
                },
                // Initial triangle vertices
                vertices: vec![PARIS, LONDON, BRUSSELS],
                hovered_vertex: None,
                dragging_vertex: None,
            },
            Task::done(Message::Cache(CacheMessage::Load {
                id: TileCoord::ZERO,
            })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MapProjector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
            Message::HoverVertex(index) => {
                self.hovered_vertex = index;
            }
            Message::StartDrag(index) => {
                self.dragging_vertex = Some(index);
                self.hovered_vertex = None;
            }
            Message::EndDrag => {
                self.dragging_vertex = None;
            }
            Message::MoveVertex(geo) => {
                if let Some(index) = self.dragging_vertex {
                    self.vertices[index] = geo;
                }
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        let vertices = self.vertices.clone();
        let hovered_vertex = self.hovered_vertex;
        let dragging_vertex = self.dragging_vertex;

        let vertices_interact = vertices.clone();

        MapProgram::new(&self.cache)
            .on_cache(Message::Cache)
            .on_update(Message::MapProjector)
            .with_draw_layer(move |projector, frame| {
                let mut iterator = vertices.iter().cloned();
                if let Some(vertex) = iterator.next() {
                    let triangle = Path::new(|builder| {
                        builder.move_to(projector.geographic_into_screen_space(vertex));
                        for vertex in iterator {
                            builder.line_to(projector.geographic_into_screen_space(vertex));
                        }
                        builder.close();
                    });

                    // Fill the triangle with semi-transparent color
                    frame.fill(&triangle, Color::from_rgba(0.0, 0.8, 0.0, 0.2));

                    // Stroke the outline
                    frame.stroke(
                        &triangle,
                        Stroke::default()
                            .with_width(2.0)
                            .with_color(Color::from_rgb(0.0, 0.8, 0.0)),
                    );
                }

                // 2. Draw Points (Interactive Handles)
                for (i, vertex) in vertices.iter().enumerate() {
                    let pos = projector.geographic_into_screen_space(*vertex);

                    let (radius, color) = if dragging_vertex == Some(i) {
                        (10.0, Color::from_rgb(1.0, 0.0, 0.0)) // Red when dragging
                    } else if hovered_vertex == Some(i) {
                        (8.0, Color::from_rgb(1.0, 0.5, 0.0)) // Orange when hovered
                    } else {
                        (5.0, Color::from_rgb(0.0, 0.0, 1.0)) // Blue normally
                    };

                    let circle = Path::circle(pos, radius);
                    frame.fill(&circle, color);
                    frame.stroke(
                        &circle,
                        Stroke::default().with_color(Color::WHITE).with_width(1.5),
                    );
                }
            })
            .with_interaction(move |projector, event| {
                use slippery::Action;
                match event {
                    canvas::Event::Mouse(mouse_event) => {
                        match mouse_event {
                            mouse::Event::CursorMoved { .. } => {
                                let cursor_pos = if let Some(p) = projector.cursor {
                                    p
                                } else {
                                    return Action::None;
                                };

                                // Handling Dragging
                                if let Some(_) = dragging_vertex {
                                    // Project cursor back to Geographic to move vertex
                                    let mercator = projector.screen_space_into_mercator(cursor_pos);
                                    let geo = mercator.as_geographic();
                                    return Action::Capture(Message::MoveVertex(geo));
                                }

                                // Handling Hover
                                // Check if cursor is over any vertex
                                for (i, vertex) in vertices_interact.iter().enumerate() {
                                    let screen_pos =
                                        projector.geographic_into_screen_space(*vertex);
                                    if screen_pos.distance(cursor_pos) < 10.0 {
                                        if hovered_vertex != Some(i) {
                                            return Action::Publish(Message::HoverVertex(Some(i)));
                                        }
                                        return Action::None; // Already hovered, don't spam messages
                                    }
                                }

                                // If we were hovering, but now aren't
                                if hovered_vertex.is_some() {
                                    return Action::Publish(Message::HoverVertex(None));
                                }

                                Action::None
                            }
                            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                                // If hovering over a vertex, start dragging
                                if let Some(idx) = hovered_vertex {
                                    return Action::Capture(Message::StartDrag(idx));
                                }
                                Action::None
                            }
                            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                                if dragging_vertex.is_some() {
                                    return Action::Capture(Message::EndDrag);
                                }
                                Action::None
                            }
                            _ => Action::None,
                        }
                    }
                    _ => Action::None,
                }
            })
            .build(self.viewpoint)
    }
}
