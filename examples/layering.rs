use iced::widget::canvas::Path;
use iced::{self, Color, Point, Size, Task, Widget};
use slippery::{
    CacheMessage, Geodetic, MapProgram, Projector, TileCache, TileCoord, Viewpoint, Zoom,
    sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery - Visual Layering")
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

const POSITION: Geodetic = slippery::location::paris();

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint {
                    position: POSITION.as_mercator(),
                    zoom: Zoom::try_from(10.).unwrap(),
                },
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
        }

        Task::none()
    }

    pub fn view(&self) -> impl Widget<Message> {
        MapProgram::new(&self.cache)
            .on_cache(Message::Cache)
            .on_update(Message::MapProjector)
            .with_visual_layer(move |_, frame| {
                const OFFSET: f32 = 250.0;

                let shrunk_size = Size::new(
                    (frame.size().width - OFFSET * 2.0).max(0.0),
                    (frame.size().height - OFFSET * 2.0).max(0.0),
                );

                let square = Path::rectangle(Point::new(OFFSET, OFFSET), shrunk_size);
                frame.fill(&square, Color::from_rgba(0.5, 0.5, 0.5, 1.0));
            })
            .with_visual_layer(move |projector, frame| {
                let top_left = projector.geodetic_into_screen_space(Geodetic::new(
                    POSITION.longitude() - 0.1,
                    POSITION.latitude() - 0.1,
                ));
                let bottom_right = projector.geodetic_into_screen_space(Geodetic::new(
                    POSITION.longitude() + 0.1,
                    POSITION.latitude() + 0.1,
                ));

                let size = Size::from(bottom_right - top_left);

                let triangle = Path::rectangle(top_left, size);
                frame.fill(&triangle, Color::from_rgba(0.0, 0.8, 0.8, 1.0));
            })
            .with_visual_layer(move |_, frame| {
                const OFFSET: f32 = 350.0;

                let shrunk_size = Size::new(
                    (frame.size().width - OFFSET * 2.0).max(0.0),
                    (frame.size().height - OFFSET * 2.0).max(0.0),
                );

                let square = Path::rectangle(Point::new(OFFSET, OFFSET), shrunk_size);
                frame.fill(&square, Color::from_rgba(0.2, 0.2, 0.2, 1.0));
            })
            .build(self.viewpoint)
    }
}
