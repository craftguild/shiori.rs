mod common;
use common::*;
use serde_json::json;

fn rss_query() -> String {
    let operations = include_str!("fixtures/rss.graphql");
    operations[operations.find("query RssItem").unwrap()..].to_string()
}

#[tokio::test]
async fn structured_only_is_complete_typed_flat_and_never_prepares_images() {
    let root = tempfile::tempdir().unwrap();
    let deep = format!("{}deep", "> ".repeat(64));
    article(
        root.path(),
        1,
        "Structured",
        &[],
        &format!(
            "# 日本語 :smile: `code` $x$\n\n# 日本語 :smile: `code` $x$\n\n[heading](#日本語-smile-code-x) [relative](../other) [unsafe](javascript:bad)\n\n![alt](/assets/missing.png)\n\nInline $\\bad{{SECRET}}$\n\n```mermaid\nINVALID SECRET\n```\n\n| A | B |\n| - | - |\n| | bee |\n\n[^a]: old\n[^a]: last\n\nNote[^a]\n\n[r]: /reference\n\n{deep}\n"
        ),
    );
    let (_, renderer, router) = app(root.path()).await;
    let value = query(&router, &rss_query(), json!({"id":"1"})).await;
    assert!(value.get("errors").is_none(), "{value:#}");
    let doc = &value["data"]["post"]["structured"];
    assert_eq!(doc["schemaVersion"], 1);
    let blocks = doc["blocks"].as_array().unwrap();
    assert!(
        blocks
            .iter()
            .filter(|n| n["__typename"] == "BlockBlockQuote")
            .count()
            >= 64
    );
    let table = blocks
        .iter()
        .find(|n| n["__typename"] == "BlockGfmTable")
        .unwrap();
    assert_eq!(table["rows"][0]["cells"][0]["richTextIds"], json!([]));
    assert_eq!(table["rows"][0]["cells"][1]["text"], "bee");
    assert!(
        doc["inlines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["__typename"] == "InlineMath"
                && n["text"].as_str().unwrap().contains("SECRET"))
    );
    let unsafe_link = doc["inlines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["url"] == "javascript:bad")
        .unwrap();
    assert_eq!(unsafe_link["normalizedUrl"], "#");
    assert_eq!(renderer.calls(), 1);
    assert_eq!(renderer.parses(), 1);
    assert_eq!(renderer.images.inspections(), 0);
    assert_eq!(renderer.images.transforms(), 0);
    let again = query(&router, &rss_query(), json!({"id":"1"})).await;
    assert_eq!(again, value);
    assert_eq!(renderer.calls(), 1);
    assert_eq!(renderer.parses(), 1);
}

#[tokio::test]
async fn mixed_representations_share_one_request_parse_and_keep_html_compatible() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Mixed",
        &[],
        "# Same\n\n# Same\n\nProse [link](#same) note[^n]\n\n[^n]: Definition\n",
    );
    let (_, renderer, router) = app(root.path()).await;
    let value = data(&router, "{post(id:\"1\"){ blocks{id html} assets{id} structured{schemaVersion anchors blocks{__typename id text childIds} inlines{__typename id text childIds}} }}").await;
    assert_eq!(renderer.calls(), 2);
    assert_eq!(renderer.parses(), 1);
    let html = value["post"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["html"].as_str().unwrap())
        .collect::<String>();
    for anchor in value["post"]["structured"]["anchors"].as_array().unwrap() {
        assert!(html.contains(&format!("id=\"{}\"", anchor.as_str().unwrap())));
    }
    assert!(html.contains("id=\"same-2\""));
}

#[tokio::test]
async fn structured_failure_preserves_metadata_and_has_a_generic_public_error() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Metadata remains",
        &[],
        &"x\n\n".repeat(20_001),
    );
    let (_, renderer, router) = app(root.path()).await;
    let value = data(&router, "{post(id:\"1\"){id title}}").await;
    assert_eq!(value["post"]["title"], "Metadata remains");
    assert_eq!(renderer.calls(), 0);
    let value = query(
        &router,
        "{post(id:\"1\"){id title structured{schemaVersion}}}",
        json!({}),
    )
    .await;
    assert_eq!(value["data"]["post"]["id"], "1");
    assert!(value["data"]["post"]["structured"].is_null());
    assert_eq!(value["errors"][0]["extensions"]["code"], "STRUCTURE_FAILED");
    assert_eq!(value["errors"][0]["message"], "Article structure failed");
    assert_eq!(renderer.images.inspections(), 0);
}
