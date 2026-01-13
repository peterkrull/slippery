use iced::widget::canvas::{Path, Stroke};
use iced::{self, Color, Element, Task};
use slippery::{
    CacheMessage, Geographic, MapProgram, Projector, TileCache, TileCoord, Viewpoint, Zoom,
    sources::OpenStreetMap,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Error)
        .filter_module("slippery", log::LevelFilter::Debug)
        .init();

    iced::application(Application::boot, Application::update, Application::view)
        .title("Slippery - Drawing Example")
        .run()
        .unwrap();
}

#[derive(Debug, Clone)]
enum Message {
    Cache(CacheMessage),
    Projector(Projector),
}

struct Application {
    cache: TileCache,
    viewpoint: Viewpoint,
}

const PARIS: Geographic = Geographic::new(2.3522, 48.8566);
const LONDON: Geographic = Geographic::new(-0.1278, 51.5074);
const BERLIN: Geographic = Geographic::new(13.4050, 52.5200);
const ROME: Geographic = Geographic::new(12.4964, 41.9028);
const MADRID: Geographic = Geographic::new(-3.7038, 40.4168);
const VIENNA: Geographic = Geographic::new(16.3738, 48.2082);

impl Application {
    pub fn boot() -> (Self, Task<Message>) {
        (
            Application {
                cache: TileCache::new(OpenStreetMap),
                viewpoint: Viewpoint {
                    // Start view centered on Europe
                    position: Geographic::new(10.0, 50.0).as_mercator(),
                    zoom: Zoom::try_from(4.0).unwrap(),
                },
            },
            Task::done(Message::Cache(CacheMessage::Load {
                id: TileCoord::ZERO,
            })),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Projector(projector) => {
                self.viewpoint = projector.viewpoint;
            }
            Message::Cache(message) => {
                return self.cache.update(message).map(Message::Cache);
            }
        }

        Task::none()
    }

    pub fn view(&self) -> impl Into<Element<'_, Message>> {
        MapProgram::new(&self.cache)
            .on_cache(Message::Cache)
            .on_update(Message::Projector)
            .with_draw_layer(|projector, frame| {
                // Line connecting Paris and London
                let p1 = projector.geographic_into_screen_space(PARIS);
                let p2 = projector.geographic_into_screen_space(LONDON);

                let line = Path::line(p1, p2);
                let stroke = Stroke::default()
                    .with_width(3.0)
                    .with_color(Color::from_rgb(1.0, 0.0, 0.0));
                frame.stroke(&line, stroke);

                // Triangle filling the area between Berlin, Paris and Rome
                let p1 = projector.geographic_into_screen_space(BERLIN);
                let p2 = projector.geographic_into_screen_space(PARIS);
                let p3 = projector.geographic_into_screen_space(ROME);

                let triangle = Path::new(|builder| {
                    builder.move_to(p1);
                    builder.line_to(p2);
                    builder.line_to(p3);
                    builder.close();
                });

                frame.fill(&triangle, Color::from_rgba(1.0, 0.8, 0.0, 0.4));

                let poly_stroke = Stroke::default()
                    .with_width(2.0)
                    .with_color(Color::from_rgb(1.0, 0.8, 0.0));
                frame.stroke(&triangle, poly_stroke);

                // Draw a bezier curve between Madrid and Vienna
                let start = MADRID;
                let end = VIENNA;

                // Calculate midpoint in geographic coordinates
                let mid_lat = (start.latitude() + end.latitude()) / 2.0;
                let mid_lon = (start.longitude() + end.longitude()) / 2.0;

                // Create control points offset from the midpoint
                let lat_offset = (end.latitude() - start.latitude()).abs() * 0.5;

                let control1_geo = Geographic::new(
                    start.longitude() + (mid_lon - start.longitude()) * 0.5,
                    start.latitude() + (mid_lat - start.latitude()) * 0.5 + lat_offset,
                );
                let control2_geo = Geographic::new(
                    end.longitude() - (end.longitude() - mid_lon) * 0.5,
                    end.latitude() - (end.latitude() - mid_lat) * 0.5 - lat_offset,
                );

                // Convert all points to screen space
                let p_start = projector.geographic_into_screen_space(start);
                let p_end = projector.geographic_into_screen_space(end);
                let p_control1 = projector.geographic_into_screen_space(control1_geo);
                let p_control2 = projector.geographic_into_screen_space(control2_geo);

                let curve = Path::new(|builder| {
                    builder.move_to(p_start);
                    builder.bezier_curve_to(p_control1, p_control2, p_end);
                });

                let curve_stroke = Stroke::default()
                    .with_width(2.5)
                    .with_color(Color::from_rgb(0.0, 0.8, 0.5));
                frame.stroke(&curve, curve_stroke);

                // Draw circles at major European cities
                let cities = vec![PARIS, LONDON, BERLIN, ROME, MADRID, VIENNA];

                for city in cities {
                    let pos = projector.geographic_into_screen_space(city);
                    let circle = Path::circle(pos, 8.0);
                    frame.fill(&circle, Color::from_rgb(0.0, 0.5, 1.0));

                    // Add a border to the circle
                    let border_stroke = Stroke::default().with_width(2.0).with_color(Color::WHITE);
                    frame.stroke(&circle, border_stroke);
                }
            })
            .build(self.viewpoint)
    }
}
