mod common;

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use shiori::{catalog::Catalog, config::Config, rendering::Renderer, server, storage};
use tower::ServiceExt;

async fn configured(
    root: &std::path::Path,
    enabled: bool,
    base: &str,
) -> (Arc<Renderer>, anyhow::Result<Router>) {
    let mut config = Config::new(root, "127.0.0.1:0".parse().unwrap(), base).unwrap();
    config.graphiql = enabled;
    let config = Arc::new(config);
    let catalog = Catalog::open(storage::root(root).unwrap()).await.unwrap();
    let renderer = Renderer::new(config.clone(), &catalog);
    let router = server::router_with_config(catalog, renderer.clone(), &config);
    (renderer, router)
}

#[tokio::test]
async fn graphiql_and_schema_introspection_are_off_by_default() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Public", &[], "Body");
    let (_, renderer, router) = common::app(root.path()).await;
    for path in ["/graphiql", "/graphiql/assets/app.js"] {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = router
        .clone()
        .oneshot(
            Request::post("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"query":"{ __schema { queryType { name } } } }"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        result["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "{result}"
    );
    assert_eq!(
        common::data(&router, "{ posts { nodes { id title } } }").await["posts"]["nodes"][0]["title"],
        "Public"
    );
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn enabled_graphiql_serves_bundled_assets_and_keeps_graphql_policy() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "GraphiQL", &[], "Body");
    let (renderer, router) = configured(root.path(), true, "http://127.0.0.1:4000").await;
    let router = router.unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::get("/graphiql?endpoint=https://evil.example&token=secret")
                .header("host", "forged.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("script-src 'self'") && csp.contains("connect-src 'self'"));
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("data-endpoint=\"/graphql\""));
    assert!(html.contains("src=\"/graphiql/assets/app.js\""));
    for secret in ["evil.example", "forged.example", "secret"] {
        assert!(!html.contains(secret));
    }
    for (asset, mime) in [
        ("app.js", "javascript"),
        ("app.css", "text/css"),
        ("THIRD_PARTY_NOTICES.txt", "text/plain"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/graphiql/assets/{asset}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains(mime)
        );
        assert!(
            !response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
    }
    for (method, path, expected) in [
        ("HEAD", "/graphiql", StatusCode::OK),
        ("POST", "/graphiql", StatusCode::METHOD_NOT_ALLOWED),
        ("GET", "/graphiql/assets/missing.js", StatusCode::NOT_FOUND),
        ("GET", "/graphiql/assets/../app.js", StatusCode::NOT_FOUND),
        (
            "GET",
            "/graphql?query=%7Bposts%7BtotalCount%7D%7D",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path}");
        if method == "HEAD" {
            assert!(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .is_empty()
            );
        }
    }
    let value = common::data(
        &router,
        "{ __schema { queryType { name } mutationType { name } } posts { nodes { id title } } }",
    )
    .await;
    assert_eq!(value["__schema"]["queryType"]["name"], "Query");
    assert!(value["__schema"]["mutationType"].is_null());
    assert_eq!(value["posts"]["nodes"][0]["title"], "GraphiQL");
    let schema = common::data(
        &router,
        include_str!("fixtures/graphiql-introspection.graphql"),
    )
    .await;
    assert!(
        schema["__schema"]["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind["name"] == "Post"
                && kind["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|field| field["name"] == "title"))
    );
    let invalid = common::query(&router, "{ posts(first:0) { totalCount } }", json!({})).await;
    assert_eq!(invalid["errors"][0]["extensions"]["code"], "BAD_USER_INPUT");
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn graphiql_public_prefix_matches_the_existing_proxy_stripping_contract() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Prefix", &[], "");
    let (_, router) = configured(root.path(), true, "https://cms.example/blog/v1/").await;
    let router = router.unwrap();
    let response = router
        .clone()
        .oneshot(Request::get("/graphiql").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("data-endpoint=\"/blog/v1/graphql\""));
    assert!(html.contains("src=\"/blog/v1/graphiql/assets/app.js\""));
    let response = router
        .clone()
        .oneshot(
            Request::get("/graphiql/assets/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        common::data(&router, "{ posts { totalCount } }").await["posts"]["totalCount"],
        1
    );
    let (_, rejected) = configured(root.path(), true, "https://cms.example/a%20b/").await;
    assert!(rejected.is_err());
    let (_, unchanged) = configured(root.path(), false, "https://cms.example/a%20b/").await;
    assert!(unchanged.is_ok());
}
