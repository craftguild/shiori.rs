pub mod cache;
mod derivation;
mod images;

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use oriha_core::BlockOutputStatus;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::{OnceCell, Semaphore, watch};

use crate::{
    catalog::Published,
    config::Config,
    images::{Dependency, ImageService},
    metadata::Article,
};
use cache::Key;

#[derive(Clone, Debug, Serialize)]
pub struct HtmlBlock {
    pub id: String,
    pub kind: String,
    pub html: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Asset {
    pub id: String,
    pub kind: String,
    pub mime_type: String,
    pub content: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Rendered {
    pub blocks: Vec<HtmlBlock>,
    pub assets: Vec<Asset>,
    #[serde(skip)]
    pub(crate) dependencies: Vec<Dependency>,
    #[serde(skip)]
    pub(crate) cacheable: bool,
}

impl Rendered {
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + self.blocks.capacity() * std::mem::size_of::<HtmlBlock>()
            + self.assets.capacity() * std::mem::size_of::<Asset>()
            + self.dependencies.capacity() * std::mem::size_of::<Dependency>()
            + self
                .dependencies
                .iter()
                .map(|d| d.retained_bytes() - std::mem::size_of::<Dependency>())
                .sum::<usize>()
            + self
                .blocks
                .iter()
                .map(|b| b.id.capacity() + b.kind.capacity() + b.html.capacity())
                .sum::<usize>()
            + self
                .assets
                .iter()
                .map(|a| {
                    a.id.capacity()
                        + a.kind.capacity()
                        + a.mime_type.capacity()
                        + a.content.capacity()
                })
                .sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFailure;
impl std::fmt::Display for RenderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("article rendering failed")
    }
}
impl std::error::Error for RenderFailure {}
pub type RenderResult = Result<Arc<Rendered>, RenderFailure>;
#[derive(Clone, Debug)]
pub enum Projection {
    Html(Arc<Rendered>),
    Structured(Arc<crate::structured::Document>),
}
impl Projection {
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Html(v) => v.retained_bytes(),
            Self::Structured(v) => v.retained_bytes(),
        }
    }
    fn html(&self) -> Option<&Rendered> {
        match self {
            Self::Html(v) => Some(v),
            _ => None,
        }
    }
}
type ProjectionResult = Result<Projection, RenderFailure>;
type ParsedCell = Arc<OnceLock<Arc<oriha_core::ParsedDocument>>>;
#[derive(Default)]
pub struct RequestMemo {
    values: Mutex<HashMap<Key, Arc<OnceCell<ProjectionResult>>>>,
    parsed: Mutex<HashMap<Key, ParsedCell>>,
}
type Flight = watch::Sender<Option<ProjectionResult>>;

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
        let profile = Sha256::digest(format!("cms-render-v3;images-v1;gfm,callouts,syntax,math,mermaid,plantuml;raw=false;limits=default;locale=en;code-ui=language,line-numbers;{};watch={}", config.public_base, config.watch)).into();
        Arc::new(Self {
            config,
            state: catalog.state.clone(),
            profile,
            flights: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(capacity)),
            calls: AtomicUsize::new(0),
            parses: AtomicUsize::new(0),
            images: ImageService::new(catalog.root.clone()),
            #[cfg(test)]
            probe: Mutex::new(None),
        })
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
    pub fn available_workers(&self) -> usize {
        self.workers.available_permits()
    }
    pub fn active_keys(&self) -> usize {
        self.flights.lock().expect("flight lock poisoned").len()
    }

    pub async fn render(
        self: &Arc<Self>,
        article: Arc<Article>,
        memo: &RequestMemo,
    ) -> RenderResult {
        match self.projection(article, memo, false).await? {
            Projection::Html(value) => Ok(value),
            _ => Err(RenderFailure),
        }
    }
    pub async fn structure(
        self: &Arc<Self>,
        article: Arc<Article>,
        memo: &RequestMemo,
    ) -> Result<Arc<crate::structured::Document>, RenderFailure> {
        match self.projection(article, memo, true).await? {
            Projection::Structured(value) => Ok(value),
            _ => Err(RenderFailure),
        }
    }
    pub fn parses(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }
    async fn projection(
        self: &Arc<Self>,
        article: Arc<Article>,
        memo: &RequestMemo,
        structured: bool,
    ) -> ProjectionResult {
        let parse_key = Key {
            id: article.id,
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
        if structured {
            key.profile =
                Sha256::digest([self.profile.as_slice(), b"structured-v1"].concat()).into();
        }
        let cell = memo
            .values
            .lock()
            .expect("memo lock poisoned")
            .entry(key.clone())
            .or_default()
            .clone();
        cell.get_or_init(|| self.shared(key, article, parsed, structured))
            .await
            .clone()
    }

    async fn shared(
        self: &Arc<Self>,
        key: Key,
        article: Arc<Article>,
        parsed: ParsedCell,
        structured: bool,
    ) -> ProjectionResult {
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
        // Acquire before creating retained workers/keys. Dropping this wait is safe.
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
        // The supervisor owns cleanup even when every HTTP waiter is cancelled.
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
                if structured {
                    derivation::structure(parsed).map(Projection::Structured)
                } else {
                    derivation::derive(&worker_owner.config, &article, &worker_owner.images, parsed)
                        .map(Projection::Html)
                }
            })
            .await;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    tracing::error!(event="render_worker_failed", article_id=key.id, %error);
                    Err(RenderFailure)
                }
            };
            if let Ok(value) = &result
                && match value.html() {
                    Some(html) => {
                        html.cacheable && owner.images.valid(html.dependencies.clone()).await
                    }
                    None => true,
                }
            {
                let mut state = owner.state.lock().expect("catalog lock poisoned");
                if state
                    .snapshot
                    .articles
                    .get(&key.id)
                    .is_some_and(|a| a.hash == key.hash)
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
        if match cached.html() {
            Some(html) => self.images.valid(html.dependencies.clone()).await,
            None => true,
        } {
            Some(cached)
        } else {
            None
        }
    }
}

async fn receive(mut receiver: watch::Receiver<Option<ProjectionResult>>) -> ProjectionResult {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver.changed().await.map_err(|_| RenderFailure)?;
    }
}

fn project(output: oriha_core::RenderedBlockOutput) -> RenderResult {
    if output.status != BlockOutputStatus::Complete {
        return Err(RenderFailure);
    }
    let mut blocks = Vec::new();
    for block in output.blocks {
        if let Some(html) = block.html() {
            let kind = serde_json::to_value(block.kind).map_err(|_| RenderFailure)?;
            blocks.push(HtmlBlock {
                id: block.id.clone(),
                kind: kind.as_str().ok_or(RenderFailure)?.into(),
                html: html.into(),
            });
        }
    }
    let assets = output
        .assets
        .into_iter()
        .map(|a| {
            let kind = serde_json::to_value(a.kind).map_err(|_| RenderFailure)?;
            Ok(Asset {
                id: a.id,
                kind: kind.as_str().ok_or(RenderFailure)?.into(),
                mime_type: a.mime_type,
                content: a.content,
            })
        })
        .collect::<Result<_, RenderFailure>>()?;
    Ok(Arc::new(Rendered {
        blocks,
        assets,
        dependencies: vec![],
        cacheable: true,
    }))
}

#[cfg(test)]
mod tests;
