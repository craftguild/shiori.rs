mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn real_asset_bytes_mime_revalidation_cors_and_boundaries() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Asset", &[], "![a](/assets/image.png)");
    std::fs::create_dir(root.path().join("assets")).unwrap();
    let bytes = b"\x89PNG\r\n\x1a\nactual bytes";
    std::fs::write(root.path().join("assets/image.png"), bytes).unwrap();
    std::fs::create_dir(root.path().join("assets/folder")).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.path().join("assets/pipe"))
            .status()
            .unwrap()
            .success()
    );
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        root.path().join("assets/link.txt"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("assets/escape")).unwrap();
    let (_, _, router) = common::app(root.path()).await;
    let response = router
        .clone()
        .oneshot(
            Request::get("/assets/image.png")
                .header("origin", "https://frontend.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response
            .headers()
            .get("access-control-allow-credentials")
            .is_none()
    );
    let etag = response.headers()["etag"].clone();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        bytes.as_slice()
    );
    let response = router
        .clone()
        .oneshot(
            Request::get("/assets/image.png")
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len(),
        0
    );
    std::fs::write(root.path().join("assets/image.png"), b"new").unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::get("/assets/image.png")
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_ne!(response.headers()["etag"], etag);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        b"new".as_slice()
    );
    let head = router
        .clone()
        .oneshot(
            Request::head("/assets/image.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head.headers()["content-length"], "3");
    assert!(
        head.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    for path in [
        "/assets/pipe",
        "/assets/../contents/1.md",
        "/assets/%2e%2e/contents/1.md",
        "/assets/%2e%2e%2fcontents%2f1.md",
        "/assets/%5c..%5csecret.txt",
        "/assets/%00",
        "/assets/link.txt",
        "/assets/escape/secret.txt",
        "/assets/folder",
        "/assets/missing",
        "/assets/",
    ] {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(!response.status().is_success(), "{path}");
        assert!(
            !String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .contains("secret")
        );
    }
    let preflight = router
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/graphql")
                .header("origin", "https://frontend.example")
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(preflight.status().is_success());
    assert_eq!(preflight.headers()["access-control-allow-origin"], "*");
    std::fs::remove_dir(root.path().join("assets/folder")).unwrap();
    std::fs::remove_file(root.path().join("assets/pipe")).unwrap();
    std::fs::remove_file(root.path().join("assets/image.png")).unwrap();
    std::fs::remove_file(root.path().join("assets/link.txt")).unwrap();
    std::fs::remove_file(root.path().join("assets/escape")).unwrap();
    std::fs::remove_dir(root.path().join("assets")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("assets")).unwrap();
    let response = router
        .oneshot(
            Request::get("/assets/secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn public_base_validation_and_exact_prefix_policy() {
    use shiori::config::Config;
    let root = tempfile::tempdir().unwrap();
    for base in [
        "/relative",
        "ftp://host",
        "https://user@host",
        "https://host/?q=1",
        "https://host/#x",
    ] {
        assert!(
            Config::new(root.path(), "127.0.0.1:0".parse().unwrap(), base).is_err(),
            "{base}"
        );
    }
    let config = Config::new(
        root.path(),
        "127.0.0.1:0".parse().unwrap(),
        "https://cms.example/prefix",
    )
    .unwrap();
    assert_eq!(
        config.image_url("/assets/a%20b.png?q=x#f").unwrap(),
        "https://cms.example/prefix/assets/a%20b.png?q=x#f"
    );
    for input in [
        "https://external.example/assets/a.png",
        "//external.example/a",
        "assets/a.png",
        "/other/assets/a.png",
    ] {
        assert!(config.image_url(input).is_none());
    }
}
