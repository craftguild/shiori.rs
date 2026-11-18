pub mod cache;
mod derivation;
mod images;
mod profile;
pub use profile::Profile;

use crate::{
    catalog::Published, config::Config, document::Document, images::ImageService, metadata::Article,
};
use cache::Key;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{OnceCell, Semaphore, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFailure;
impl std::fmt::Display for RenderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("article document failed")
    }
}
impl std::error::Error for RenderFailure {}
pub type Projection = Arc<Document>;
pub type RenderResult = Result<Projection, RenderFailure>;
type ParsedCell = Arc<OnceLock<Arc<oriha_core::ParsedDocument>>>;
#[derive(Default)]
pub struct RequestMemo {
    values: Mutex<HashMap<Key, Arc<OnceCell<RenderResult>>>>,
    parsed: Mutex<HashMap<Key, ParsedCell>>,
}
type Flight = watch::Sender<Option<RenderResult>>;

pub struct Renderer {
    config: Arc<Config>,
    state: Arc<Mutex<Published>>,
    profile: [u8; 32],
    flights: Mutex<HashMap<Key, Flight>>,
    workers: Arc<Semaphore>,
    calls: AtomicUsize,
    parses: AtomicUsize,
    pub images: Arc<ImageService>,
    #[cfg(test)]
    probe: Mutex<Option<Arc<tests::Gate>>>,
}
impl Renderer {
    pub fn new(config: Arc<Config>, catalog: &crate::catalog::Catalog) -> Arc<Self> {
        let capacity = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .clamp(1, 4);
        let profile = Sha256::digest(format!("cms-document-v2;images-v1;gfm,callouts,math,mermaid,plantuml;raw=false;limits=default;locale=en;{};watch={}", config.public_base, config.watch)).into();
        Arc::new(Self {
            config,
            state: catalog.state.clone(),
            profile,
            flights: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(capacity)),
            calls: AtomicUsize::new(0),
            parses: AtomicUsize::new(0),
            images: ImageService::new(catalog.assets.clone()),
            #[cfg(test)]
            probe: Mutex::new(None),
        })
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
    pub fn parses(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }
    pub fn available_workers(&self) -> usize {
        self.workers.available_permits()
    }
    pub fn active_keys(&self) -> usize {
        self.flights.lock().expect("flight lock poisoned").len()
    }

    pub async fn document(
        self: &Arc<Self>,
        article: Arc<Article>,
        memo: &RequestMemo,
        profile: Profile,
    ) -> RenderResult {
        let parse_key = Key {
            id: article.id.clone(),
            hash: article.hash,
            profile: self.profile,
        };
        let parsed = memo
            .parsed
            .lock()
            .expect("parse memo lock poisoned")
            .entry(parse_key.clone())
            .or_default()
            .clone();
        let mut key = parse_key;
        let mut digest = Sha256::new();
        digest.update(self.profile);
        digest.update([profile.bits()]);
        key.profile = digest.finalize().into();
        let cell = memo
            .values
            .lock()
            .expect("memo lock poisoned")
            .entry(key.clone())
            .or_default()
            .clone();
        cell.get_or_init(|| self.shared(key, article, parsed, profile))
            .await
            .clone()
    }

    async fn shared(
        self: &Arc<Self>,
        key: Key,
        article: Arc<Article>,
        parsed: ParsedCell,
        profile: Profile,
    ) -> RenderResult {
        if let Some(cached) = self.cached(&key).await {
            return Ok(cached);
        }
        let existing = self
            .flights
            .lock()
            .expect("flight lock poisoned")
            .get(&key)
            .map(Flight::subscribe);
        if let Some(receiver) = existing {
            return receive(receiver).await;
        }
        // Admission precedes retained flight keys and workers. Dropping this wait is safe.
        let permit = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| RenderFailure)?;
        if let Some(cached) = self.cached(&key).await {
            return Ok(cached);
        }
        let (receiver, is_owner) = {
            let mut flights = self.flights.lock().expect("flight lock poisoned");
            if let Some(sender) = flights.get(&key) {
                (sender.subscribe(), false)
            } else {
                let (sender, receiver) = watch::channel(None);
                flights.insert(key.clone(), sender);
                (receiver, true)
            }
        };
        if !is_owner {
            drop(permit);
            return receive(receiver).await;
        }
        let owner = self.clone();
        // The supervisor owns cleanup even if every HTTP waiter is cancelled.
        tokio::spawn(async move {
            let worker_owner = owner.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                worker_owner.calls.fetch_add(1, Ordering::Relaxed);
                #[cfg(test)]
                {
                    let probe = worker_owner.probe.lock().unwrap().clone();
                    if let Some(probe) = probe {
                        probe.wait();
                    }
                }
                let parsed = parsed.get_or_init(|| {
                    worker_owner.parses.fetch_add(1, Ordering::Relaxed);
                    Arc::new(derivation::parse(&article))
                });
                derivation::derive(
                    &worker_owner.config,
                    &article,
                    &worker_owner.images,
                    parsed,
                    profile,
                )
            })
            .await;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    tracing::error!(event="document_worker_failed",article_id=%key.id,%error);
                    Err(RenderFailure)
                }
            };
            if let Ok(value) = &result
                && value.cacheable
                && owner.images.valid(value.dependencies.clone()).await
            {
                let mut state = owner.state.lock().expect("catalog lock poisoned");
                if state
                    .snapshot
                    .articles
                    .get(&key.id)
                    .is_some_and(|article| article.hash == key.hash)
                {
                    state.cache.insert(key.clone(), value.clone());
                }
            }
            if let Some(sender) = owner
                .flights
                .lock()
                .expect("flight lock poisoned")
                .remove(&key)
            {
                sender.send_replace(Some(result));
            }
        });
        receive(receiver).await
    }
    async fn cached(&self, key: &Key) -> Option<Projection> {
        let cached = self
            .state
            .lock()
            .expect("catalog lock poisoned")
            .cache
            .get(key)?;
        self.images
            .valid(cached.dependencies.clone())
            .await
            .then_some(cached)
    }
}
async fn receive(mut receiver: watch::Receiver<Option<RenderResult>>) -> RenderResult {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver.changed().await.map_err(|_| RenderFailure)?;
    }
}
#[cfg(test)]
mod tests;
