use iced::{
    self, Element, Task,
    alignment::Vertical,
    widget::{Button, Text, column, container, row, text},
};
use slippery::{
    CacheMessage, Geographic, GlobalElement, MapWidget, Projector, TileCache, TileCoord, Viewpoint,
    Zoom, sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery")
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum MarkerOp {
    Remove(usize),
    Add(Geographic),
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    Marker(MarkerOp),
    Button,
    MapProjector(Projector),
}

struct Application {
    cache: TileCache,
    viewpoint: Viewpoint,
    projector: Option<Projector>,
    markers: Vec<(Geographic, f32)>,
}

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                projector: None,
                viewpoint: Viewpoint {
                    position: Geographic::new(2.35, 48.85).as_mercator(),
                    zoom: Zoom::try_from(12.0).unwrap(),
                },
                markers: Vec::new(),
            },
            // This ensures we always have something to fall back on when rendering.
            Task::done(Message::Cache(CacheMessage::LoadTile {
                id: TileCoord::ZERO,
            })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Button => {
                log::info!("Click!");
            }
            Message::MapProjector(projector) => {
                self.viewpoint = projector.viewpoint;

                if let Some(cursor) = projector.cursor_into_pixel_space() {
                    for (position, distance) in &mut self.markers {
                        let diff = projector
                            .mercator_into_pixel_space(position.as_mercator())
                            .distance(cursor);
                        *distance = diff as f32;
                    }
                }

                self.projector = Some(projector);
            }
            Message::Marker(operation) => match operation {
                MarkerOp::Remove(index) => _ = self.markers.remove(index),
                MarkerOp::Add(geographic) => self.markers.push((geographic, 0.0)),
            },
            // Glue the map widgets update function into our application
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        let map = MapWidget::new(&self.cache, Message::Cache, self.viewpoint)
            .on_update(Message::MapProjector)
            .on_click(|x| Message::Marker(MarkerOp::Add(x)))
            .with_children(
                self.markers
                    .iter()
                    .enumerate()
                    .map(|(index, (position, distance))| GlobalElement {
                        element: container(
                            Button::new(Text::new(index.to_string())).on_press(Message::Button),
                        )
                        .style(if *distance < 75. {
                            container::success
                        } else {
                            container::secondary
                        })
                        .padding(15.)
                        .into(),
                        position: *position,
                    })
                    .collect::<Vec<_>>(),
            );

        let mut stack = iced::widget::Stack::new().push(map);

        if !self.markers.is_empty() {
            let content =
                iced::widget::container(
                    iced::widget::container(
                        column(self.markers.iter().enumerate().map(
                            |(index, (marker, distance))| {
                                row![
                                    Button::new("X")
                                        .on_press(Message::Marker(MarkerOp::Remove(index))),
                                    text(format!(
                                        "[{index}] {marker:.5?}, ({distance} pixels away)"
                                    ))
                                ]
                                .spacing(10.)
                                .align_y(Vertical::Center)
                                .into()
                            },
                        ))
                        .spacing(10.)
                        .width(370.),
                    )
                    .style(container::bordered_box)
                    .padding(15.),
                )
                .padding(20.);

            stack = stack.push(content);
        };

        stack
    }
}
