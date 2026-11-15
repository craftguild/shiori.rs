use super::*;
use futures_util::{StreamExt, pin_mut};
use std::time::Duration;

#[tokio::test]
async fn revisions_track_accepted_content_and_asset_bytes_not_noops_or_symlinks() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    let article = root.path().join("articles/1.md");
    std::fs::write(&article, "---\ntitle: First\n---\nBody").unwrap();
    let catalog = Catalog::open(crate::storage::root(root.path()).unwrap())
        .await
        .unwrap();
    let preview = Preview::open(catalog.clone()).await.unwrap();
    let source = preview.source();
    pin_mut!(source);
    let first = source.next().await.unwrap().unwrap().0;
    catalog.reconcile().await.unwrap();
    preview.refresh(catalog.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), source.next())
            .await
            .is_err()
    );
    std::fs::write(&article, "not valid frontmatter").unwrap();
    catalog.reconcile().await.unwrap();
    preview.refresh(catalog.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), source.next())
            .await
            .is_err()
    );
    std::fs::write(&article, "---\ntitle: Second\n---\nBody").unwrap();
    catalog.reconcile().await.unwrap();
    preview.refresh(catalog.clone()).await;
    let second = source.next().await.unwrap().unwrap().0;
    assert_ne!(first, second);
    let image = root.path().join("assets/image.svg");
    std::fs::write(&image, "<svg>first</svg>").unwrap();
    preview.refresh(catalog.clone()).await;
    let added = source.next().await.unwrap().unwrap().0;
    let modified = std::fs::metadata(&image).unwrap().modified().unwrap();
    std::fs::write(&image, "<svg>other</svg>").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&image)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    preview.refresh(catalog.clone()).await;
    let replaced = source.next().await.unwrap().unwrap().0;
    assert_ne!(added, replaced);
    std::os::unix::fs::symlink("/etc/passwd", root.path().join("assets/outside")).unwrap();
    preview.refresh(catalog.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), source.next())
            .await
            .is_err()
    );
    std::fs::remove_file(image).unwrap();
    preview.refresh(catalog.clone()).await;
    assert_ne!(replaced, source.next().await.unwrap().unwrap().0);
    preview.shutdown();
    assert!(source.next().await.is_none());
}

#[tokio::test]
async fn replacing_assets_retains_the_revision_once_and_rereads_on_the_next_refresh() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    std::fs::write(root.path().join("assets/file.txt"), "OLD").unwrap();
    let catalog = Catalog::open(crate::storage::root(root.path()).unwrap())
        .await
        .unwrap();
    let preview = Preview::open(catalog.clone()).await.unwrap();
    let source = preview.source();
    pin_mut!(source);
    let old = source.next().await.unwrap().unwrap().0;
    std::fs::rename(root.path().join("assets"), root.path().join("retired")).unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    std::fs::write(root.path().join("assets/file.txt"), "NEW").unwrap();
    preview.refresh(catalog.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), source.next())
            .await
            .is_err()
    );
    preview.refresh(catalog).await;
    assert_ne!(old, source.next().await.unwrap().unwrap().0);
    preview.shutdown();
}
