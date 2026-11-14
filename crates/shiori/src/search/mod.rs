use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::Semaphore;

use crate::{catalog::Snapshot, graphql::Error, metadata::Article};

pub struct SearchQuery {
    terms: Vec<String>,
}

impl SearchQuery {
    pub fn new(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        if value.is_empty() || value.chars().count() > 256 {
            return Err(Error::Input("query must contain 1 to 256 characters"));
        }
        let terms: Vec<_> = value.split_whitespace().map(str::to_lowercase).collect();
        if terms.len() > 16 {
            return Err(Error::Input("query must contain at most 16 terms"));
        }
        Ok(Self { terms })
    }

    fn matches(&self, article: &Article) -> bool {
        let title = article.metadata.title.to_lowercase();
        let tags = article.metadata.tags.join("\n").to_lowercase();
        let remaining: Vec<_> = self
            .terms
            .iter()
            .filter(|term| !title.contains(term.as_str()) && !tags.contains(term.as_str()))
            .collect();
        if remaining.is_empty() {
            return true;
        }
        let body = article.body.to_lowercase();
        remaining.iter().all(|term| body.contains(term.as_str()))
    }
}

pub struct SearchExecutor {
    slots: Arc<Semaphore>,
}

struct Cancellation(Arc<AtomicBool>);

impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl SearchExecutor {
    pub fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
        }
    }

    async fn worker<T: Send + 'static>(
        &self,
        work: impl FnOnce(Arc<AtomicBool>) -> T + Send + 'static,
    ) -> Result<T, Error> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Search)?;
        let canceled = Arc::new(AtomicBool::new(false));
        // The request owns cancellation; the blocking worker owns admission.
        // Dropping a request cannot admit a third worker before this one exits.
        let _cancellation = Cancellation(canceled.clone());
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work(canceled)
        })
        .await
        .map_err(|_| Error::Search)
    }

    pub async fn select(
        &self,
        snapshot: Arc<Snapshot>,
        query: SearchQuery,
        tag: Option<String>,
    ) -> Result<Vec<Arc<Article>>, Error> {
        self.worker(move |canceled| {
            let mut found = Vec::new();
            for article in snapshot.articles.values().rev() {
                if canceled.load(Ordering::Relaxed) {
                    break;
                }
                if tag
                    .as_ref()
                    .is_none_or(|tag| article.metadata.tags.contains(tag))
                    && query.matches(article)
                {
                    found.push(article.clone());
                }
            }
            found
        })
        .await
    }
}

#[cfg(test)]
mod tests;
