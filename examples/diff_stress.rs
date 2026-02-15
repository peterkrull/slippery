use iced::{
    Border, Color, Element, Shadow, Task, Vector,
    alignment::{Horizontal, Vertical},
    widget::{button, column, container, text},
};
use iced::widget::image;
use slippery::{
    Action, CacheMessage, Geodetic, GlobalElement, MapProgram, Mercator, Projector, TileCache, Viewpoint, Zoom, location, sources::OpenStreetMap
};

fn main() {
    iced::application(StressTest::boot, StressTest::update, StressTest::view)
        .title("Slippery - Diff Stress Test")
        .run()
        .unwrap();
}

struct PointData {
    id: usize,
    position: Mercator,
    is_popup_open: bool,
}

struct StressTest {
    cache: TileCache,
    point_handle_red: image::Handle,
    point_handle_blue: image::Handle,
    viewpoint: Viewpoint,
    points: Vec<PointData>,
    dragged_point: Option<usize>,
    drag_start: Option<iced::Point>,
}

const RADIUS: f32 = 5.0;

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    Projector(Projector),

    // Interaction
    DragStart(usize, iced::Point),
    DragMove(Mercator),
    DragEnd(iced::Point),

    // Popup controls
    ClosePopup(usize),
}

impl StressTest {
    fn boot() -> (Self, Task<Message>) {
        let center = location::paris();
        let mut points = Vec::new();

        // Create initial points around Paris
        let mut index = 0;
        for lat_offs in -500..=500 {
            for lon_offs in -500..=500 {
                points.push(PointData {
                    id: index,
                    position: Geodetic::new(
                        center.longitude() + lon_offs as f64 / 60.0,
                        center.latitude() + lat_offs as f64 / 100.0,
                    ).as_mercator(),
                    is_popup_open: false,
                });
                index += 1;
            }
        }

        (
            Self {
                cache: TileCache::new(OpenStreetMap),
                point_handle_red: create_circle_handle(Color::from_rgb(0.8, 0.2, 0.2)),
                point_handle_blue: create_circle_handle(Color::from_rgb(0.2, 0.4, 0.8)),
                viewpoint: Viewpoint {
                    position: center.as_mercator(),
                    zoom: Zoom::try_from(12.0).unwrap(),
                },
                points,
                dragged_point: None,
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
            Message::DragStart(id, point) => {
                self.dragged_point = Some(id);
                self.drag_start = Some(point);
            }
            Message::DragMove(geo) => {
                if let Some(id) = self.dragged_point {
                    if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
                        point.position = geo;
                    }
                }
            }
            Message::DragEnd(point) => {
                if let Some(id) = self.dragged_point {
                    // Check if it was a click (little to no movement)
                    if let Some(start) = self.drag_start {
                        let dx = point.x - start.x;
                        let dy = point.y - start.y;
                        let dist = (dx * dx + dy * dy).sqrt();

                        // If moved less than 5 pixels, treat as click to toggle popup
                        if dist < 5.0 {
                            if let Some(p) = self.points.iter_mut().find(|p| p.id == id) {
                                p.is_popup_open = !p.is_popup_open;
                            }
                        }
                    }
                    self.dragged_point = None;
                    self.drag_start = None;
                }
            }
            Message::ClosePopup(id) => {
                if let Some(p) = self.points.iter_mut().find(|p| p.id == id) {
                    p.is_popup_open = false;
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let map = MapProgram::new(&self.cache)
            .on_cache(Message::Cache)
            .on_update(Message::Projector)
            .with_draw_layer({
                let points: Vec<_> = self
                    .points
                    .iter()
                    .map(|p| (p.position, p.is_popup_open, p.id))
                    .collect();

                let handle_red = self.point_handle_red.clone();
                let handle_blue = self.point_handle_blue.clone();

                move |projector, frame| {
                    for (pos, is_open, _id) in &points {
                        let screen_pos = projector.mercator_into_screen_space(*pos);

                        // Skip adding the point to the frame if it is out of bounds
                        if !projector.bounds.expand(RADIUS * 2.0).contains(screen_pos) {
                            continue;
                        }

                        let handle = if *is_open {
                            handle_red.clone()
                        } else {
                            handle_blue.clone()
                        };

                        let image = iced::widget::canvas::Image::new(handle);
                        let bounds = iced::Rectangle::new(
                            screen_pos - Vector::new(RADIUS, RADIUS),
                            iced::Size::new(RADIUS * 2.0, RADIUS * 2.0)
                        );
                        frame.draw_image(bounds, image);
                    }
                }
            })
            .with_interaction({
                let points: Vec<_> = self.points.iter().map(|p| (p.position, p.id)).collect();
                let dragged_point = self.dragged_point;

                move |projector, event| {
                    use iced::mouse;
                    use iced::widget::canvas::Event;

                    let cursor = if let Some(c) = projector.cursor {
                        c
                    } else {
                        return Action::None;
                    };

                    match event {
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                            // Find clicked point (reverse to pick top-most)
                            for (pos, id) in points.iter().rev() {
                                let screen_pos = projector.mercator_into_screen_space(*pos);
                                if screen_pos.distance(cursor) < 10.0 {
                                    return Action::Capture(Message::DragStart(*id, cursor));
                                }
                            }
                        }
                        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                            if dragged_point.is_some() {
                                let mercator = projector.screen_space_into_mercator(cursor);
                                return Action::Capture(Message::DragMove(
                                    mercator,
                                ));
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                            if dragged_point.is_some() {
                                return Action::Capture(Message::DragEnd(cursor));
                            }
                        }
                        _ => {}
                    }
                    Action::None
                }
            })
            .with_children(
                self.points
                    .iter()
                    .filter(|p| p.is_popup_open)
                    .map(|p| {
                        GlobalElement::new(
                            container(
                                column![
                                    text(format!("Point #{}", p.id))
                                        .font(iced::font::Font::MONOSPACE),
                                    text(format!(
                                        "{:.4}, {:.4}",
                                        p.position.east_x(),
                                        p.position.south_y()
                                    )),
                                    button("Close")
                                        .on_press(Message::ClosePopup(p.id))
                                        .padding(2)
                                ]
                                .align_x(Horizontal::Center)
                                .spacing(5),
                            )
                            .padding(10)
                            .style(|theme| {
                                container::rounded_box(theme)
                                    .shadow(Shadow {
                                        color: Color::BLACK,
                                        offset: Vector::new(0.0, 2.0),
                                        blur_radius: 4.0,
                                    })
                                    .border(Border::default().rounded(5.0))
                            }),
                            p.position,
                        )
                        .align(Horizontal::Left, Vertical::Top)
                    })
                    .collect::<Vec<_>>(),
            )
            .build(self.viewpoint);

        map.into()
    }
}

fn create_circle_handle(color: Color) -> image::Handle {
    let size = RADIUS as u32 * 2;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - RADIUS;
            let dy = y as f32 - RADIUS;
            let dist = (dx * dx + dy * dy).sqrt();

            let alpha = if dist < RADIUS - 1.0 {
                1.0
            } else if dist < RADIUS {
                RADIUS - dist
            } else {
                0.0
            };

            let r = (color.r * 255.0) as u8;
            let g = (color.g * 255.0) as u8;
            let b = (color.b * 255.0) as u8;
            let a = (color.a * alpha * 255.0) as u8;

            pixels.extend_from_slice(&[r, g, b, a]);
        }
    }

    image::Handle::from_rgba(size, size, pixels)
}
