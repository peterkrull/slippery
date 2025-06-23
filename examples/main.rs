use iced::{
    self, Element, Task,
    alignment::Vertical,
    widget::{Button, column, container, row, slider, text},
};
use slippery::{
    CacheMessage, Geographic, MapWidget, TileCache, TileId, Viewpoint, Zoom, sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippy", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery")
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    AddMarker(Geographic),
    RemoveMarker(usize),
    ChangeViewpoint(Viewpoint),
    ChangeScale(f32),
}

struct Application {
    scale: f32,
    cache: TileCache,
    viewpoint: Viewpoint,
    markers: Vec<Geographic>,
}

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                scale: 1.0,
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint {
                    position: Geographic::new(2.35, 48.85).as_mercator(),
                    zoom: Zoom::try_from(12.0).unwrap(),
                },
                markers: Vec::new(),
            },
            // This ensures we always have something to fall back on when rendering.
            Task::done(Message::Cache(CacheMessage::LoadTile { id: TileId::ZERO })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ChangeScale(scale) => self.scale = scale,
            Message::ChangeViewpoint(position) => self.viewpoint = position,
            Message::RemoveMarker(index) => _ = self.markers.remove(index),
            Message::AddMarker(geographic) => {
                self.markers.push(geographic);
            }
            // Glue the map widgets update function into our application
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        let map = MapWidget::new(&self.cache, Message::Cache, self.viewpoint)
            .on_viewpoint_change(Message::ChangeViewpoint)
            .on_click(Message::AddMarker)
            .with_markers(&self.markers)
            .with_scale(self.scale);

        let mut stack = iced::widget::Stack::new().push(map);

        if !self.markers.is_empty() {
            let content = row![
                iced::widget::container(
                    column(self.markers.iter().enumerate().map(|(index, marker)| {
                        row![
                            Button::new("X").on_press(Message::RemoveMarker(index)),
                            text(format!("{:.5?}", marker))
                        ]
                        .spacing(10.)
                        .align_y(Vertical::Center)
                        .into()
                    }))
                    .spacing(10.)
                    .width(370.),
                )
                .style(container::bordered_box)
                .padding(15.)
            ]
            .padding(10.0);

            stack = stack.push(content);
        };

        iced::widget::container(column![
            stack,
            row![
                text(format!("Scale: {:.1}", self.scale)),
                slider(0.1..=1.5, self.scale, Message::ChangeScale).step(0.1)
            ]
            .padding(10.0)
            .spacing(10.0)
        ])
    }
}
