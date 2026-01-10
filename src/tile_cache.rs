use std::{collections::HashMap, sync::Arc, time::Duration};

use iced::Task;
use iced_core::image::{Allocation, Handle};
use tokio::sync::Semaphore;

use crate::{
    sources::{Attribution, Source},
    tile_coord::TileCoord,
};

#[derive(thiserror::Error, Debug)]
enum FetcherError {
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
    LoadTile {
        id: TileCoord,
    },
    TileLoaded {
        id: TileCoord,
        handle: Handle,
    },
    TileLoadFailed {
        id: TileCoord,
    },
    AllocateTile {
        id: TileCoord,
    },
    TileAllocated {
        id: TileCoord,
        alloc: Allocation,
    },
    TileAllocFailed {
        id: TileCoord,
        err: iced::widget::image::Error,
    },
    DeallocateTile {
        id: TileCoord,
    },
}

#[derive(Debug)]
pub enum TileEntry {
    Loading,
    Loaded(Handle),
    Allocating(Handle),
    #[allow(unused)]
    Allocated(Handle, Allocation),
}

#[derive(Debug)]
/// The cache which holds the raster tiles.
/// An application can hold multiple caches with different tile sources
pub struct TileCache {
    cache: HashMap<TileCoord, TileEntry>,
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

    pub fn should_load(&self, tile_id: &TileCoord) -> bool {
        self.cache.get(tile_id).is_none()
    }

    pub fn should_alloc(&self, tile_id: &TileCoord) -> bool {
        matches!(self.cache.get(tile_id), Some(TileEntry::Loaded(_)))
    }

    pub fn get_drawable(&self, tile_id: &TileCoord) -> Option<(Handle, Allocation)> {
        match self.cache.get(tile_id) {
            Some(TileEntry::Allocated(handle, allocation)) => {
                Some((handle.clone(), allocation.clone()))
            }
            _ => None,
        }
    }

    pub fn update(&mut self, update: CacheMessage) -> Task<CacheMessage> {
        match update {
            CacheMessage::LoadTile { id } => {
                if !id.valid() || self.cache.contains_key(&id) {
                    return Task::none();
                }

                // Insert entry to indicate the tile is being loaded
                self.cache.insert(id, TileEntry::Loading);

                let handle = self.fetcher.clone();
                return Task::future(async move {
                    match handle.fetch_tile(id).await {
                        Ok(tile) => CacheMessage::TileLoaded { id, handle: tile },
                        Err(_) => CacheMessage::TileLoadFailed { id },
                    }
                });
            }
            CacheMessage::TileLoaded { id, handle } => {
                self.cache.insert(id, TileEntry::Loaded(handle.clone()));

                // Immediately allocate tile with the renderer
                return Task::done(CacheMessage::AllocateTile { id });
            }
            CacheMessage::TileLoadFailed { id } => {
                if let Some(TileEntry::Loading) = self.cache.get(&id) {
                    self.cache.remove(&id);
                }
            }
            CacheMessage::AllocateTile { id } => {
                if let Some(entry) = self.cache.get_mut(&id) {
                    if let TileEntry::Loaded(handle) = entry {
                        let alloc_task =
                            iced::widget::image::allocate(handle.clone()).map(move |result| {
                                match result {
                                    Ok(alloc) => CacheMessage::TileAllocated { id, alloc },
                                    Err(err) => CacheMessage::TileAllocFailed { id, err },
                                }
                            });

                        *entry = TileEntry::Allocating(handle.clone());

                        return alloc_task;
                    }
                }
            }
            CacheMessage::TileAllocated {
                id,
                alloc: allocation,
            } => {
                if let Some(entry) = self.cache.get_mut(&id) {
                    if let TileEntry::Allocating(handle) | TileEntry::Loaded(handle) = entry {
                        *entry = TileEntry::Allocated(handle.clone(), allocation);

                        // The allocation is Arc, so widgets will hold on if they need it longer
                        return Task::future(async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            CacheMessage::DeallocateTile { id }
                        });
                    }
                }
            }
            CacheMessage::TileAllocFailed { id, err } => {
                log::error!("Unable to allocate tile {id:?} with renderer: {err:?}");
                if let Some(entry) = self.cache.get_mut(&id) {
                    if let TileEntry::Allocating(handle) = entry {
                        *entry = TileEntry::Loaded(handle.clone());
                    }
                }
            }
            CacheMessage::DeallocateTile { id } => {
                if let Some(entry) = self.cache.get_mut(&id) {
                    if let TileEntry::Allocated(handle, _) = entry {
                        *entry = TileEntry::Loaded(handle.clone());
                    }
                }
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
    async fn fetch_tile(&self, tile_id: TileCoord) -> Result<Handle, FetcherError> {
        // Semaphore ensures we are not making too many requests.
        // Assume that if we have been waiting for a while, that the
        // viewpoint may have moved and the tile in no longer needed.
        // If it was needed, another fetch request will just be made.
        let _permit = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            self.semaphore.acquire(),
        )
        .await
        .map_err(|_| FetcherError::SemaphoreTimeout)?
        .map_err(|_| FetcherError::SemaphoreClosed)?;

        // Construct the http request
        let source = self.source.tile_url(tile_id);

        // Make request to tile source and get response
        let response = self.client.get(source).send().await?.error_for_status()?;

        // Returns the bytes as an image handle
        let bytes = response.bytes().await?;
        Ok(Handle::from_bytes(bytes))
    }
}
