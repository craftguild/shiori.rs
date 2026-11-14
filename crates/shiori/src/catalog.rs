use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, ensure};
use rustix::fs::{AtFlags, FileType, statat};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    metadata::{self, Article},
    rendering::cache::RenderCache,
    storage,
};

pub const MAX_ARTICLES: usize = 9_999;
pub const MAX_SOURCE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_DISTINCT_TAGS: usize = 9_999;

#[derive(Default, Debug)]
pub struct Snapshot {
    pub articles: BTreeMap<i64, Arc<Article>>,
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
    pub(crate) state: Arc<Mutex<Published>>,
    coordinator: AsyncMutex<()>,
    changes: tokio::sync::watch::Sender<u64>,
    pub(crate) preview: Mutex<Option<Arc<crate::preview::Preview>>>,
    pub(crate) search: crate::search::SearchExecutor,
}

impl Catalog {
    pub async fn open(root: File) -> Result<Arc<Self>> {
        let root = Arc::new(root);
        let scan_root = root.clone();
        let snapshot = tokio::task::spawn_blocking(move || scan(&scan_root, None)).await??;
        let (changes, _) = tokio::sync::watch::channel(snapshot.revision);
        Ok(Arc::new(Self {
            root,
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
        let snapshot = tokio::task::spawn_blocking(move || {
            let mut snapshot = scan(&root, if strict { None } else { Some(&old) })?;
            snapshot.revision = old.revision + 1;
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

pub fn scan(root: &File, previous: Option<&Snapshot>) -> Result<Snapshot> {
    prepare_scan(root, previous)?.finish(root)
}

struct PendingScan {
    snapshot: Snapshot,
    dir: File,
    verified: Vec<(String, [u8; 32])>,
}

impl PendingScan {
    fn finish(self, root: &File) -> Result<Snapshot> {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::MetadataExt;
        for (name, hash) in &self.verified {
            let bytes = storage::stable_read(&self.dir, name)?;
            let current: [u8; 32] = Sha256::digest(&bytes).into();
            ensure!(
                current == *hash,
                "{name} changed while reload was queued; retry next scan"
            );
        }
        let current = storage::directory(root, "contents")?;
        ensure!(
            current.metadata()?.dev() == self.dir.metadata()?.dev()
                && current.metadata()?.ino() == self.dir.metadata()?.ino(),
            "contents directory replaced during scan"
        );
        Ok(self.snapshot)
    }
}

fn prepare_scan(root: &File, previous: Option<&Snapshot>) -> Result<PendingScan> {
    let dir = storage::directory(root, "contents")
        .context("cannot read non-symlink contents directory")?;
    let mut candidates = BTreeSet::new();
    for entry in rustix::fs::Dir::read_from(&dir)? {
        let entry = entry.context("cannot enumerate contents; previous snapshot retained")?;
        let bytes = entry.file_name().to_bytes();
        if !bytes.ends_with(b".md") {
            continue;
        }
        let name = std::str::from_utf8(bytes).context("article filename must be UTF-8")?;
        candidates.insert(name.to_owned());
        ensure!(
            candidates.len() <= MAX_ARTICLES,
            "contents exceeds {MAX_ARTICLES} candidate articles"
        );
    }
    let mut articles = BTreeMap::new();
    let mut verified = Vec::new();
    let mut source_bytes = 0;
    for name in candidates {
        let stat = statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        let regular = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile;
        if !regular {
            if let Some(old) = name
                .strip_suffix(".md")
                .and_then(|id| metadata::parse_id(id).ok())
                .and_then(|id| previous?.articles.get(&id))
            {
                tracing::error!(event="invalid_article", file=%name, "non-regular replacement; previous valid article retained");
                source_bytes += old.source_bytes;
                ensure!(
                    source_bytes <= MAX_SOURCE_BYTES,
                    "catalog source exceeds 512 MiB; snapshot retained"
                );
                articles.insert(old.id, old.clone());
            }
            continue;
        }
        let stem = name.strip_suffix(".md").expect("candidate extension");
        let result = metadata::parse_id(stem)
            .and_then(|id| storage::stable_read(&dir, &name).map(|bytes| (id, bytes)))
            .and_then(|(id, bytes)| {
                if let Some(old) = previous.and_then(|p| p.articles.get(&id)) {
                    use sha2::{Digest, Sha256};
                    let hash: [u8; 32] = Sha256::digest(&bytes).into();
                    if old.hash == hash {
                        return Ok(old.clone());
                    }
                }
                metadata::parse(id, &bytes).map(Arc::new)
            });
        let article = match result {
            Ok(article) => {
                verified.push((name.clone(), article.hash));
                Some(article)
            }
            Err(error) if previous.is_some() => {
                tracing::error!(event="invalid_article", file=%name, error=%format!("{error:#}"), "previous valid article retained if available");
                metadata::parse_id(stem)
                    .ok()
                    .and_then(|id| previous?.articles.get(&id).cloned())
            }
            Err(error) => return Err(error).with_context(|| format!("invalid article {name}")),
        };
        if let Some(article) = article {
            source_bytes += article.source_bytes;
            ensure!(
                source_bytes <= MAX_SOURCE_BYTES,
                "catalog source exceeds 512 MiB; snapshot retained"
            );
            articles.insert(article.id, article);
        }
    }
    let mut tags = BTreeMap::new();
    for article in articles.values() {
        for tag in &article.metadata.tags {
            *tags.entry(tag.clone()).or_insert(0) += 1;
        }
        ensure!(
            tags.len() <= MAX_DISTINCT_TAGS,
            "catalog exceeds {MAX_DISTINCT_TAGS} distinct tags"
        );
    }
    Ok(PendingScan {
        snapshot: Snapshot {
            articles,
            tags,
            source_bytes,
            revision: previous.map_or(1, |p| p.revision + 1),
        },
        dir,
        verified,
    })
}

#[cfg(test)]
mod tests;
