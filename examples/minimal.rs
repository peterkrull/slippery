use iced::{self, Element, Task};
use slippery::{
    CacheMessage, MapWidget, Projector, TileCache, Viewpoint, Zoom, location,
    sources::OpenStreetMap,
};

fn main() {
    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery minimal example")
        .run()
        .unwrap();
}

struct Application {
    cache: TileCache,
    viewpoint: Viewpoint,
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    Projector(Projector),
}

impl Application {
    pub fn boot() -> Self {
        Application {
            cache: TileCache::new(OpenStreetMap),
            viewpoint: Viewpoint {
                position: location::paris().as_mercator(),
                zoom: Zoom::try_from(12.0).unwrap(),
                rotation: 0.0,
            },
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // The updated projector contains the new viewpoint
            Message::Projector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            // Glue the cache update function into our application
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        MapWidget::new(&self.cache, Message::Cache, self.viewpoint).on_update(Message::Projector)
    }
}
