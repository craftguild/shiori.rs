use super::{
    AssetPath, Dependency, Error, Info, POLICY, SourceState, Variant, Width, accept,
    cache::{Cache, Key, Lookup},
    codec, source,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct ImageService {
    root: Arc<crate::asset_root::AssetRoot>,
    workers: Arc<Semaphore>,
    waiting: Arc<Semaphore>,
    cache: Arc<Mutex<Cache>>,
    inspections: AtomicUsize,
    transforms: AtomicUsize,
    #[cfg(test)]
    pub(crate) probe: Mutex<Option<Arc<super::tests::Probe>>>,
    #[cfg(test)]
    pub(crate) flight_waiters: AtomicUsize,
}

impl ImageService {
    pub(crate) fn new(root: Arc<crate::asset_root::AssetRoot>) -> Arc<Self> {
        let count = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .clamp(1, 2);
        Self::with_source(root, count, 64 * 1024 * 1024)
    }
    #[cfg(test)]
    pub(super) fn configured(root: Arc<std::fs::File>, workers: usize, budget: usize) -> Arc<Self> {
        Self::with_source(
            Arc::new(
                crate::asset_root::AssetRoot::new(root, Default::default())
                    .expect("test asset root"),
            ),
            workers,
            budget,
        )
    }
    fn with_source(
        root: Arc<crate::asset_root::AssetRoot>,
        workers: usize,
        budget: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            root,
            workers: Arc::new(Semaphore::new(workers)),
            waiting: Arc::new(Semaphore::new(64)),
            cache: Arc::new(Mutex::new(Cache::new(budget))),
            inspections: AtomicUsize::new(0),
            transforms: AtomicUsize::new(0),
            #[cfg(test)]
            probe: Mutex::new(None),
            #[cfg(test)]
            flight_waiters: AtomicUsize::new(0),
        })
    }
    pub fn inspections(&self) -> usize {
        self.inspections.load(Ordering::Relaxed)
    }
    pub fn transforms(&self) -> usize {
        self.transforms.load(Ordering::Relaxed)
    }
    pub fn available_workers(&self) -> usize {
        self.workers.available_permits()
    }
    pub fn queued(&self) -> usize {
        64 - self.waiting.available_permits()
    }
    pub fn cache_bytes(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }
    pub fn active_keys(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .flights
            .len()
    }
    pub fn begin_shutdown(&self) {
        self.waiting.close();
        self.workers.close();
    }
    #[cfg(test)]
    pub(crate) fn checkpoint(&self, event: &str) {
        let probe = self.probe.lock().unwrap().clone();
        if let Some(probe) = probe {
            probe.wait(event);
        }
    }

    async fn admission(&self) -> Result<OwnedSemaphorePermit, Error> {
        if let Ok(permit) = self.workers.clone().try_acquire_owned() {
            return Ok(permit);
        }
        let waiting = self
            .waiting
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let permit = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Busy)?;
        drop(waiting);
        Ok(permit)
    }
    async fn run<T: Send + 'static>(
        self: &Arc<Self>,
        job: impl FnOnce(&Self) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let permit = self.admission().await?;
        let owner = self.clone();
        // A blocking task owns its permit even when every async waiter is cancelled.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            job(&owner)
        })
        .await
        .map_err(|error| {
            tracing::error!(event="image_worker_failed",%error);
            Error::Internal
        })?
    }
    pub(crate) async fn inspect(self: &Arc<Self>, path: AssetPath) -> (Dependency, Option<Info>) {
        let saved = path.clone();
        let result = self
            .run(move |owner| {
                owner.inspections.fetch_add(1, Ordering::Relaxed);
                match source::read(&owner.root, &path) {
                    Ok((bytes, hash)) => {
                        let info = match codec::decode(&bytes) {
                            Ok(decoded) => Some(decoded.info),
                            Err(error) => {
                                tracing::debug!(event="image_inspection_fallback",path=%path.0,%error);
                                None
                            }
                        };
                        #[cfg(test)]
                        owner.checkpoint("inspected");
                        Ok((
                            Dependency {
                                path,
                                state: SourceState::Hash(hash),
                            },
                            info,
                        ))
                    }
                    Err(Error::Missing) => Ok((
                        Dependency {
                            path,
                            state: SourceState::Missing,
                        },
                        None,
                    )),
                    Err(error) => Err(error),
                }
            })
            .await;
        result.unwrap_or_else(|error| {
            tracing::debug!(event="image_inspection_unavailable",%error,path=%saved.0);
            (
                Dependency {
                    path: saved,
                    state: SourceState::Unstable,
                },
                None,
            )
        })
    }
    pub(crate) async fn valid(self: &Arc<Self>, dependencies: Vec<Dependency>) -> bool {
        if dependencies.is_empty() {
            return true;
        }
        self.run(move |owner| {
            Ok(dependencies.iter().all(|dependency| {
                dependency.state != SourceState::Unstable
                    && source::state(&owner.root, &dependency.path) == dependency.state
            }))
        })
        .await
        .unwrap_or(false)
    }
    pub(crate) async fn variant(
        self: &Arc<Self>,
        path: AssetPath,
        width: Width,
        accept: Option<String>,
    ) -> Result<Arc<Variant>, Error> {
        self.run(move |owner| {
            let (bytes, hash) = source::read(&owner.root, &path)?;
            // Inspect/validate within the same admitted operation as encoding.
            let header = codec::header(&bytes)?;
            let width = width.effective(header.info.width);
            let format = accept::negotiate(
                accept.as_deref(),
                header.fallback(),
                header.webp_available(width),
            )?;
            let key = Key {
                hash,
                width,
                format,
                policy: POLICY,
            };
            match Cache::lookup(owner.cache.clone(), key) {
                Lookup::Cached(value) => Ok(value),
                Lookup::Wait(flight) => {
                    drop(header);
                    drop(bytes);
                    #[cfg(test)]
                    owner.flight_waiters.fetch_add(1, Ordering::Relaxed);
                    let result = flight.wait();
                    #[cfg(test)]
                    owner.flight_waiters.fetch_sub(1, Ordering::Relaxed);
                    result
                }
                Lookup::Own(flight) => {
                    owner.transforms.fetch_add(1, Ordering::Relaxed);
                    #[cfg(test)]
                    owner.checkpoint("encode");
                    let result = header
                        .decode(&bytes)
                        .and_then(|decoded| decoded.encode(bytes, width, format));
                    flight.finish(result.map(Arc::new))
                }
            }
        })
        .await
    }
}
