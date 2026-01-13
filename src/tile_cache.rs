use std::{
    cell::Cell,
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use iced::Task;
use iced_core::image::{self, Allocation, Handle};
use tokio::sync::Semaphore;

use crate::{
    sources::{Attribution, Source},
    tile_coord::TileCoord,
};

const PRUNE_TIME: Duration = Duration::from_secs(60);
const PRUNE_THRESH: usize = 1024;

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
    Load {
        id: TileCoord,
    },
    Loaded {
        id: TileCoord,
        handle: Handle,
    },
    LoadFailed {
        id: TileCoord,
    },
    Allocate {
        id: TileCoord,
    },
    Allocated {
        id: TileCoord,
        alloc: Allocation,
    },
    AllocFailed {
        id: TileCoord,
        err: image::Error,
    },
    Deallocate {
        id: TileCoord,
    },
    Prune,
}

#[derive(Debug)]
pub enum State {
    Loading,
    Loaded(Handle),
    Allocating(Handle),
    Allocated(Handle, Allocation),
}

#[derive(Debug)]
struct Entry {
    state: State,
    last_used: Cell<Instant>,
}

impl Entry {
    fn new(entry: State) -> Self {
        Self {
            state: entry,
            last_used: Cell::new(Instant::now()),
        }
    }

    fn touch(&self) {
        self.last_used.set(Instant::now());
    }
}

#[derive(Debug)]
/// The cache which holds the raster tiles.
/// An application can hold multiple caches with different tile sources
pub struct TileCache {
    cache: HashMap<TileCoord, Entry>,
    fetcher: Arc<HttpFetcher>,
    cleanup_timer: Instant,
}

impl TileCache {
    /// The [`MapState`] acts acts as a stateful backend for the [`crate::MapWidget`]. It returns an instance
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
            cleanup_timer: Instant::now(),
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
        if let Some(entry) = self.cache.get(tile_id) {
            entry.touch();
            false
        } else {
            true
        }
    }

    pub fn should_alloc(&self, tile_id: &TileCoord) -> bool {
        if let Some(entry) = self.cache.get(tile_id) {
            entry.touch();
            matches!(entry.state, State::Loaded(_))
        } else {
            false
        }
    }

    pub fn get_drawable(&self, tile_id: &TileCoord) -> Option<(Handle, Allocation)> {
        let entry = self.cache.get(tile_id)?;
        match entry {
            Entry {
                state: State::Allocated(handle, allocation),
                ..
            } => {
                entry.touch();
                Some((handle.clone(), allocation.clone()))
            }
            _ => None,
        }
    }

    pub fn update(&mut self, update: CacheMessage) -> Task<CacheMessage> {
        // Periodically schedule a prune
        let mut cleanup_task = Task::none();
        if self.cache.len() > PRUNE_THRESH && self.cleanup_timer.elapsed() > Duration::from_secs(5)
        {
            self.cleanup_timer = Instant::now();
            cleanup_task = Task::done(CacheMessage::Prune);
        }

        let task = match update {
            CacheMessage::Prune => {
                let start_time = Instant::now();
                let start_size = self.cache.len();
                let mut prune_count = 0;
                let prune_target = start_size - PRUNE_THRESH;
                self.cache.retain(|id, v| {
                    if prune_count >= prune_target {
                        return true;
                    }

                    let retain = match v.state {
                        State::Loading | State::Allocating(_) => true,
                        // Keep the most zoomed out tiles
                        _ if id.zoom() < 6 => true,
                        _ => start_time
                            .checked_duration_since(v.last_used.get())
                            .is_none_or(|diff| diff < PRUNE_TIME),
                    };
                    if !retain {
                        prune_count += 1
                    }
                    retain
                });
                let elapsed = start_time.elapsed();
                let pruned = start_size - self.cache.len();
                println!(
                    "Time to prune: {elapsed:?}, pruned {pruned}, down from {start_size}, now {}",
                    self.cache.len()
                );
                Task::none()
            }
            CacheMessage::Load { id } => {
                if self.cache.contains_key(&id) {
                    Task::none()
                } else {
                    // Insert entry to indicate the tile is being loaded
                    self.cache.insert(id, Entry::new(State::Loading));

                    let handle = self.fetcher.clone();
                    Task::future(async move {
                        match handle.fetch_tile(id).await {
                            Ok(tile) => CacheMessage::Loaded { id, handle: tile },
                            Err(_) => CacheMessage::LoadFailed { id },
                        }
                    })
                }
            }
            CacheMessage::Loaded { id, handle } => {
                self.cache
                    .insert(id, Entry::new(State::Loaded(handle.clone())));

                // Immediately allocate tile with the renderer
                Task::done(CacheMessage::Allocate { id })
            }
            CacheMessage::LoadFailed { id } => {
                if let Some(Entry {
                    state: State::Loading,
                    ..
                }) = self.cache.get(&id)
                {
                    self.cache.remove(&id);
                }
                Task::none()
            }
            CacheMessage::Allocate { id } => {
                if let Some(entry) = self.cache.get_mut(&id)
                    && let State::Loaded(handle) = &entry.state
                {
                    let alloc_task = iced::widget::image::allocate(handle.clone()).map(
                        move |result| match result {
                            Ok(alloc) => CacheMessage::Allocated { id, alloc },
                            Err(err) => CacheMessage::AllocFailed { id, err },
                        },
                    );

                    entry.state = State::Allocating(handle.clone());
                    entry.touch();

                    alloc_task
                } else {
                    Task::none()
                }
            }
            CacheMessage::Allocated {
                id,
                alloc: allocation,
            } => {
                if let Some(entry) = self.cache.get_mut(&id) {
                    let mut auto_dealloc_task = Task::none();
                    match &entry.state {
                        State::Allocating(handle) | State::Loaded(handle) => {
                            entry.state = State::Allocated(handle.clone(), allocation);
                            entry.touch();

                            // The allocation is Arc, so widgets will hold on if they need it longer
                            // Except for the lowest zoom levels, keep those allocated as a last resort 
                            if id.zoom() > 1 {
                                auto_dealloc_task = Task::future(async move {
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                    CacheMessage::Deallocate { id }
                                });
                            }
                        }
                        _ => {}
                    }
                    auto_dealloc_task
                } else {
                    Task::none()
                }
            }
            CacheMessage::AllocFailed { id, err } => {
                log::error!("Unable to allocate tile {id:?} with renderer: {err:?}");
                if let Some(entry) = self.cache.get_mut(&id)
                    && let State::Allocating(handle) = &entry.state
                {
                    entry.state = State::Loaded(handle.clone());
                }

                Task::none()
            }
            CacheMessage::Deallocate { id } => {
                if let Some(entry) = self.cache.get_mut(&id) {
                    // Downgrade from Allocated to Loaded by dropping the Allocation
                    if let State::Allocated(handle, _) = &entry.state {
                        entry.state = State::Loaded(handle.clone());
                    }
                }
                Task::none()
            }
        };

        Task::batch([cleanup_task, task])
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
