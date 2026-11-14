use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::watch, task::JoinHandle};

use crate::{catalog::Catalog, config::Config, storage};

pub struct Reload {
    watcher: RecommendedWatcher,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Reload {
    pub async fn start(config: &Config) -> Result<(Arc<Catalog>, Self)> {
        let (events, mut changed) = watch::channel(());
        let root = config.root.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                match event {
                    Ok(event) if event.kind.is_access() => return,
                    Ok(event)
                        if !event.paths.iter().any(|path| {
                            path == &root
                                || path.strip_prefix(&root).ok().is_some_and(|path| {
                                    path.starts_with("contents") || path.starts_with("assets")
                                })
                        }) =>
                    {
                        return;
                    }
                    Err(error) => tracing::error!(event="watch_error", %error),
                    _ => {}
                }
                events.send_replace(());
            })
            .context("cannot initialize filesystem notifications")?;
        watcher
            .watch(&config.root, RecursiveMode::Recursive)
            .context("cannot watch root")?;
        let catalog = Catalog::open(storage::root(&config.root)?).await?;
        for attempt in 0..3 {
            changed.borrow_and_update();
            catalog
                .reconcile_startup()
                .await
                .context("startup reconciliation failed")?;
            if !changed.has_changed()? {
                break;
            }
            anyhow::ensure!(
                attempt < 2,
                "storage kept changing across three startup reconciliations; retry when saves finish"
            );
        }
        let (stop, stopped) = watch::channel(false);
        if config.watch {
            let preview = crate::preview::Preview::open(catalog.clone()).await?;
            *catalog.preview.lock().expect("preview lock poisoned") = Some(preview);
        }
        let preview = catalog.preview();
        let owner = catalog.clone();
        let task = tokio::spawn(async move {
            coordinate(owner, changed, stopped).await;
            if let Some(preview) = preview {
                preview.shutdown();
            }
        });
        Ok((
            catalog,
            Self {
                watcher,
                stop,
                task,
            },
        ))
    }
    pub async fn shutdown(self) -> Result<()> {
        self.stop.send_replace(true);
        self.task.await?;
        drop(self.watcher);
        Ok(())
    }
}

async fn coordinate(
    catalog: Arc<Catalog>,
    mut changed: watch::Receiver<()>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut periodic = tokio::time::interval(Duration::from_secs(30));
    periodic.tick().await;
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = periodic.tick() => {},
            event = changed.changed() => {
                if event.is_err() { break; }
                let quiet = tokio::time::sleep(Duration::from_millis(200));
                tokio::pin!(quiet);
                loop {
                    tokio::select! {
                        _ = stopped.changed() => return,
                        _ = &mut quiet => break,
                        _ = periodic.tick() => break,
                        event = changed.changed() => {
                            if event.is_err() { return; }
                            quiet.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(200));
                        }
                    }
                }
            }
        }
        match catalog.reconcile().await {
            Ok(revision) => tracing::info!(
                event = "reload",
                revision,
                articles = catalog.snapshot().articles.len()
            ),
            Err(error) => {
                tracing::error!(event="reload_failed", error=%format!("{error:#}"), "previous snapshot retained")
            }
        }
        if let Some(preview) = catalog.preview() {
            preview.refresh(catalog.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests;
