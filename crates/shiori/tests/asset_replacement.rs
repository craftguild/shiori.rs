mod common;
use axum::http::{Method, StatusCode};
use common::{app, article, images::*};

#[tokio::test]
async fn replacement_cancels_delivery_then_rereads_raw_and_responsive_assets() {
    for path in ["/assets/photo.png", "/images/auto/photo.png"] {
        let root = tempfile::tempdir().unwrap();
        article(root.path(), 1, "Post", &[], "Body");
        asset(root.path(), "photo.png", &png(16, 8, false));
        let (_, _, router) = app(root.path()).await;
        let (status, old_headers, old) = get(&router, Method::GET, path, &[]).await;
        assert_eq!(status, StatusCode::OK);
        std::fs::rename(root.path().join("assets"), root.path().join("retired")).unwrap();
        asset(root.path(), "photo.png", &png(32, 12, false));
        let (status, headers, body) = get(
            &router,
            Method::HEAD,
            path,
            &[("if-none-match", old_headers["etag"].to_str().unwrap())],
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(headers["cache-control"], "no-store");
        assert!(body.is_empty());
        let (status, headers, new) = get(&router, Method::GET, path, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(new, old);
        assert_ne!(headers["etag"], old_headers["etag"]);
        assert_eq!(decoded(&new).dimensions(), (32, 12));
    }
}

#[tokio::test]
async fn a_symlink_replacement_stays_unavailable_until_a_real_directory_is_restored() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Post", &[], "Body");
    asset(root.path(), "file.txt", b"OLD");
    let (catalog, _, router) = app(root.path()).await;
    std::fs::write(outside.path().join("file.txt"), "private outside bytes").unwrap();
    std::fs::rename(root.path().join("assets"), root.path().join("retired")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("assets")).unwrap();
    let previous = catalog.snapshot();
    assert!(catalog.reconcile().await.is_err());
    assert!(std::sync::Arc::ptr_eq(&previous, &catalog.snapshot()));
    for _ in 0..2 {
        let (status, _, body) = get(&router, Method::GET, "/assets/file.txt", &[]).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!String::from_utf8_lossy(&body).contains("private outside"));
    }
    std::fs::remove_file(root.path().join("assets")).unwrap();
    asset(root.path(), "file.txt", b"NEW");
    assert_eq!(
        get(&router, Method::GET, "/assets/file.txt", &[]).await.0,
        503
    );
    let (status, _, bytes) = get(&router, Method::GET, "/assets/file.txt", &[]).await;
    assert_eq!(status, 200);
    assert_eq!(bytes, b"NEW");
    catalog.reconcile().await.unwrap();
}
