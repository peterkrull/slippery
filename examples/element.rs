use iced::{
    Border, Color, Element, Shadow, Task, Vector, alignment,
    widget::{button, column, container, text},
};
use slippery::{
    Action, CacheMessage, Geographic, GlobalElement, MapProgram, Projector, TileCache, Viewpoint,
    Zoom, location, sources::OpenStreetMap,
};

fn main() {
    iced::application(PopupExample::boot, PopupExample::update, PopupExample::view)
        .title("Slippery - Popup Example")
        .run()
        .unwrap();
}

struct PopupExample {
    cache: TileCache,
    viewpoint: Viewpoint,
    point_position: Geographic,
    is_dragging: bool,
    is_popup_open: bool,
    drag_start: Option<iced::Point>,
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    Projector(Projector),
    DragStart(iced::Point),
    DragMove(Geographic),
    DragEnd(iced::Point),
    ClosePopup,
}

impl PopupExample {
    fn boot() -> (Self, Task<Message>) {
        (
            Self {
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint {
                    position: location::paris().as_mercator(),
                    zoom: Zoom::try_from(12.0).unwrap(),
                },
                point_position: location::paris(),
                is_dragging: false,
                is_popup_open: false,
                drag_start: None,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Cache(msg) => {
                return self.cache.update(msg).map(Message::Cache);
            }
            Message::Projector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            Message::DragStart(point) => {
                self.is_dragging = true;
                self.drag_start = Some(point);
            }
            Message::DragMove(geo) => {
                if self.is_dragging {
                    self.point_position = geo;
                }
            }
            Message::DragEnd(point) => {
                if self.is_dragging {
                    // Check if it was a click (little to no movement)
                    if let Some(start) = self.drag_start {
                        let dx = point.x - start.x;
                        let dy = point.y - start.y;
                        let dist = (dx * dx + dy * dy).sqrt();

                        // If moved less than 5 pixels, treat as click to toggle popup
                        if dist < 5.0 {
                            self.is_popup_open = !self.is_popup_open;
                        }
                    }
                    self.is_dragging = false;
                    self.drag_start = None;
                }
            }
            Message::ClosePopup => {
                self.is_popup_open = false;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let point_position = self.point_position;
        let is_dragging = self.is_dragging;
        let is_popup_open = self.is_popup_open;

        MapProgram::new(&self.cache)
            .on_cache(Message::Cache)
            .on_update(Message::Projector)
            .with_draw_layer(move |projector, frame| {
                let screen_pos = projector.geographic_into_screen_space(point_position);

                // Draw a nice circle
                let circle = iced::widget::canvas::Path::circle(screen_pos, 10.0);
                frame.fill(&circle, Color::from_rgb(0.0, 0.7, 0.3));
                frame.stroke(
                    &circle,
                    iced::widget::canvas::Stroke::default()
                        .with_color(Color::WHITE)
                        .with_width(2.0),
                );
            })
            .with_interaction(move |projector, event| {
                use iced::mouse;
                use iced::widget::canvas::Event;

                let cursor = if let Some(c) = projector.cursor {
                    c
                } else {
                    return Action::None;
                };
                let screen_pos = projector.geographic_into_screen_space(point_position);
                let distance_to_point = screen_pos.distance(cursor);
                let is_hovering = distance_to_point < 15.0; // slightly larger hit area

                match event {
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                        if is_hovering {
                            return Action::Capture(Message::DragStart(cursor));
                        }
                    }
                    Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if is_dragging {
                            // Convert screen cursor to Geographic to move the point
                            let mercator = projector.screen_space_into_mercator(cursor);
                            return Action::Capture(Message::DragMove(mercator.as_geographic()));
                        }
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        if is_dragging {
                            return Action::Capture(Message::DragEnd(cursor));
                        }
                    }
                    _ => {}
                }

                Action::None
            })
            .with_children(if is_popup_open {
                vec![
                    GlobalElement::new(
                        container(
                            column![
                                text("Movable Point")
                                    .size(14)
                                    .font(iced::font::Font::MONOSPACE),
                                text(format!(
                                    "{:.4}, {:.4}",
                                    point_position.latitude(),
                                    point_position.longitude()
                                ))
                                .size(12),
                                button("Close").on_press(Message::ClosePopup).padding(5)
                            ]
                            .spacing(5)
                            .align_x(alignment::Horizontal::Center),
                        )
                        .padding(10)
                        .style(|theme| {
                            container::rounded_box(theme)
                                .shadow(Shadow {
                                    color: Color::BLACK,
                                    offset: Vector::new(0.0, 2.0),
                                    blur_radius: 10.0,
                                })
                                .border(Border::default().rounded(10.0))
                        }),
                        point_position.as_mercator(),
                    )
                    .align(alignment::Horizontal::Left, alignment::Vertical::Top),
                ]
            } else {
                vec![]
            })
            .build(self.viewpoint)
    }
}
