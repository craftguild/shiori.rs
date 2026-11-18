mod common;
use common::*;
use serde_json::json;

fn rss_query() -> String {
    let operations = include_str!("fixtures/rss.graphql");
    operations[operations.find("query RssItem").unwrap()..].to_string()
}

#[tokio::test]
async fn document_only_is_complete_typed_flat_and_never_prepares_images() {
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
    let doc = &value["data"]["post"]["document"];
    assert_eq!(doc["schemaVersion"], 2);
    let nodes = doc["nodes"].as_array().unwrap();
    assert!(
        nodes
            .iter()
            .filter(|n| n["__typename"] == "BlockQuoteNode")
            .count()
            >= 64
    );
    let table = nodes
        .iter()
        .find(|n| n["__typename"] == "GfmTableNode")
        .unwrap();
    let find = |id: &serde_json::Value| nodes.iter().find(|n| &n["id"] == id).unwrap();
    let row = find(&table["rowIds"][1]);
    let first = find(&row["cellIds"][0]);
    assert_eq!(first["childIds"], json!([]));
    let second = find(&row["cellIds"][1]);
    assert_eq!(find(&second["childIds"][0])["text"], "bee");
    assert!(
        nodes
            .iter()
            .any(|n| n["__typename"] == "MathNode"
                && n["source"].as_str().unwrap().contains("SECRET"))
    );
    let unsafe_link = nodes
        .iter()
        .find(|n| n["destination"] == "javascript:bad")
        .unwrap();
    assert_eq!(unsafe_link["normalizedDestination"], "#");
    let reference = nodes
        .iter()
        .find(|n| n["__typename"] == "FootnoteReferenceNode")
        .unwrap();
    assert!(
        doc["footnoteIds"]
            .as_array()
            .unwrap()
            .contains(&reference["definitionId"])
    );
    let definition = find(&reference["definitionId"]);
    assert_eq!(definition["label"], "a");
    assert_eq!(reference["definitionId"], definition["id"]);
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
async fn retired_body_fields_have_no_compatibility_aliases() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Retired", &[], "Text");
    let (_, renderer, router) = app(root.path()).await;
    for field in [
        "blocks { html }",
        "assets { content }",
        "structured { schemaVersion }",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::post("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"query":format!("{{post(id:\"1\"){{{field}}}}}")}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let response: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(
            response["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty())
        );
        assert!(response.get("data").is_none());
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn document_failure_preserves_metadata_and_has_a_generic_public_error() {
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
        "{post(id:\"1\"){id title document{schemaVersion}}}",
        json!({}),
    )
    .await;
    assert_eq!(value["data"]["post"]["id"], "1");
    assert!(value["data"]["post"]["document"].is_null());
    assert_eq!(value["errors"][0]["extensions"]["code"], "STRUCTURE_FAILED");
    assert_eq!(value["errors"][0]["message"], "Article structure failed");
    assert_eq!(renderer.images.inspections(), 0);
}
