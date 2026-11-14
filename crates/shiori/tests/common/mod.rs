#![allow(dead_code)]
pub mod images;
pub mod release_images;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use shiori::{catalog::Catalog, config::Config, rendering::Renderer, storage};
use std::{path::Path, sync::Arc};
use tower::ServiceExt;

pub fn article(root: &Path, id: i64, title: &str, tags: &[&str], body: &str) {
    std::fs::create_dir_all(root.join("contents")).unwrap();
    let metadata = json!({"title":title,"description":"Description λ", "tags":tags});
    let text = format!(
        "---\n{}---\n{body}",
        serde_saphyr::to_string(&metadata).unwrap()
    );
    std::fs::write(root.join(format!("contents/{id}.md")), text).unwrap();
}

pub async fn app(root: &Path) -> (Arc<Catalog>, Arc<Renderer>, Router) {
    let config = Arc::new(
        Config::new(
            root,
            "127.0.0.1:0".parse().unwrap(),
            "https://cms.example/blog/",
        )
        .unwrap(),
    );
    let catalog = Catalog::open(storage::root(root).unwrap()).await.unwrap();
    let renderer = Renderer::new(config, &catalog);
    let router = shiori::server::router(catalog.clone(), renderer.clone()).unwrap();
    (catalog, renderer, router)
}

pub async fn query(router: &Router, query: &str, variables: Value) -> Value {
    let response = router
        .clone()
        .oneshot(
            Request::post("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"query":query,"variables":variables}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

pub async fn data(router: &Router, query_text: &str) -> Value {
    let value = query(router, query_text, json!({})).await;
    assert!(value.get("errors").is_none(), "{value:#}");
    value["data"].clone()
}
