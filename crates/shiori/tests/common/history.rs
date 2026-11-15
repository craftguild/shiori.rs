use base64::Engine;
use serde_json::Value;
use std::path::Path;

pub async fn app(
    root: &Path,
) -> (
    std::sync::Arc<shiori::catalog::Catalog>,
    std::sync::Arc<shiori::rendering::Renderer>,
    axum::Router,
) {
    let config = std::sync::Arc::new(
        shiori::config::Config::with_layout(
            &root.join("site"),
            "127.0.0.1:0".parse().unwrap(),
            "http://reader.example/",
            shiori::layout::Layout::default(),
        )
        .unwrap(),
    );
    let catalog = shiori::catalog::Catalog::open_config(&config)
        .await
        .unwrap();
    let renderer = shiori::rendering::Renderer::new(config, &catalog);
    let router = shiori::server::router(catalog.clone(), renderer.clone()).unwrap();
    (catalog, renderer, router)
}

pub fn install(root: &Path, head: &str) {
    let data: Value = serde_json::from_str(include_str!("../fixtures/history.json")).unwrap();
    std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
    std::fs::write(
        root.join(".git/config"),
        "[core]\nrepositoryformatversion = 0\nbare = false\n",
    )
    .unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    for (oid, data) in data["objects"].as_object().unwrap() {
        let directory = root.join(".git/objects").join(&oid[..2]);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(&oid[2..]),
            base64::engine::general_purpose::STANDARD
                .decode(data.as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
    }
    for (path, text) in data["files"].as_object().unwrap() {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text.as_str().unwrap()).unwrap();
    }
    if head == "P" || head == "N" {
        let group = if head == "P" {
            "page_files"
        } else {
            "merge_files"
        };
        for (path, text) in data[group].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text.as_str().unwrap()).unwrap();
        }
    }
    if let Some(files) = data.get("variants").and_then(|variants| variants.get(head)) {
        for (path, text) in files.as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text.as_str().unwrap()).unwrap();
        }
    }
    select(root, head);
}

pub fn select(root: &Path, head: &str) {
    let data: Value = serde_json::from_str(include_str!("../fixtures/history.json")).unwrap();
    std::fs::write(
        root.join(".git/refs/heads/main"),
        format!("{}\n", data["heads"][head].as_str().unwrap()),
    )
    .unwrap();
}
