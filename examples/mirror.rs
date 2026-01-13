use iced::{self, Element, Task, widget::row};
use slippery::{
    CacheMessage, MapWidget, Projector, TileCache, TileCoord, Viewpoint, Zoom, location,
    sources::{ArcGisWorldMap, OpenStreetMap},
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery mirror view example")
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache1(CacheMessage),
    Cache2(CacheMessage),
    MapProjector(Projector),
}

struct Application {
    cache1: TileCache,
    cache2: TileCache,
    viewpoint: Viewpoint,
}

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache1: TileCache::new(OpenStreetMap),
                cache2: TileCache::new(ArcGisWorldMap),
                viewpoint: Viewpoint {
                    position: location::paris().as_mercator(),
                    zoom: Zoom::try_from(12.0).unwrap(),
                    rotation: 0.0,
                },
            },
            Task::done(Message::Cache1(CacheMessage::Load {
                id: TileCoord::ZERO,
            }))
            .chain(Task::done(Message::Cache2(CacheMessage::Load {
                id: TileCoord::ZERO,
            }))),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MapProjector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            Message::Cache1(message) => {
                return self.cache1.update(message.clone()).map(Message::Cache1);
            }
            Message::Cache2(message) => {
                return self.cache2.update(message.clone()).map(Message::Cache2);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        row![
            MapWidget::new(&self.cache1, Message::Cache1, self.viewpoint)
                .on_update(Message::MapProjector),
            MapWidget::new(&self.cache2, Message::Cache2, self.viewpoint)
                .on_update(Message::MapProjector),
        ]
    }
}
