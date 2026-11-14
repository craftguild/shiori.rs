mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use shiori::{catalog::Catalog, metadata, storage};
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;

#[tokio::test]
async fn source_asset_tag_article_and_http_bounds_fail_safely() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Valid", &[], "Text");
    let (catalog, _, router) = app(root.path()).await;
    let file = root.path().join("contents/1.md");
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len(metadata::MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles[&1].metadata.title, "Valid");
    assert!(
        Catalog::open(storage::root(root.path()).unwrap())
            .await
            .is_err()
    );
    std::fs::create_dir(root.path().join("assets")).unwrap();
    std::fs::File::create(root.path().join("assets/large.bin"))
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::get("/assets/large.bin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = router
        .clone()
        .oneshot(
            Request::post("/graphql")
                .header("content-type", "application/json")
                .body(Body::from("x".repeat(256 * 1024 + 1)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let tags: Vec<_> = (0..1001).map(|i| format!("tag{i}")).collect();
    let yaml = serde_saphyr::to_string(&serde_json::json!({"title":"Tags","tags":tags})).unwrap();
    assert!(metadata::parse(1, format!("---\n{yaml}---\n").as_bytes()).is_err());
    // A small on-disk corpus with 10,000 distinct tags exercises the catalog cap.
    std::fs::remove_file(file).unwrap();
    for i in 0..10 {
        let tags: Vec<_> = (0..1000).map(|j| format!("tag-{}", i * 1000 + j)).collect();
        let borrowed: Vec<_> = tags.iter().map(String::as_str).collect();
        article(root.path(), i + 1, "Tags", &borrowed, "");
    }
    assert!(
        Catalog::open(storage::root(root.path()).unwrap())
            .await
            .is_err()
    );
    for entry in std::fs::read_dir(root.path().join("contents")).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    for id in 1..=10000 {
        std::fs::write(root.path().join(format!("contents/{id}.md")), "").unwrap();
    }
    let error = match Catalog::open(storage::root(root.path()).unwrap()).await {
        Err(error) => error,
        Ok(_) => panic!("oversized corpus accepted"),
    };
    assert!(error.to_string().contains("9999"));
}

#[tokio::test]
async fn unreadable_directory_and_article_keep_the_previous_snapshot() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Valid", &[], "Text");
    let (catalog, _, _) = app(root.path()).await;
    let file = root.path().join("contents/1.md");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let attempted = storage::stable_read(
        &storage::directory(&storage::root(root.path()).unwrap(), "contents").unwrap(),
        "1.md",
    );
    assert!(
        attempted.is_err(),
        "this permission test requires an unprivileged test user"
    );
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles[&1].metadata.title, "Valid");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = catalog.snapshot();
    let dir = root.path().join("contents");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = catalog.reconcile().await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &catalog.snapshot()));
    article(root.path(), 1, "Recovered", &[], "Body");
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles[&1].metadata.title, "Recovered");
}

#[tokio::test]
async fn get_transport_requires_the_real_preflight_header() {
    article_get().await;
}

async fn article_get() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "GET", &[], "");
    let (_, _, router) = app(root.path()).await;
    let encoded = percent_encoding::utf8_percent_encode(
        "{ posts { nodes { id } } }",
        percent_encoding::NON_ALPHANUMERIC,
    );
    let uri = format!("/graphql?query={encoded}");
    let rejected = router
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let accepted = router
        .clone()
        .oneshot(
            Request::get(&uri)
                .header("x-chigiri-preflight", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
}
