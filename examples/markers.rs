use iced::{
    self, Element, Task, Theme,
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
        .theme(Theme::Dark)
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

const DISTANCE: f32 = 60.0;

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
            // This should ensure we always have something to fall back on when rendering.
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
                        .style(if *distance < DISTANCE {
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

        let map = container(map).style(container::secondary);

        let mut stack = iced::widget::Stack::new().push(map);

        if !self.markers.is_empty() {
            let content = container(
                container(column(self.markers.iter().enumerate().map(
                    |(index, (marker, distance))| {
                        container(
                            row![
                                Button::new("X").on_press(Message::Marker(MarkerOp::Remove(index))),
                                text(format!(
                                    "[{index}] {marker:.5?}, ({distance:.03} pixels away)"
                                ))
                            ]
                            .spacing(10.)
                            .align_y(Vertical::Center),
                        )
                        .padding(10.)
                        .style(if *distance < DISTANCE {
                            container::secondary
                        } else {
                            container::transparent
                        })
                        .width(360)
                        .into()
                    },
                )))
                .style(|theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(palette.background.weak.color.into()),
                        text_color: Some(palette.background.weak.text),
                        border: iced::Border {
                            width: 1.0,
                            radius: 8.0.into(),
                            color: palette.background.weak.color,
                        },
                        ..container::Style::default()
                    }
                })
                .padding(15.),
            )
            .padding(20.);

            stack = stack.push(content);
        };

        stack
    }
}
