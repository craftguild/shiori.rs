use super::*;
use crate::{
    catalog::{Catalog, Snapshot},
    metadata, storage,
};
use oriha_core::BlockOutputMode;
use oriha_core::{
    AssetKind, BlockNullReason, HtmlBlockKind, RenderAsset, RenderedBlockContent,
    RenderedBlockOutput, RenderedHtmlBlock,
};
use std::sync::{Condvar, atomic::AtomicBool};

pub(super) struct Gate {
    entered: Semaphore,
    released: Mutex<bool>,
    wake: Condvar,
    panic: AtomicBool,
}
impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            wake: Condvar::new(),
            panic: AtomicBool::new(false),
        })
    }
    pub(super) fn wait(&self) {
        self.entered.add_permits(1);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.wake.wait(released).unwrap();
        }
        drop(released);
        assert!(
            !self.panic.load(Ordering::Relaxed),
            "deliberate worker panic"
        );
    }
    async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn fixture() -> (tempfile::TempDir, Arc<Catalog>, Arc<Renderer>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    write(&root, 1, "Old");
    write(&root, 2, "Second");
    let catalog = Catalog::open(storage::root(root.path()).unwrap())
        .await
        .unwrap();
    let config = Arc::new(
        Config::new(
            root.path(),
            "127.0.0.1:0".parse().unwrap(),
            "http://cms.example",
        )
        .unwrap(),
    );
    let renderer = Renderer::new(config, &catalog);
    (root, catalog, renderer)
}
fn write(root: &tempfile::TempDir, id: i64, body: &str) {
    std::fs::write(
        root.path().join(format!("articles/{id}.md")),
        format!("---\ntitle: {body}\n---\n{body}"),
    )
    .unwrap();
}
fn request(
    renderer: Arc<Renderer>,
    article: Arc<Article>,
) -> tokio::task::JoinHandle<RenderResult> {
    tokio::spawn(async move { renderer.render(article, &RequestMemo::default()).await })
}
async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("controlled task must reach synchronization point");
}

#[test]
fn adapter_preserves_all_typed_kinds_empty_html_order_and_resource_encoding() {
    let kinds = [
        HtmlBlockKind::Document,
        HtmlBlockKind::Paragraph,
        HtmlBlockKind::Heading,
        HtmlBlockKind::FencedCode,
        HtmlBlockKind::IndentedCode,
        HtmlBlockKind::MathBlock,
        HtmlBlockKind::ThematicBreak,
        HtmlBlockKind::HtmlBlock,
        HtmlBlockKind::LinkReferenceDefinition,
        HtmlBlockKind::BlockQuote,
        HtmlBlockKind::List,
        HtmlBlockKind::ListItem,
        HtmlBlockKind::GfmTable,
        HtmlBlockKind::BlankLine,
        HtmlBlockKind::Callout,
        HtmlBlockKind::ForeignHtml,
        HtmlBlockKind::Footnotes,
    ];
    let mut blocks: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| RenderedHtmlBlock {
            id: i.to_string(),
            kind: *kind,
            content: RenderedBlockContent::Html(String::new()),
        })
        .collect();
    blocks.insert(
        1,
        RenderedHtmlBlock {
            id: "null".into(),
            kind: HtmlBlockKind::BlankLine,
            content: RenderedBlockContent::Null(BlockNullReason::BlankLine),
        },
    );
    let assets = [
        (AssetKind::Svg, "image/svg+xml", "<svg>λ</svg>"),
        (AssetKind::Css, "text/css", "a{}"),
        (AssetKind::Other, "font/otf", "AAEC"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (kind, mime, content))| RenderAsset {
        id: i.to_string(),
        kind,
        mime_type: mime.into(),
        content: content.into(),
    })
    .collect();
    let mut output = RenderedBlockOutput {
        schema_version: 1,
        mode: BlockOutputMode::Rendered,
        status: BlockOutputStatus::Complete,
        blocks,
        assets,
        diagnostics: vec![],
    };
    let result = project(output.clone()).unwrap();
    assert_eq!(result.blocks.len(), kinds.len());
    for (i, block) in result.blocks.iter().enumerate() {
        assert_eq!(block.id, i.to_string());
        assert_eq!(
            block.kind,
            serde_json::to_value(kinds[i]).unwrap().as_str().unwrap()
        );
        assert_eq!(block.html, "");
    }
    assert_eq!(result.assets[0].content, "<svg>λ</svg>");
    assert_eq!(result.assets[1].content, "a{}");
    assert_eq!(result.assets[2].content, "AAEC");
    assert_eq!(result.assets[2].mime_type, "font/otf");
    let json = serde_json::to_string(result.as_ref()).unwrap();
    for forbidden in ["children", "nullReason", "markdown", "properties"] {
        assert!(!json.contains(forbidden));
    }
    for status in [BlockOutputStatus::Incomplete, BlockOutputStatus::Failed] {
        output.status = status;
        assert!(project(output.clone()).is_err());
    }
}

#[test]
fn cache_accounts_capacity_evicts_lru_and_invalidates_changed_deleted_entries() {
    let key = |id: i64| Key {
        id: id.to_string(),
        hash: [id as u8; 32],
        profile: [0; 32],
    };
    let value = Arc::new(Rendered::default());
    let charge = value.retained_bytes() + std::mem::size_of::<Key>() + 128 + key(1).id.capacity();
    let mut cache = cache::RenderCache::new(charge * 2);
    cache.insert(key(1), Projection::Html(value.clone()));
    cache.insert(key(2), Projection::Html(value.clone()));
    assert_eq!(cache.bytes(), charge * 2);
    assert!(cache.get(&key(1)).is_some());
    cache.insert(key(3), Projection::Html(value.clone()));
    assert!(cache.get(&key(2)).is_none());
    assert!(cache.get(&key(1)).is_some());
    let mut reserved = String::with_capacity(4096);
    reserved.push('x');
    let oversized = Arc::new(Rendered {
        blocks: vec![HtmlBlock {
            id: "a".into(),
            kind: "paragraph".into(),
            html: reserved,
        }],
        assets: vec![],
        ..Default::default()
    });
    cache.insert(key(4), Projection::Html(oversized));
    assert!(cache.get(&key(4)).is_none());
    assert_eq!(cache.bytes(), charge * 2);
    let changed = Arc::new(metadata::parse(1, b"---\ntitle: New\n---\n").unwrap());
    let snapshot = Snapshot {
        articles: [("1".to_owned(), changed)].into_iter().collect(),
        ..Default::default()
    };
    cache.retain_current(&snapshot);
    assert!(cache.is_empty());
    assert_eq!(cache.bytes(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_flight_cancellation_stale_completion_and_snapshot_consistency() {
    let (root, catalog, renderer) = fixture().await;
    let gate = Gate::new();
    let _release = Release(gate.clone());
    *renderer.probe.lock().unwrap() = Some(gate.clone());
    let old = catalog.snapshot().articles["1"].clone();
    let first = request(renderer.clone(), old.clone());
    gate.entered().await;
    let second = request(renderer.clone(), old.clone());
    until(|| {
        renderer
            .flights
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .receiver_count()
            >= 2
    })
    .await;
    first.abort();
    write(&root, 1, "New");
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles["1"].metadata.title, "New");
    gate.release();
    let old_result = second.await.unwrap().unwrap();
    assert!(old_result.blocks[0].html.contains("Old"));
    assert!(catalog.state.lock().unwrap().cache.is_empty());
    assert_eq!(renderer.calls(), 1);
    assert_eq!(renderer.active_keys(), 0);
    *renderer.probe.lock().unwrap() = None;
    let new = request(renderer.clone(), catalog.snapshot().articles["1"].clone())
        .await
        .unwrap()
        .unwrap();
    assert!(new.blocks[0].html.contains("New"));
    assert_eq!(renderer.calls(), 2);
    std::fs::remove_file(root.path().join("articles/1.md")).unwrap();
    catalog.reconcile().await.unwrap();
    assert!(catalog.state.lock().unwrap().cache.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_cancellation_and_worker_panic_release_capacity_and_key() {
    let (_, catalog, mut renderer) = fixture().await;
    // One actual worker makes queue and ownership checks deterministic.
    Arc::get_mut(&mut renderer).unwrap().workers = Arc::new(Semaphore::new(1));
    let gate = Gate::new();
    let _release = Release(gate.clone());
    gate.panic.store(true, Ordering::Relaxed);
    *renderer.probe.lock().unwrap() = Some(gate.clone());
    let first = request(renderer.clone(), catalog.snapshot().articles["1"].clone());
    gate.entered().await;
    let queued = request(renderer.clone(), catalog.snapshot().articles["2"].clone());
    tokio::task::yield_now().await;
    assert_eq!(renderer.calls(), 1);
    assert_eq!(renderer.available_workers(), 0);
    queued.abort();
    gate.release();
    assert!(first.await.unwrap().is_err());
    assert_eq!(renderer.active_keys(), 0);
    assert_eq!(renderer.available_workers(), 1);
    *renderer.probe.lock().unwrap() = None;
    let result = request(renderer.clone(), catalog.snapshot().articles["2"].clone())
        .await
        .unwrap()
        .unwrap();
    assert!(result.blocks[0].html.contains("Second"));
    assert_eq!(renderer.calls(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http_request_keeps_one_snapshot_during_overlapping_reload() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let (root, catalog, renderer) = fixture().await;
    let gate = Gate::new();
    let _release = Release(gate.clone());
    *renderer.probe.lock().unwrap() = Some(gate.clone());
    let router = crate::server::router(catalog.clone(), renderer.clone()).unwrap();
    let query = serde_json::json!({"query":"{ a: post(id:\"1\") { title blocks { html } } b: posts { nodes { title } } c: post(id:\"1\") { title } }"});
    let mut pending = tokio::spawn(
        router.clone().oneshot(
            Request::post("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(query.to_string()))
                .unwrap(),
        ),
    );
    tokio::select! {
        _ = gate.entered() => {},
        response = &mut pending => {
            let response = response.unwrap().unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            panic!("HTTP completed before rendering: {status} {headers:?} {}", String::from_utf8_lossy(&bytes));
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("HTTP did not reach renderer"),
    }
    write(&root, 1, "New");
    catalog.reconcile().await.unwrap();
    gate.release();
    let response = pending.await.unwrap().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(value.get("errors").is_none(), "{value}");
    assert_eq!(value["data"]["a"]["title"], "Old");
    assert_eq!(value["data"]["c"]["title"], "Old");
    assert!(
        value["data"]["b"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["title"] == "Old")
    );
    assert!(
        value["data"]["a"]["blocks"][0]["html"]
            .as_str()
            .unwrap()
            .contains("Old")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_every_waiter_still_cleans_up_and_failure_memo_is_request_local() {
    let (_root, catalog, renderer) = fixture().await;
    let gate = Gate::new();
    let _release = Release(gate.clone());
    *renderer.probe.lock().unwrap() = Some(gate.clone());
    let first = request(renderer.clone(), catalog.snapshot().articles["1"].clone());
    gate.entered().await;
    first.abort();
    gate.release();
    until(|| renderer.active_keys() == 0).await;
    assert!(renderer.available_workers() > 0);
    assert_eq!(catalog.state.lock().unwrap().cache.len(), 1);
    *renderer.probe.lock().unwrap() = None;
    let malicious = format!(
        "---\ntitle: Failed\n---\n{}x{}",
        "![".repeat(300),
        "](x)".repeat(300)
    );
    let article = Arc::new(metadata::parse(1, malicious.as_bytes()).unwrap());
    let memo = RequestMemo::default();
    assert!(renderer.render(article.clone(), &memo).await.is_err());
    let count = renderer.calls();
    assert!(renderer.render(article.clone(), &memo).await.is_err());
    assert_eq!(renderer.calls(), count);
    assert!(
        renderer
            .render(article, &RequestMemo::default())
            .await
            .is_err()
    );
    assert_eq!(renderer.calls(), count + 1);
    assert_eq!(renderer.active_keys(), 0);
}

#[tokio::test]
async fn actual_producer_block_and_output_limits_are_enforced() {
    let (_root, catalog, renderer) = fixture().await;
    for body in ["x\n\n".repeat(20_001), "&".repeat(900_000)] {
        let source = format!("---\ntitle: Bounded\n---\n{body}");
        let article = Arc::new(metadata::parse(1, source.as_bytes()).unwrap());
        assert!(
            renderer
                .render(article, &RequestMemo::default())
                .await
                .is_err()
        );
    }
    assert!(catalog.state.lock().unwrap().cache.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn structured_cancellation_keeps_shared_worker_supervision_and_cache_publication() {
    let (_root, catalog, renderer) = fixture().await;
    let gate = Gate::new();
    let _release = Release(gate.clone());
    *renderer.probe.lock().unwrap() = Some(gate.clone());
    let worker = renderer.clone();
    let article = catalog.snapshot().articles["1"].clone();
    let waiter =
        tokio::spawn(async move { worker.structure(article, &RequestMemo::default()).await });
    gate.entered().await;
    waiter.abort();
    gate.release();
    until(|| renderer.active_keys() == 0).await;
    assert!(renderer.available_workers() > 0);
    assert_eq!(renderer.images.inspections(), 0);
    assert_eq!(renderer.calls(), 1);
    *renderer.probe.lock().unwrap() = None;
    let cached = renderer
        .structure(
            catalog.snapshot().articles["1"].clone(),
            &RequestMemo::default(),
        )
        .await
        .unwrap();
    assert!(!cached.blocks.is_empty());
    assert_eq!(renderer.calls(), 1);
}

#[tokio::test]
async fn structured_producer_output_limit_is_enforced_before_flattening() {
    let (_root, catalog, renderer) = fixture().await;
    let source = format!("---\ntitle: Bounded\n---\n{}", "\"".repeat(900_000));
    let article = Arc::new(metadata::parse(1, source.as_bytes()).unwrap());
    assert!(
        renderer
            .structure(article, &RequestMemo::default())
            .await
            .is_err()
    );
    assert!(catalog.state.lock().unwrap().cache.is_empty());
    assert_eq!(renderer.images.inspections(), 0);
}

#[test]
fn html_and_structured_share_one_retained_capacity_budget() {
    let html = Projection::Html(Arc::new(Rendered::default()));
    let structured = Projection::Structured(
        crate::structured::project(
            oriha_core::StructuredBlockOutput {
                schema_version: 1,
                mode: oriha_core::BlockOutputMode::Structured,
                status: BlockOutputStatus::Complete,
                blocks: vec![],
                references: Default::default(),
                footnotes: vec![],
                diagnostics: vec![],
            },
            vec![],
        )
        .unwrap(),
    );
    let charge = |value: &Projection| {
        value.retained_bytes() + std::mem::size_of::<Key>() + 128 + "1".to_owned().capacity()
    };
    let budget = charge(&html) + charge(&structured);
    let mut cache = cache::RenderCache::new(budget);
    let key = |mode| Key {
        id: "1".to_owned(),
        hash: [1; 32],
        profile: [mode; 32],
    };
    cache.insert(key(0), html.clone());
    cache.insert(key(1), structured);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.bytes(), budget);
    cache.insert(key(2), html);
    assert!(cache.get(&key(0)).is_none());
    assert!(cache.bytes() <= budget);
}
