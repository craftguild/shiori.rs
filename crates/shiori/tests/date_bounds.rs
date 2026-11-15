mod common;
use common::history;

#[tokio::test]
async fn unsupported_git_dates_reject_startup_and_retain_only_the_invalid_article_on_reload() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "S");
    let (catalog, _, _) = history::app(root.path()).await;
    let old = catalog.snapshot().articles["clock"].clone();
    history::select(root.path(), "U");
    std::fs::write(
        root.path().join("site/articles/other.md"),
        "---\ntitle: Valid update\n---\nBody",
    )
    .unwrap();
    catalog.reconcile().await.unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &old,
        &catalog.snapshot().articles["clock"]
    ));
    assert!(catalog.snapshot().articles.contains_key("other"));
    let config = shiori::config::Config::new(
        &root.path().join("site"),
        "127.0.0.1:0".parse().unwrap(),
        "http://reader.example/",
    )
    .unwrap();
    assert!(
        shiori::catalog::Catalog::open_config(&config)
            .await
            .is_err()
    );
}
