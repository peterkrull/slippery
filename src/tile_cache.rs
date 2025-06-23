use std::{collections::HashMap, sync::Arc};

use iced::Task;
use iced_core::image::Handle;
use tokio::sync::Semaphore;

use crate::{
    sources::{Attribution, Source},
    tile::TileId,
};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("The semaphore timed out")]
    SemaphoreTimeout,
    #[error("The samaphore was closed")]
    SemaphoreClosed,
}

/// The message that the [`TileCache`] uses to update. It is typically produced when
/// interacting with a [`crate::map_widget::MapWidget`] in order to fetch new tiles,
/// or when the fetching future resolves and responds with its result.
#[derive(Debug, Clone)]
pub enum CacheMessage {
    LoadTile { id: TileId },
    TileLoaded { id: TileId, handle: Handle },
    TileLoadFailed { id: TileId },
}

/// The cache which holds the raster tiles.
/// An application can hold multiple caches with different tile sources
pub struct TileCache {
    cache: HashMap<TileId, Option<Handle>>,
    fetcher: Arc<Fetcher>,
}

impl TileCache {
    /// The [`MapState`] acts acts as a stateful backend for the [`MapWidget`]. It returns an instance
    /// of itself, which should be helt along with the map state, as well as a [`iced::Task`] that should be
    /// executed in order allow the backend to request the application to redraw.
    pub fn new(source: impl Source + 'static) -> Self {
        // For receiving tiles
        Self {
            cache: HashMap::new(),
            fetcher: Arc::new(Fetcher {
                semaphore: Semaphore::new(6),
                source: Box::new(source),
                client: reqwest::ClientBuilder::new()
                    .user_agent("lib-slippery")
                    .build()
                    .unwrap(),
            }),
        }
    }

    pub fn attribution(&self) -> Attribution {
        self.fetcher.source.attribution()
    }

    pub fn tile_size(&self) -> u32 {
        self.fetcher.source.tile_size()
    }

    pub fn max_zoom(&self) -> u8 {
        self.fetcher.source.max_zoom()
    }

    pub fn should_fetch(&self, tile_id: &TileId) -> bool {
        self.cache.get(tile_id).is_none()
    }

    pub fn get(&self, tile_id: &TileId) -> Option<&Handle> {
        self.cache
            .get(tile_id)
            .map(|inner| inner.as_ref())
            .flatten()
    }

    pub fn update(&mut self, update: CacheMessage) -> Task<CacheMessage> {
        match update {
            CacheMessage::TileLoaded { id, handle } => {
                self.cache.insert(id, Some(handle));
            }
            CacheMessage::TileLoadFailed { id } => {
                // Remove entry of failed tile load
                if self.cache.get(&id).is_some_and(|handle| handle.is_none()) {
                    self.cache.remove(&id);
                }
            }
            CacheMessage::LoadTile { id } => {
                if !id.valid() || self.cache.contains_key(&id) {
                    return Task::none();
                }

                // Insert None entry to indicate the tile is being loaded
                if let Some(None) = self.cache.insert(id, None) {
                    return Task::none(); // Already loaded, skip
                }

                let handle = self.fetcher.clone();
                return Task::future(async move {
                    match handle.fetch_tile(id).await {
                        Ok(tile) => CacheMessage::TileLoaded { id, handle: tile },
                        // TODO, handle this error better
                        Err(_) => CacheMessage::TileLoadFailed { id }
                    }
                });
            }
        }

        Task::none()
    }

    // /// Get at tile, or interpolate it from lower zoom levels. This function does not start any
    // /// downloads.
    // fn get_from_cache_or_interpolate(&self, tile_id: TileId) -> Option<ScreenTile> {
    //     let mut zoom_candidate = tile_id.zoom();

    //     loop {
    //         let (zoomed_tile_id, uv) =
    //             interpolate_from_lower_zoom(tile_id, zoom_candidate);

    //         if let Some(Some(texture)) = self.cache.get(&zoomed_tile_id) {
    //             break Some(ScreenTile {
    //                 handle: texture.clone(),
    //                 place: uv,
    //             });
    //         }

    //         // Keep zooming out until we find a donor or there is no more zoom levels.
    //         zoom_candidate = zoom_candidate.checked_sub(1)?;
    //     }
    // }

    // fn interpolate(&self, tile_id L, rectangle: Rectangle) -> Option<(Handle, Rectangle)> {
    //     let mut replacement_tile_id = *self;
    //     while let Some(next_tile_id) = replacement_tile_id.downsample() {
    //         replacement_tile_id = next_tile_id;
    //         if let Some(Some(tile)) = self.map.cache.get(&replacement_tile_id) {

    //             // This tile is already set to be drawn
    //             if draw_cache.contains_key(&replacement_tile_id) {
    //                 break;
    //             }

    //             let dzoom = 2u32.pow((tile_id.zoom() - replacement_tile_id.zoom()) as u32);

    //             let map_center = self.position
    //                 .point
    //                 .into_pixel_space(self.map.fetcher.source.tile_size(), self.position.zoom.f64());

    //             // Determine the offset of this tile relative to the viewport center
    //             let corrected_tile_size = rectangle.width as f64 * dzoom as f64;
    //             let tile_projected = replacement_tile_id.project(corrected_tile_size);
    //             let tile_offset = vec32(tile_projected - map_center);

    //             // The area on the screen the tile will be rendered in
    //             let tile_screen_position = _viewport.center() + tile_offset;
    //             let projected_position = rect(tile_screen_position, corrected_tile_size);

    //             draw_cache.insert(replacement_tile_id, tile, &projected_position);

    //             break;
    //         }
    //     }
    //     None
    // }
}

#[derive(Debug)]
struct Fetcher {
    semaphore: Semaphore,
    source: Box<dyn Source>,
    client: reqwest::Client,
}

impl Fetcher {
    async fn fetch_tile(&self, tile_id: TileId) -> Result<Handle, Error> {
        // Semaphore ensures we are not making too many requests
        // Assume that if we have been locked for more than a second,
        // that the camera may have moved and the tile in no longer needed.
        let _permit =
            tokio::time::timeout(std::time::Duration::from_secs(1), self.semaphore.acquire())
                .await
                .map_err(|_| Error::SemaphoreTimeout)?
                .map_err(|_| Error::SemaphoreClosed)?;

        // Construct the http request
        let source = self.source.tile_url(tile_id);

        // Make request to tile source and get response
        let response = self.client.get(source).send().await?.error_for_status()?;

        // Returns the bytes as an image handle
        let bytes = response.bytes().await?;
        Ok(Handle::from_bytes(bytes))
    }
}
