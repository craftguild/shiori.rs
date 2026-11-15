mod common;
use common::*;
use serde_json::{Value, json};

const SEARCH: &str = "query($q:String,$tag:String,$after:ID,$before:ID){posts(query:$q,tag:$tag,first:2,after:$after,before:$before){nodes{id title}totalCount pageInfo{startCursor endCursor hasPreviousPage hasNextPage}}}";

#[tokio::test]
async fn search_filters_before_date_key_bidirectional_pagination_without_rendering() {
    let root = tempfile::tempdir().unwrap();
    for id in [9, 10, 20, 30, 100] {
        article(
            root.path(),
            id,
            "Rust 東京",
            &["Web", "日本語"],
            "A PRINTLN example.",
        );
    }
    article(
        root.path(),
        200,
        "Unrelated",
        &["rust"],
        "Does not match the body.",
    );
    let (_, renderer, router) = app(root.path()).await;
    let first = query(&router, SEARCH, json!({"q":"RUST println 日本語"})).await;
    assert!(first.get("errors").is_none(), "{first}");
    assert_eq!(first["data"]["posts"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(first["data"]["posts"]["totalCount"], 5);
    let second = query(
        &router,
        SEARCH,
        json!({"q":"rust println 日本語","after":first["data"]["posts"]["pageInfo"]["endCursor"]}),
    )
    .await;
    assert!(second.get("errors").is_none(), "{second}");
    assert_eq!(
        second["data"]["posts"]["nodes"].as_array().unwrap().len(),
        2
    );
    let newer=query(&router,SEARCH,json!({"q":"rust println 日本語","before":second["data"]["posts"]["pageInfo"]["startCursor"]})).await;
    assert_eq!(newer["data"]["posts"], first["data"]["posts"]);
    let exact_tag = query(&router, SEARCH, json!({"q":"rust println","tag":"web"})).await;
    assert_eq!(exact_tag["data"]["posts"]["totalCount"], 0);
    let exact_tag = query(&router, SEARCH, json!({"q":"rust println","tag":" Web "})).await;
    assert_eq!(exact_tag["data"]["posts"]["totalCount"], 5);
    for q in [Value::Null, json!("rust"), json!("東京")] {
        let result = query(&router, SEARCH, json!({"q":q})).await;
        assert!(result.get("errors").is_none(), "{result}");
        assert_eq!(
            result["data"]["posts"]["totalCount"],
            if q == json!("東京") { 5 } else { 6 }
        );
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn raw_markdown_limits_empty_results_and_catalog_changes() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Original",
        &[],
        "```rust\nprintln!(\"λ\");\n```\n[link](https://hidden.test)",
    );
    let (catalog, renderer, router) = app(root.path()).await;
    for q in ["println", "HIDDEN.TEST", "!(\"λ\");"] {
        let found = query(&router, SEARCH, json!({"q":q})).await;
        assert_eq!(found["data"]["posts"]["totalCount"], 1);
    }
    for q in [
        String::new(),
        " \n\t".into(),
        "日".repeat(257),
        "x ".repeat(17),
    ] {
        let result = query(&router, SEARCH, json!({"q":q})).await;
        assert_eq!(
            result["errors"][0]["extensions"]["code"], "BAD_USER_INPUT",
            "{result}"
        );
    }
    article(root.path(), 1, "Changed", &["NewTag"], "Different body");
    catalog.reconcile().await.unwrap();
    let old = query(&router, SEARCH, json!({"q":"println"})).await;
    assert_eq!(old["data"]["posts"]["totalCount"], 0);
    assert_eq!(
        old["data"]["posts"]["pageInfo"],
        json!({"startCursor":null,"endCursor":null,"hasPreviousPage":false,"hasNextPage":false})
    );
    assert_eq!(
        query(&router, SEARCH, json!({"q":"changed newtag different"})).await["data"]["posts"]["totalCount"],
        1
    );
    std::fs::remove_file(root.path().join("articles/1.md")).unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(
        query(&router, SEARCH, json!({"q":"newtag"})).await["data"]["posts"]["totalCount"],
        0
    );
    assert_eq!(renderer.calls(), 0);
}
