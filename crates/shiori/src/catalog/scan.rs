use super::{MAX_ARTICLES, MAX_DISTINCT_TAGS, MAX_SOURCE_BYTES, Snapshot};
use crate::{
    layout::Layout,
    metadata, storage,
    timestamps::{Dates, SourceDates},
};
use anyhow::{Context, Result, ensure};
use rustix::fs::{AtFlags, FileType, statat};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn configured(
    root: &File,
    layout: &Layout,
    path: Option<&Path>,
    previous: Option<&Snapshot>,
) -> Result<Snapshot> {
    prepare(root, layout, path, previous)?.finish(root)
}

#[cfg(test)]
pub(super) fn prepare_scan(root: &File, previous: Option<&Snapshot>) -> Result<PendingScan> {
    prepare(root, &Layout::default(), None, previous)
}

pub(super) struct PendingScan {
    snapshot: Snapshot,
    dir: File,
    assets: File,
    verified: Vec<(String, [u8; 32], std::fs::Metadata)>,
    layout: Layout,
    path: Option<PathBuf>,
    dates: SourceDates,
}

impl PendingScan {
    pub(super) fn finish(self, root: &File) -> Result<Snapshot> {
        use std::os::unix::fs::MetadataExt;
        for (name, hash, metadata) in &self.verified {
            let (bytes, current) = storage::stable_source(&self.dir, name)?;
            ensure!(
                *hash == <[u8; 32]>::from(Sha256::digest(&bytes))
                    && storage::fingerprint(metadata) == storage::fingerprint(&current),
                "{name} changed during scan; retry"
            );
        }
        let current = self.layout.open(root, &self.layout.articles, false)?;
        ensure!(
            current.metadata()?.dev() == self.dir.metadata()?.dev()
                && current.metadata()?.ino() == self.dir.metadata()?.ino(),
            "article directory replaced during scan"
        );
        let assets = self.layout.open(root, &self.layout.assets, false)?;
        ensure!(
            assets.metadata()?.dev() == self.assets.metadata()?.dev()
                && assets.metadata()?.ino() == self.assets.metadata()?.ino(),
            "asset directory replaced during scan"
        );
        self.dates.verify(self.path.as_deref())?;
        Ok(self.snapshot)
    }
}

fn candidates(
    dir: &File,
    prefix: &str,
    depth: usize,
    result: &mut BTreeMap<String, (String, bool)>,
) -> Result<()> {
    ensure!(depth <= 64, "article nesting exceeds 64 directories");
    for entry in rustix::fs::Dir::read_from(dir)? {
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let name = std::str::from_utf8(bytes).context("article filename must be UTF-8")?;
        let path = format!("{prefix}{name}");
        let stat = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
        let kind = FileType::from_raw_mode(stat.st_mode);
        if kind == FileType::Directory {
            candidates(
                &storage::directory(dir, name)?,
                &format!("{path}/"),
                depth + 1,
                result,
            )?;
        } else if let Some(key) = path
            .strip_suffix(".md")
            .or_else(|| path.strip_suffix(".markdown"))
        {
            ensure!(metadata::valid_key(key), "invalid article key {key}");
            ensure!(
                result
                    .insert(
                        key.to_owned(),
                        (path.clone(), kind == FileType::RegularFile)
                    )
                    .is_none(),
                "duplicate article key {key}"
            );
            ensure!(
                result.len() <= MAX_ARTICLES,
                "articles exceeds {MAX_ARTICLES} candidate articles"
            );
        }
    }
    Ok(())
}

fn prepare(
    root: &File,
    layout: &Layout,
    path: Option<&Path>,
    previous: Option<&Snapshot>,
) -> Result<PendingScan> {
    let dir = layout
        .open(root, &layout.articles, false)
        .context("cannot read article directory")?;
    let dates = SourceDates::open(path)?;
    let assets = layout
        .open(root, &layout.assets, false)
        .context("cannot read asset directory")?;
    let mut found = BTreeMap::new();
    candidates(&dir, "", 0, &mut found)?;
    if let Some(previous) = previous {
        for key in previous
            .articles
            .keys()
            .filter(|key| !found.contains_key(*key))
        {
            let mut parent = dir.try_clone()?;
            let parts: Vec<_> = key.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                match storage::directory(&parent, part) {
                    Ok(next) => parent = next,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(error) => {
                        return Err(error).context(
                            "previous article ancestor became untrustworthy; snapshot retained",
                        );
                    }
                }
            }
        }
    }
    let mut snapshot = Snapshot {
        history_token: dates.token.clone(),
        revision: previous.map_or(1, |p| p.revision + 1),
        ..Snapshot::default()
    };
    let mut verified = Vec::new();
    for (key, (name, regular)) in found {
        let old = previous.and_then(|p| p.articles.get(&key));
        let loaded = if regular {
            storage::stable_source(&dir, &name)
        } else {
            Err(anyhow::anyhow!("non-regular article replacement"))
        };
        let loaded = match loaded {
            Ok(value) => Some(value),
            Err(error) if previous.is_some() || !regular => {
                tracing::error!(event="invalid_article", file=%name, %error, "previous valid article retained if available");
                None
            }
            Err(error) => return Err(error).with_context(|| format!("invalid article {name}")),
        };
        let article = if let Some((bytes, meta)) = loaded {
            let source_path = dates.source_path(&layout.articles.join(&name))?;
            let cached = old.filter(|old| {
                old.git_revision == dates.token && old.git_tracked && old.source_path == source_path
            });
            let committed = match cached {
                Some(old) => Ok(Some(Dates {
                    created: old.created_at.clone(),
                    updated: old.updated_at.clone(),
                    created_ms: old.created_ms,
                })),
                None => dates.committed(&source_path),
            };
            if let Err(error) = &committed
                && error
                    .downcast_ref::<crate::timestamps::UnsupportedDate>()
                    .is_none()
            {
                return Err(committed.expect_err("history failure"));
            }
            let hash: [u8; 32] = Sha256::digest(&bytes).into();
            let parsed = committed.and_then(|committed| {
                let tracked = committed.is_some();
                let times = committed.map_or_else(|| Dates::filesystem(&meta), Ok)?;
                old.filter(|old| old.hash == hash)
                    .map_or_else(|| metadata::parse(&key, &bytes), |old| Ok((**old).clone()))
                    .map(|mut article| {
                        article.created_at = times.created;
                        article.updated_at = times.updated;
                        article.created_ms = times.created_ms;
                        article.source_path = source_path;
                        article.git_tracked = tracked;
                        article.git_revision = if tracked { dates.token.clone() } else { None };
                        Arc::new(article)
                    })
            });
            match parsed {
                Ok(article) => {
                    verified.push((name.clone(), article.hash, meta));
                    Some(
                        old.filter(|old| {
                            old.hash == article.hash
                                && old.created_at == article.created_at
                                && old.updated_at == article.updated_at
                                && old.source_path == article.source_path
                                && old.git_tracked == article.git_tracked
                                && old.git_revision == article.git_revision
                        })
                        .cloned()
                        .unwrap_or(article),
                    )
                }
                Err(error) if previous.is_some() => {
                    tracing::error!(event="invalid_article", file=%name, %error, "previous valid article retained if available");
                    old.cloned()
                }
                Err(error) => return Err(error).with_context(|| format!("invalid article {name}")),
            }
        } else {
            old.cloned()
        };
        if let Some(article) = article {
            snapshot.source_bytes += article.source_bytes;
            ensure!(
                snapshot.source_bytes <= MAX_SOURCE_BYTES,
                "catalog source exceeds 512 MiB; snapshot retained"
            );
            for tag in &article.metadata.tags {
                *snapshot.tags.entry(tag.clone()).or_insert(0) += 1;
            }
            ensure!(
                snapshot.tags.len() <= MAX_DISTINCT_TAGS,
                "catalog exceeds {MAX_DISTINCT_TAGS} distinct tags"
            );
            snapshot.articles.insert(key, article);
        }
    }
    snapshot.ordered = snapshot.articles.values().cloned().collect();
    snapshot.ordered.sort_by(|a, b| {
        b.created_ms
            .cmp(&a.created_ms)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(PendingScan {
        snapshot,
        dir,
        assets,
        verified,
        layout: layout.clone(),
        path: path.map(Path::to_path_buf),
        dates,
    })
}
