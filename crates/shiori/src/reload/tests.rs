use super::*;

#[tokio::test]
async fn startup_reconciliation_does_not_accept_invalid_new_content_as_a_live_edit() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("contents")).unwrap();
    std::fs::write(
        root.path().join("contents/1.md"),
        "---\ntitle: Valid\n---\n",
    )
    .unwrap();
    let catalog = Catalog::open(storage::root(root.path()).unwrap())
        .await
        .unwrap();
    std::fs::write(root.path().join("contents/2.md"), "invalid").unwrap();
    assert!(catalog.reconcile_startup().await.is_err());
    assert_eq!(catalog.snapshot().articles.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn debounce_and_periodic_hash_repair_use_controlled_time() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("contents")).unwrap();
    let path = root.path().join("contents/1.md");
    std::fs::write(&path, "---\ntitle: First\n---\nOld").unwrap();
    let catalog = Catalog::open(storage::root(root.path()).unwrap())
        .await
        .unwrap();
    let mut publications = catalog.changes();
    let (events, changed) = watch::channel(());
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(coordinate(catalog.clone(), changed, stopped));
    tokio::task::yield_now().await;
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, "---\ntitle: Other\n---\nNew").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    // No event is delivered. Thirty-second reconciliation must read content hashes.
    tokio::time::advance(Duration::from_secs(29)).await;
    assert!(!publications.has_changed().unwrap());
    tokio::time::advance(Duration::from_secs(1)).await;
    publications.changed().await.unwrap();
    assert_eq!(catalog.snapshot().articles[&1].metadata.title, "Other");
    let revision = catalog.snapshot().revision;
    std::fs::write(&path, "---\ntitle: Third\n---\nBody").unwrap();
    events.send_replace(());
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(199)).await;
    assert_eq!(catalog.snapshot().revision, revision);
    events.send_replace(());
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(199)).await;
    assert_eq!(catalog.snapshot().revision, revision);
    tokio::time::advance(Duration::from_millis(1)).await;
    publications.changed().await.unwrap();
    assert_eq!(catalog.snapshot().articles[&1].metadata.title, "Third");
    stop.send_replace(true);
    task.await.unwrap();
}
