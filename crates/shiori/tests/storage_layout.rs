mod common;
use axum::{body::Body, http::Request};
use common::*;
use http_body_util::BodyExt;
use shiori::{catalog::Catalog, config::Config, layout::Layout, rendering::Renderer};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tower::ServiceExt;

#[tokio::test]
async fn configured_directories_are_created_and_nested_markdown_and_binary_paths_keep_identity() {
    let root = tempfile::tempdir().unwrap();
    let config = Arc::new(
        Config::with_layout(
            root.path(),
            "127.0.0.1:0".parse().unwrap(),
            "http://reader.example/",
            Layout::new("./text/posts", "./media/files").unwrap(),
        )
        .unwrap(),
    );
    assert!(root.path().join("text/posts").is_dir());
    assert!(root.path().join("media/files").is_dir());
    std::fs::create_dir(root.path().join("text/posts/foo")).unwrap();
    std::fs::write(
        root.path().join("text/posts/foo/記事2.markdown"),
        "---\ntitle: Natural key\n---\nBody",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("media/files/foo")).unwrap();
    let bytes = [0, 255, 128, 1];
    std::fs::write(root.path().join("media/files/foo/test.bin"), bytes).unwrap();
    std::fs::write(
        root.path().join("media/files/foo/photo.png"),
        images::png(16, 8, false),
    )
    .unwrap();
    let catalog = Catalog::open_config(&config).await.unwrap();
    let renderer = Renderer::new(config, &catalog);
    let router = shiori::server::router(catalog.clone(), renderer).unwrap();
    assert_eq!(
        data(&router, "{ post(id:\"foo/記事2\") { id sourcePath } }").await,
        serde_json::json!({"post":{"id":"foo/記事2","sourcePath":"text/posts/foo/記事2.markdown"}})
    );
    let response = router
        .clone()
        .oneshot(
            Request::get("/assets/foo/test.bin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        &bytes
    );
    let response = router
        .oneshot(
            Request::get("/images/auto/foo/photo.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    std::fs::write(
        root.path().join("text/posts/foo/記事2.md"),
        "---\ntitle: Duplicate\n---\n",
    )
    .unwrap();
    let old = catalog.snapshot();
    assert!(catalog.reconcile().await.is_err());
    assert!(Arc::ptr_eq(&old, &catalog.snapshot()));
}

#[test]
fn layout_rejects_escape_symlinks_overlap_and_permission_errors_without_escalation() {
    for (articles, assets) in [
        ("../outside", "assets"),
        ("/absolute", "assets"),
        (".", "assets"),
        ("a", "a/b"),
        ("a/b", "a"),
        ("a", "a"),
        ("a\\b", "assets"),
    ] {
        assert!(Layout::new(articles, assets).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
    let fd = shiori::storage::root(root.path()).unwrap();
    assert!(
        Layout::new("link/posts", "assets")
            .unwrap()
            .create(&fd)
            .is_err()
    );
    assert!(!outside.path().join("posts").exists());
    std::fs::create_dir(root.path().join("read-only")).unwrap();
    std::fs::set_permissions(
        root.path().join("read-only"),
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    let result = Layout::new("read-only/missing", "assets")
        .unwrap()
        .create(&fd);
    std::fs::set_permissions(
        root.path().join("read-only"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(
        result.is_err(),
        "permission test must run as an ordinary user"
    );
}

#[test]
fn generate_honors_environment_and_reserves_both_markdown_extensions() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shiori"))
        .env("SHIORI_ARTICLES_DIR", "./text/posts")
        .env("SHIORI_ASSETS_DIR", "./media/files")
        .args([
            "--root",
            root.path().to_str().unwrap(),
            "generate",
            "--title",
            "Configured",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    assert_eq!(
        path.parent().unwrap(),
        root.path().canonicalize().unwrap().join("text/posts")
    );
    assert!(
        root.path()
            .join("media/files")
            .join(path.file_stem().unwrap())
            .is_dir()
    );
    let plain = tempfile::tempdir().unwrap();
    std::fs::create_dir(plain.path().join("articles")).unwrap();
    std::fs::write(plain.path().join("articles/100.markdown"), "preserved").unwrap();
    let mut ticks = ["100", "101"].into_iter();
    let path = shiori::generate::generate_with_clock(plain.path(), "New", || {
        Ok(ticks.next().unwrap().to_owned())
    })
    .unwrap();
    assert_eq!(path.file_name().unwrap(), "101.md");
    assert_eq!(
        std::fs::read_to_string(plain.path().join("articles/100.markdown")).unwrap(),
        "preserved"
    );
}
