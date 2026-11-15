mod common;
use common::*;
use futures_util::StreamExt;

#[tokio::test]
async fn committer_clock_skew_keeps_ancestry_selection_without_timestamp_clamping() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "S");
    let catalog = Catalog::open_config(&config(&root.path().join("site")))
        .await
        .unwrap();
    let snapshot = catalog.snapshot();
    let post = &snapshot.articles["clock"];
    assert_eq!(post.created_at, "2021-01-01T00:00:00.000Z");
    assert_eq!(post.updated_at, "2020-01-01T00:00:00.000Z");
}

#[tokio::test]
async fn divergent_merge_considers_additions_on_both_parents() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "N");
    let catalog = Catalog::open_config(&config(&root.path().join("site")))
        .await
        .unwrap();
    assert_eq!(
        catalog.snapshot().articles["merge"].created_at,
        "2020-02-01T00:00:00.000Z"
    );
    assert_eq!(
        catalog.snapshot().articles["merge"].updated_at,
        "2020-03-01T00:00:00.000Z"
    );
}
use serde_json::json;
use shiori::{
    catalog::Catalog, config::Config, layout::Layout, preview::Preview, rendering::Renderer,
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

fn config(root: &std::path::Path) -> Config {
    Config::with_layout(
        root,
        "127.0.0.1:0".parse().unwrap(),
        "http://reader.example/",
        Layout::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn git_dates_are_committer_dates_for_current_paths_and_metadata_only_head_changes_refresh_preview()
 {
    let temp = tempfile::tempdir().unwrap();
    history::install(temp.path(), "C");
    let config = Arc::new(config(&temp.path().join("site")));
    let catalog = Catalog::open_config(&config).await.unwrap();
    let renderer = Renderer::new(config, &catalog);
    let router = shiori::server::router(catalog.clone(), renderer).unwrap();
    let q = "{ posts { nodes { id createdAt updatedAt sourcePath } } monthlyArchives { year month count } }";
    let first = data(&router, q).await;
    assert_eq!(
        first["posts"]["nodes"],
        json!([
            {"id":"foo/移動", "createdAt":"2020-02-29T23:59:59.000Z", "updatedAt":"2020-02-29T23:59:59.000Z", "sourcePath":"site/articles/foo/移動.markdown"},
            {"id":"記事1", "createdAt":"2020-01-31T23:59:59.000Z", "updatedAt":"2020-02-01T00:00:00.000Z", "sourcePath":"site/articles/記事1.md"}
        ])
    );
    assert_eq!(
        first["monthlyArchives"],
        json!([{"year":2020,"month":2,"count":1},{"year":2020,"month":1,"count":1}])
    );
    let preview = Preview::open(catalog.clone()).await.unwrap();
    let mut stream = Box::pin(preview.source());
    let revision = stream.next().await.unwrap().unwrap().0;
    let bytes = catalog.snapshot().articles["記事1"].hash;
    history::select(temp.path(), "D");
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles["記事1"].hash, bytes);
    assert_eq!(
        catalog.snapshot().articles["記事1"].updated_at,
        "2020-03-01T00:00:00.000Z"
    );
    preview.refresh(catalog.clone()).await;
    let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .0;
    assert_ne!(revision, next);
    history::select(temp.path(), "F");
    catalog.reconcile().await.unwrap();
    assert_eq!(
        catalog.snapshot().articles["記事1"].created_at,
        "2021-01-01T00:00:00.000Z"
    );
    assert_eq!(catalog.snapshot().ordered[0].id, "記事1");
}

#[tokio::test]
async fn merge_unchanged_parent_is_not_a_path_change_and_broken_history_preserves_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    history::install(temp.path(), "M");
    let config = config(&temp.path().join("site"));
    let catalog = Catalog::open_config(&config).await.unwrap();
    assert_eq!(
        catalog.snapshot().articles["foo/移動"].created_at,
        "2020-02-29T23:59:59.000Z"
    );
    assert_eq!(
        catalog.snapshot().articles["記事1"].updated_at,
        "2020-02-01T00:00:00.000Z"
    );
    let old = catalog.snapshot();
    std::fs::write(
        temp.path().join(".git/shallow"),
        std::fs::read(temp.path().join(".git/refs/heads/main")).unwrap(),
    )
    .unwrap();
    assert!(catalog.reconcile().await.is_err());
    assert!(Arc::ptr_eq(&old, &catalog.snapshot()));
    assert!(Catalog::open_config(&config).await.is_err());
}

#[tokio::test]
async fn filesystem_dates_ignore_numeric_names_and_untracked_files_use_actual_metadata() {
    let temp = tempfile::tempdir().unwrap();
    history::install(temp.path(), "C");
    let root = temp.path().join("site");
    let path = root.join("articles/foo/自然言語.md");
    std::fs::write(&path, "---\ntitle: Filesystem\n---\nBody").unwrap();
    let updated = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(updated))
        .unwrap();
    let catalog = Catalog::open_config(&config(&root)).await.unwrap();
    let snapshot = catalog.snapshot();
    let post = &snapshot.articles["foo/自然言語"];
    assert!(!post.git_tracked);
    assert_eq!(post.updated_at, "2020-09-13T12:26:40.000Z");
    assert_eq!(post.source_path, "site/articles/foo/自然言語.md");
    let non_git = tempfile::tempdir().unwrap();
    article(non_git.path(), 1, "Not epoch", &[], "Body");
    let catalog = Catalog::open_config(&config(non_git.path())).await.unwrap();
    assert_ne!(
        catalog.snapshot().articles["1"].created_at,
        "1970-01-01T00:00:00.001Z"
    );
    assert_eq!(
        catalog.snapshot().articles["1"].source_path,
        "articles/1.md"
    );
}

#[tokio::test]
async fn repair_after_invalid_save_resolves_dates_from_the_new_head() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "C");
    let config = config(&root.path().join("site"));
    let catalog = Catalog::open_config(&config).await.unwrap();
    let file = root.path().join("site/articles/記事1.md");
    let original = std::fs::read(&file).unwrap();
    std::fs::write(&file, "invalid front matter").unwrap();
    history::select(root.path(), "D");
    catalog.reconcile().await.unwrap();
    assert_eq!(
        catalog.snapshot().articles["記事1"].updated_at,
        "2020-02-01T00:00:00.000Z"
    );
    std::fs::write(file, original).unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(
        catalog.snapshot().articles["記事1"].updated_at,
        "2020-03-01T00:00:00.000Z"
    );
}

#[tokio::test]
async fn linked_worktrees_and_unborn_repositories_keep_relative_source_paths() {
    let root = tempfile::tempdir().unwrap();
    let main = root.path().join("main");
    history::install(&main, "C");
    let linked = root.path().join("linked");
    let admin = main.join(".git/worktrees/linked");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::create_dir_all(linked.join("site/articles/foo")).unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        admin.join("gitdir"),
        format!("{}\n", linked.join(".git").display()),
    )
    .unwrap();
    std::fs::write(
        admin.join("HEAD"),
        std::fs::read(main.join(".git/refs/heads/main")).unwrap(),
    )
    .unwrap();
    for file in ["記事1.md", "foo/移動.markdown"] {
        std::fs::copy(
            main.join("site/articles").join(file),
            linked.join("site/articles").join(file),
        )
        .unwrap();
    }
    let config = config(&linked.join("site"));
    let catalog = Catalog::open_config(&config).await.unwrap();
    assert_eq!(
        catalog.snapshot().articles["記事1"].source_path,
        "site/articles/記事1.md"
    );
    assert!(catalog.snapshot().articles["記事1"].git_tracked);
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/unborn\n").unwrap();
    catalog.reconcile().await.unwrap();
    assert!(!catalog.snapshot().articles["記事1"].git_tracked);
    assert_eq!(
        catalog.snapshot().articles["記事1"].source_path,
        "site/articles/記事1.md"
    );
}
