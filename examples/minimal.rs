use iced::{self, Element, Task, Theme};
use slippery::{
    CacheMessage, MapWidget, Projector, TileCache, TileCoord, Viewpoint, sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery minimal example")
        .theme(Theme::Dark)
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    MapProjector(Projector),
}

struct Application {
    cache: TileCache,
    viewpoint: Viewpoint,
}

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint::new_paris(),
            },
            // This should ensure we always have something to fall back on when rendering.
            Task::done(Message::Cache(CacheMessage::LoadTile {
                id: TileCoord::ZERO,
            })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // The updated projector contains the new viewpoint
            Message::MapProjector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            // Glue the map widgets update function into our application
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        MapWidget::new(&self.cache, Message::Cache, self.viewpoint).on_update(Message::MapProjector)
    }
}
