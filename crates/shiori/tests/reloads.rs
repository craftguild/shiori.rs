mod common;
use common::*;
use shiori::{catalog::Catalog, config::Config, reload::Reload, storage};
use std::sync::Arc;

#[tokio::test]
async fn reconciliation_retains_invalid_updates_repairs_and_confirms_deletions() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 10, "First", &["old"], "Old body");
    article(root.path(), 20, "Second", &["shared"], "Second");
    let (catalog, renderer, router) = app(root.path()).await;
    let old = catalog.snapshot();
    let original = data(
        &router,
        "{ post(id:\"10\") { title document { nodes { ... on TextNode { text } } } } }",
    )
    .await;
    std::fs::write(root.path().join("articles/10.md"), "invalid").unwrap();
    std::fs::write(root.path().join("articles/30.md"), "invalid new").unwrap();
    article(root.path(), 20, "Changed", &["new"], "New body");
    catalog.reconcile().await.unwrap();
    let snapshot = catalog.snapshot();
    assert_eq!(snapshot.articles.len(), 2);
    assert_eq!(snapshot.articles["10"].metadata.title, "First");
    assert_eq!(snapshot.articles["20"].metadata.title, "Changed");
    assert_eq!(old.articles["20"].metadata.title, "Second");
    assert_eq!(
        snapshot.tags.keys().map(String::as_str).collect::<Vec<_>>(),
        ["new", "old"]
    );
    assert_eq!(
        data(
            &router,
            "{ post(id:\"10\") { title document { nodes { ... on TextNode { text } } } } }"
        )
        .await,
        original
    );
    assert_eq!(renderer.calls(), 1);
    article(root.path(), 10, "Fixed", &["fixed"], "New revision");
    article(root.path(), 30, "Added", &[], "");
    std::fs::rename(
        root.path().join("articles/10.md"),
        root.path().join("articles/.save"),
    )
    .unwrap();
    std::fs::rename(
        root.path().join("articles/.save"),
        root.path().join("articles/10.md"),
    )
    .unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles.len(), 3);
    let fixed = data(
        &router,
        "{ post(id:\"10\") { id title document { nodes { ... on TextNode { text } } } } }",
    )
    .await;
    assert_eq!(fixed["post"]["title"], "Fixed");
    assert!(fixed.to_string().contains("New revision"));
    assert_eq!(renderer.calls(), 2);
    // Metadata-valid new source can fail lazy derivation; it must not reuse old document.
    article(
        root.path(),
        10,
        "New failed revision",
        &["fixed"],
        &format!("{}x{}", "![".repeat(300), "](x)".repeat(300)),
    );
    catalog.reconcile().await.unwrap();
    let failed = query(
        &router,
        "{ post(id:\"10\") { title document { nodes { ... on TextNode { text } } } } }",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(failed["data"]["post"]["title"], "New failed revision");
    assert!(failed["data"]["post"]["document"].is_null());
    assert_eq!(
        failed["errors"][0]["extensions"]["code"],
        "STRUCTURE_FAILED"
    );
    std::fs::remove_file(root.path().join("articles/10.md")).unwrap();
    catalog.reconcile().await.unwrap();
    assert!(!catalog.snapshot().articles.contains_key("10"));
    assert_eq!(
        data(&router, "{ post(id:\"10\") { id } }").await["post"],
        serde_json::Value::Null
    );
    let retained = catalog.snapshot();
    std::fs::rename(
        root.path().join("articles"),
        root.path().join("unavailable"),
    )
    .unwrap();
    assert!(catalog.reconcile().await.is_err());
    assert!(Arc::ptr_eq(&catalog.snapshot(), &retained));
    std::fs::rename(
        root.path().join("unavailable"),
        root.path().join("articles"),
    )
    .unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles.len(), 2);
    // A symlink replacement never follows the link or loses last-valid source.
    std::fs::remove_file(root.path().join("articles/20.md")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.path().join("articles/20.md")).unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles["20"].metadata.title, "Changed");
    // A restart certifies the source, so invalid initial files fail rather than disappearing.
    std::fs::write(root.path().join("articles/bad.md"), "invalid front matter").unwrap();
    assert!(
        Catalog::open(storage::root(root.path()).unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn hashes_find_equal_size_equal_mtime_edits_without_notifications() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Alpha", &[], "Old");
    let (catalog, _, _) = app(root.path()).await;
    let path = root.path().join("articles/1.md");
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let old_hash = catalog.snapshot().articles["1"].hash;
    article(root.path(), 1, "Bravo", &[], "New");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles["1"].metadata.title, "Bravo");
    assert_ne!(catalog.snapshot().articles["1"].hash, old_hash);
}

#[tokio::test]
async fn real_notification_rename_save_and_clean_shutdown() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Before", &[], "Text");
    let config = Config::new(
        root.path(),
        "127.0.0.1:0".parse().unwrap(),
        "http://localhost:4000",
    )
    .unwrap();
    let (catalog, reload) = Reload::start(&config).await.unwrap();
    let mut changed = catalog.changes();
    let temp = root.path().join("articles/.editor-save");
    std::fs::write(&temp, "---\ntitle: After\ntags: [new]\n---\nNew body").unwrap();
    std::fs::rename(temp, root.path().join("articles/1.md")).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            changed.changed().await.unwrap();
            if catalog.snapshot().articles["1"].metadata.title == "After" {
                break;
            }
        }
    })
    .await
    .expect("watcher must publish the rename save");
    assert_eq!(catalog.snapshot().tags["new"], 1);
    reload.shutdown().await.unwrap();
}
