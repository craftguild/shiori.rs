mod assets;
#[cfg(test)]
mod tests;

use crate::catalog::Catalog;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub struct Preview {
    assets: Mutex<[u8; 32]>,
    changes: watch::Sender<Option<String>>,
}

#[derive(Clone)]
pub struct ContentChange(pub String);

fn revision(catalog: &Catalog, assets: [u8; 32]) -> String {
    let mut digest = Sha256::new();
    for article in catalog.snapshot().articles.values() {
        for field in [
            &article.id,
            &article.created_at,
            &article.updated_at,
            &article.source_path,
        ] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field.as_bytes());
        }
        digest.update(article.hash);
    }
    digest.update(assets);
    format!("{:x}", digest.finalize())
}

impl Preview {
    pub async fn open(catalog: Arc<Catalog>) -> Result<Arc<Self>> {
        tokio::task::spawn_blocking(move || {
            let assets = assets::fingerprint(&catalog.assets).unwrap_or_else(|error| {
                tracing::warn!(event="preview_assets_retry", %error);
                Sha256::digest([]).into()
            });
            let (changes, _) = watch::channel(Some(revision(&catalog, assets)));
            Ok(Arc::new(Self {
                assets: Mutex::new(assets),
                changes,
            }))
        })
        .await?
    }

    pub async fn refresh(self: &Arc<Self>, catalog: Arc<Catalog>) {
        let owner = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut assets = owner.assets.lock().expect("preview asset lock poisoned");
            match assets::fingerprint(&catalog.assets) {
                Ok(next) => *assets = next,
                Err(error) => tracing::warn!(event="preview_assets_retry", %error),
            }
            let next = revision(&catalog, *assets);
            owner.changes.send_if_modified(|current| {
                if current.as_ref().is_some_and(|value| value != &next) {
                    *current = Some(next.clone());
                    tracing::debug!(event="preview_changed", revision=%next);
                    true
                } else {
                    false
                }
            });
        })
        .await;
        if let Err(error) = result {
            tracing::error!(event="preview_failed", %error);
        }
    }

    pub fn source(
        &self,
    ) -> impl futures_util::Stream<Item = Result<ContentChange, crate::graphql::Error>> + Send + 'static
    {
        futures_util::stream::unfold(
            (self.changes.subscribe(), true),
            |(mut receiver, initial)| async move {
                if !initial && receiver.changed().await.is_err() {
                    return None;
                }
                let revision = receiver.borrow_and_update().clone()?;
                Some((Ok(ContentChange(revision)), (receiver, false)))
            },
        )
    }

    pub fn shutdown(&self) {
        self.changes.send_replace(None);
    }
}
