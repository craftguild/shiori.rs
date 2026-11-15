mod common;
use common::*;
use serde_json::json;
use shiori::{generate::generate, metadata};

#[test]
fn optional_author_names_are_trimmed_exact_strings_without_coercion() {
    for (field, expected) in [
        ("", ""),
        ("author: null", ""),
        ("author: ''", ""),
        ("author: '  '", ""),
        ("author: '  山田 太郎 λ  '", "山田 太郎 λ"),
        ("author: Alice", "Alice"),
        ("author: alice", "alice"),
    ] {
        let source = format!("---\ntitle: Example\n{field}\n---\n本文");
        assert_eq!(
            metadata::parse(1, source.as_bytes())
                .unwrap()
                .metadata
                .author,
            expected
        );
    }
    for field in [
        "author: 1",
        "author: true",
        "author: []",
        "author: {}",
        "author: A\nauthor: B",
    ] {
        let source = format!("---\ntitle: Example\n{field}\n---\n");
        assert!(metadata::parse(1, source.as_bytes()).is_err(), "{field}");
    }
}

#[test]
fn generated_template_has_an_editable_empty_author() {
    let root = tempfile::tempdir().unwrap();
    let path = generate(root.path(), "Example").unwrap();
    let source = std::fs::read(&path).unwrap();
    let id = metadata::parse_id(path.file_stem().unwrap().to_str().unwrap()).unwrap();
    assert!(
        metadata::parse(id, &source)
            .unwrap()
            .metadata
            .author
            .is_empty()
    );
    let front = std::str::from_utf8(&source)
        .unwrap()
        .split("---")
        .nth(1)
        .unwrap();
    let yaml: serde_json::Value = serde_saphyr::from_str(front).unwrap();
    assert_eq!(yaml["author"], "");
    assert!(root.path().join(format!("assets/{id}")).is_dir());
}

#[tokio::test]
async fn author_queries_are_lazy_and_reconciliation_updates_names_without_changing_identity() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Legacy", &[], "Legacy body");
    let path = root.path().join("articles/2.md");
    let source = |name: &str| {
        format!(
            "---\n{}\n---\n本文",
            json!({"title":"Named", "author":name, "tags":["日記"]})
        )
    };
    std::fs::write(&path, source("  山田 太郎  ")).unwrap();
    let (catalog, renderer, router) = app(root.path()).await;
    let query = "{ post(id:\"2\") { id author createdAt } posts { nodes { id author } } }";
    let first = data(&router, query).await;
    assert_eq!(first["post"]["author"], "山田 太郎");
    let nodes = first["posts"]["nodes"].as_array().unwrap();
    assert_eq!(
        nodes.iter().find(|node| node["id"] == "2").unwrap()["author"],
        "山田 太郎"
    );
    assert!(nodes.iter().find(|node| node["id"] == "1").unwrap()["author"].is_null());
    assert_eq!(renderer.calls(), 0);
    let old = catalog.snapshot();
    std::fs::write(&path, source("別の投稿者")).unwrap();
    catalog.reconcile().await.unwrap();
    let changed = data(&router, query).await;
    assert_eq!(changed["post"]["author"], "別の投稿者");
    assert_eq!(changed["post"]["id"], first["post"]["id"]);
    assert_eq!(changed["post"]["createdAt"], first["post"]["createdAt"]);
    assert_eq!(old.articles["2"].metadata.author, "山田 太郎");
    assert_eq!(renderer.calls(), 0);
}
