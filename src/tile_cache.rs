use std::{collections::HashMap, sync::Arc};

use iced::Task;
use iced_core::image::Handle;
use tokio::sync::Semaphore;

use crate::{
    sources::{Attribution, Source},
    tile_coord::TileCoord,
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
    LoadTile { id: TileCoord },
    TileLoaded { id: TileCoord, handle: Handle },
    TileLoadFailed { id: TileCoord },
}

#[derive(Debug)]
/// The cache which holds the raster tiles.
/// An application can hold multiple caches with different tile sources
pub struct TileCache {
    cache: HashMap<TileCoord, Option<Handle>>,
    fetcher: Arc<HttpFetcher>,
}

impl TileCache {
    /// The [`MapState`] acts acts as a stateful backend for the [`MapWidget`]. It returns an instance
    /// of itself, which should be helt along with the map state, as well as a [`iced::Task`] that should be
    /// executed in order allow the backend to request the application to redraw.
    pub fn new(source: impl Source + 'static) -> Self {
        // For receiving tiles
        Self {
            cache: HashMap::new(),
            fetcher: Arc::new(HttpFetcher {
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

    pub fn should_fetch(&self, tile_id: &TileCoord) -> bool {
        self.cache.get(tile_id).is_none()
    }

    pub fn get(&self, tile_id: &TileCoord) -> Option<&Handle> {
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
                        Err(_) => CacheMessage::TileLoadFailed { id },
                    }
                });
            }
        }

        Task::none()
    }
}

/// The fetcher is cloned and moved into an async task to fetch a tile.
#[derive(Debug)]
struct HttpFetcher {
    semaphore: Semaphore,
    source: Box<dyn Source>,
    client: reqwest::Client,
}

impl HttpFetcher {
    async fn fetch_tile(&self, tile_id: TileCoord) -> Result<Handle, Error> {
        // Semaphore ensures we are not making too many requests
        // Assume that if we have been locked for more than a second,
        // that the camera may have moved and the tile in no longer needed.
        // If it was needed, another fetch request will just be made.
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
