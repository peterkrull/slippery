use iced::{self, Element, Task, Theme, widget::row};
use slippery::{
    CacheMessage, MapWidget, Projector, TileCache, TileCoord, Viewpoint, sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery double view example")
        .theme(Theme::Dark)
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    MapProjector1(Projector),
    MapProjector2(Projector),
}

struct Application {
    cache: TileCache,
    viewpoint1: Viewpoint,
    viewpoint2: Viewpoint,
}

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                viewpoint1: Viewpoint::new_paris(),
                viewpoint2: Viewpoint::new_denmark(),
            },
            Task::done(Message::Cache(CacheMessage::LoadTile {
                id: TileCoord::ZERO,
            })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MapProjector1(projector) => {
                self.viewpoint1 = projector.viewpoint;
            }
            Message::MapProjector2(projector) => {
                self.viewpoint2 = projector.viewpoint;
            }
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        row![
            MapWidget::new(&self.cache, Message::Cache, self.viewpoint1).on_update(Message::MapProjector1),
            iced::widget::rule::vertical(2),
            MapWidget::new(&self.cache, Message::Cache, self.viewpoint2).on_update(Message::MapProjector2),
        ]
    }
}
