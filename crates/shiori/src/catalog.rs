use std::{
    collections::BTreeMap,
    fs::File,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    config::Config, layout::Layout, metadata::Article, rendering::cache::RenderCache, storage,
};

pub const MAX_ARTICLES: usize = 9_999;
pub const MAX_SOURCE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_DISTINCT_TAGS: usize = 9_999;

#[derive(Default, Debug)]
pub struct Snapshot {
    pub articles: BTreeMap<String, Arc<Article>>,
    pub ordered: Vec<Arc<Article>>,
    pub history_token: Option<String>,
    pub tags: BTreeMap<String, usize>,
    pub source_bytes: usize,
    pub revision: u64,
}

pub struct Published {
    pub snapshot: Arc<Snapshot>,
    pub cache: RenderCache,
}

pub struct Catalog {
    pub(crate) root: Arc<File>,
    pub(crate) assets: Arc<crate::asset_root::AssetRoot>,
    layout: Layout,
    root_path: Option<std::path::PathBuf>,
    pub(crate) state: Arc<Mutex<Published>>,
    coordinator: AsyncMutex<()>,
    changes: tokio::sync::watch::Sender<u64>,
    pub(crate) preview: Mutex<Option<Arc<crate::preview::Preview>>>,
    pub(crate) search: crate::search::SearchExecutor,
}

impl Catalog {
    pub async fn open(root: File) -> Result<Arc<Self>> {
        Self::configured(root, Layout::default(), None).await
    }

    pub async fn open_config(config: &Config) -> Result<Arc<Self>> {
        Self::configured(
            storage::root(&config.root)?,
            config.layout.clone(),
            Some(config.root.clone()),
        )
        .await
    }

    async fn configured(
        root: File,
        layout: Layout,
        root_path: Option<std::path::PathBuf>,
    ) -> Result<Arc<Self>> {
        layout.create(&root)?;
        let root = Arc::new(root);
        let assets = Arc::new(crate::asset_root::AssetRoot::new(
            root.clone(),
            layout.assets.clone(),
        )?);
        let scan_root = root.clone();
        let scan_layout = layout.clone();
        let scan_path = root_path.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            scan::configured(&scan_root, &scan_layout, scan_path.as_deref(), None)
        })
        .await??;
        let (changes, _) = tokio::sync::watch::channel(snapshot.revision);
        Ok(Arc::new(Self {
            root,
            assets,
            layout,
            root_path,
            state: Arc::new(Mutex::new(Published {
                snapshot: Arc::new(snapshot),
                cache: RenderCache::new(64 * 1024 * 1024),
            })),
            coordinator: AsyncMutex::new(()),
            changes,
            preview: Mutex::new(None),
            search: crate::search::SearchExecutor::new(),
        }))
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.state
            .lock()
            .expect("catalog lock poisoned")
            .snapshot
            .clone()
    }

    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub fn preview(&self) -> Option<Arc<crate::preview::Preview>> {
        self.preview.lock().expect("preview lock poisoned").clone()
    }

    pub async fn reconcile(&self) -> Result<u64> {
        self.reconcile_mode(false).await
    }

    pub(crate) async fn reconcile_startup(&self) -> Result<u64> {
        self.reconcile_mode(true).await
    }

    async fn reconcile_mode(&self, strict: bool) -> Result<u64> {
        // The guard survives the blocking scan: there is exactly one publisher.
        let _coordinator = self.coordinator.lock().await;
        let old = self.snapshot();
        let root = self.root.clone();
        let layout = self.layout.clone();
        let path = self.root_path.clone();
        let assets = self.assets.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let selected = assets.open()?;
            let mut snapshot = scan::configured(
                &root,
                &layout,
                path.as_deref(),
                if strict { None } else { Some(&old) },
            )?;
            snapshot.revision = old.revision + 1;
            assets.validate(&selected)?;
            Ok::<_, anyhow::Error>(snapshot)
        })
        .await??;
        let revision = snapshot.revision;
        let mut state = self.state.lock().expect("catalog lock poisoned");
        state.cache.retain_current(&snapshot);
        state.snapshot = Arc::new(snapshot);
        self.changes.send_replace(revision);
        Ok(revision)
    }
}

mod scan;

pub fn scan(root: &File, previous: Option<&Snapshot>) -> Result<Snapshot> {
    scan::configured(root, &Layout::default(), None, previous)
}

#[cfg(test)]
use scan::prepare_scan;
#[cfg(test)]
mod tests;
