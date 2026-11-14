mod common;
use common::*;
use serde_json::json;

#[tokio::test]
async fn exact_query_pagination_tags_defaults_and_errors() {
    let root = tempfile::tempdir().unwrap();
    for id in [9, 10, 100] {
        article(
            root.path(),
            id,
            &format!("Post {id}"),
            &[" Rust ", "Rust", "rust"],
            "Text",
        );
    }
    article(root.path(), 50, "Other", &["other"], "");
    let (_, renderer, router) = app(root.path()).await;
    let value = data(&router, "{ posts(first:2) { nodes { id title createdAt } totalCount pageInfo { endCursor hasNextPage } } tags { name count } }").await;
    assert_eq!(value["posts"]["nodes"][0]["id"], "100");
    assert_eq!(value["posts"]["nodes"][1]["id"], "50");
    assert_eq!(value["posts"]["totalCount"], 4);
    assert_eq!(
        value["posts"]["pageInfo"],
        json!({"endCursor":"50","hasNextPage":true})
    );
    assert_eq!(
        value["tags"],
        json!([{"name":"Rust","count":3},{"name":"other","count":1},{"name":"rust","count":3}])
    );
    let value = data(&router, "{ posts(first:null, after:\"49\", tag:\" Rust \") { nodes { id } totalCount pageInfo { endCursor hasNextPage } } post(id:\"8\") { id } }").await;
    assert_eq!(value["posts"]["nodes"], json!([{"id":"10"},{"id":"9"}]));
    assert_eq!(value["posts"]["totalCount"], 3);
    assert_eq!(value["post"], json!(null));
    let empty = data(
        &router,
        "{ posts(after:\"1\") { nodes { id } pageInfo { endCursor hasNextPage } } }",
    )
    .await;
    assert_eq!(
        empty["posts"]["pageInfo"],
        json!({"endCursor":null,"hasNextPage":false})
    );
    for query_text in [
        "{ post(id:\"01\") { id } }",
        "{ posts(first:0) { totalCount } }",
        "{ posts(first:101) { totalCount } }",
        "{ posts(after:\"0\") { totalCount } }",
        "{ posts(tag:\" \") { totalCount } }",
    ] {
        let value = query(&router, query_text, json!({})).await;
        assert_eq!(
            value["errors"][0]["extensions"]["code"], "BAD_USER_INPUT",
            "{value}"
        );
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn real_rendering_resources_images_and_failure_isolation() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Render",
        &[],
        "# Same\n\n# Same\n\nA **bold** [link][r] and note[^n]. ![local](/assets/photo.png?q=1#x) ![remote](https://images.example/a.png)\n\n> ![nested](/assets/nested.png)\n\n| Image |\n| --- |\n| ![cell](/assets/cell.png) |\n\n[r]: https://example.com\n[^n]: Note ![foot](/assets/foot.png).\n\n<script>alert(1)</script>\n\nInline <b>unsafe</b>\n\n$$\nx^2\n$$\n\n```mermaid\nflowchart LR\n A --> B\n```\n\n```plantuml\n@startuml\nAlice -> Bob: Hi\n@enduml\n```\n",
    );
    article(root.path(), 2, "Empty", &[], "");
    article(
        root.path(),
        3,
        "Failed",
        &[],
        &format!("{}x{}", "![".repeat(300), "](x)".repeat(300)),
    );
    let (_, renderer, router) = app(root.path()).await;
    let query_text =
        "{ post(id:\"1\") { title blocks { id kind html } assets { id kind mimeType content } } }";
    let value = data(&router, query_text).await;
    let blocks = value["post"]["blocks"].as_array().unwrap();
    assert!(
        blocks
            .iter()
            .all(|b| b["html"].is_string() && b.as_object().unwrap().len() == 3)
    );
    assert!(
        blocks
            .iter()
            .all(|b| b["kind"] != "blankLine" && b["kind"] != "linkReferenceDefinition")
    );
    let html: String = blocks.iter().map(|b| b["html"].as_str().unwrap()).collect();
    assert!(
        blocks
            .iter()
            .any(|b| b["id"] == "oriha-generated-footnotes" && b["kind"] == "footnotes")
    );
    for expected in [
        "<strong>bold</strong>",
        "https://example.com",
        "id=\"same-2\"",
        "#fn-n",
        "https://cms.example/blog/assets/photo.png?q=1#x",
        "https://cms.example/blog/assets/nested.png",
        "https://cms.example/blog/assets/cell.png",
        "https://cms.example/blog/assets/foot.png",
        "https://images.example/a.png",
        "&lt;script&gt;",
        "&lt;b&gt;",
        "<svg",
    ] {
        assert!(html.contains(expected), "missing {expected} in {html}");
    }
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<b>unsafe"));
    let assets = value["post"]["assets"].as_array().unwrap();
    assert!(
        assets
            .iter()
            .any(|a| a["mimeType"] == "font/otf" && a["content"].as_str().unwrap().len() > 100)
    );
    assert_eq!(renderer.calls(), 1);
    assert_eq!(data(&router, query_text).await, value);
    assert_eq!(renderer.calls(), 1);
    assert_eq!(
        data(
            &router,
            "{ post(id:\"2\") { blocks { html } assets { id } } }"
        )
        .await["post"],
        json!({"blocks":[],"assets":[]})
    );
    let failed = query(
        &router,
        "{ post(id:\"3\") { id title blocks { html } assets { id } } }",
        json!({}),
    )
    .await;
    assert_eq!(failed["data"]["post"]["title"], "Failed");
    assert_eq!(failed["data"]["post"]["blocks"], json!(null));
    assert_eq!(failed["data"]["post"]["assets"], json!(null));
    assert_eq!(failed["errors"].as_array().unwrap().len(), 2);
    assert!(
        failed["errors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["extensions"]["code"] == "RENDER_FAILED")
    );
    assert!(!failed.to_string().contains(root.path().to_str().unwrap()));
    assert_eq!(renderer.calls(), 3);
}
