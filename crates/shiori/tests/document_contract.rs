mod common;
#[allow(dead_code)]
mod release_support;
use common::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const QUERY: &str = include_str!("fixtures/document.graphql");
const SOURCE: &str = "# Same\n\n# Same\n\nProse **strong** [safe](https://example.test) ![alt](/assets/photo.png) note[^n].\n\n```rust\nlet 日本 = 1;\n\n```\n\n```unknown-language\nplain α\n```\n\n$x^2$\n\n$$\n\\frac{a}{b}\n$$\n\n```mermaid\nflowchart LR\nA --> B\n```\n\n```plantuml\n@startuml\nAlice -> Bob: Hi\n@enduml\n```\n\n| A | B |\n| - | - |\n| 1 | bee |\n\n- [x] done\n\n> [!NOTE] Custom\n> Body\n\n[^n]: Definition\n\n[r]: /reference\n";

fn nodes(document: &Value) -> &[Value] {
    document["nodes"].as_array().unwrap()
}
fn kind<'a>(document: &'a Value, name: &str) -> &'a Value {
    nodes(document)
        .iter()
        .find(|node| node["__typename"] == name)
        .unwrap()
}

#[test]
#[ignore = "requires an explicitly selected, prepared shiori executable"]
fn prepared_binary_serves_the_genuine_document_contract_over_http() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Typed HTTP", &[], SOURCE);
    let pixel = images::png(640, 480, false);
    images::asset(root.path(), "photo.png", &pixel);
    let server = release_support::Server::start(root.path(), "127.0.0.1", None, false);
    for prepare in [false, true] {
        let response = server
            .client
            .post(format!("{}/graphql", server.origin))
            .json(&json!({"query":QUERY,"variables":{"id":"1","prepare":prepare}}))
            .send()
            .unwrap();
        assert_eq!(response.status(), 200);
        let response: Value = response.json().unwrap();
        assert!(response.get("errors").is_none(), "{response:#}");
        assert_eq!(response["data"]["post"]["title"], "Typed HTTP");
        let plain = &response["data"]["post"]["plain"];
        let rich = &response["data"]["post"]["rich"];
        assert_eq!(rich["schemaVersion"], 2);
        assert_eq!(plain["rootIds"], rich["rootIds"]);
        assert_eq!(nodes(plain).len(), nodes(rich).len());
        if prepare {
            let code = kind(rich, "FencedCodeNode");
            assert!(code["h"]["lines"].as_array().unwrap().len() > 1);
            for name in ["MathNode", "MathBlockNode"] {
                assert!(
                    kind(rich, name)["svg"]["markup"]
                        .as_str()
                        .unwrap()
                        .starts_with("<svg")
                );
            }
            for language in ["mermaid", "plantuml"] {
                let node = nodes(rich)
                    .iter()
                    .find(|node| node["language"] == language)
                    .unwrap();
                assert!(node["svg"]["markup"].as_str().unwrap().starts_with("<svg"));
            }
            assert_eq!(rich["fonts"].as_array().unwrap().len(), 1);
            let font = &rich["fonts"][0];
            assert_eq!(font["mimeType"], "font/otf");
            assert!(
                STANDARD
                    .decode(font["content"].as_str().unwrap())
                    .unwrap()
                    .starts_with(b"OTTO")
            );
            let image = &kind(rich, "ImageNode")["presentation"];
            assert_eq!(image["width"], 640);
            assert_eq!(image["height"], 480);
            let response = server
                .client
                .get(image["url"].as_str().unwrap())
                .send()
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["content-type"], "image/png");
            assert_eq!(response.bytes().unwrap().as_ref(), pixel);
        } else {
            assert!(kind(rich, "MathNode").get("svg").is_none());
            assert!(kind(rich, "FencedCodeNode").get("h").is_none());
            assert!(rich.get("fonts").is_none());
        }
    }
    server.close();
}

#[tokio::test]
async fn genuine_flat_union_execution_prepares_only_effective_selections_and_shares_parsing() {
    for prepare in [false, true] {
        let root = tempfile::tempdir().unwrap();
        article(root.path(), 1, "Typed", &[], SOURCE);
        images::asset(root.path(), "photo.png", &images::png(640, 480, false));
        let (_, renderer, router) = app(root.path()).await;
        let metadata = data(&router, "{post(id:\"1\"){id title}}").await;
        assert_eq!(metadata["post"]["title"], "Typed");
        assert_eq!(renderer.calls(), 0);
        assert_eq!(renderer.parses(), 0);
        assert_eq!(renderer.images.inspections(), 0);
        let response = query(&router, QUERY, json!({"id":"1", "prepare":prepare})).await;
        assert!(response.get("errors").is_none(), "{response:#}");
        let plain = &response["data"]["post"]["plain"];
        let rich = &response["data"]["post"]["rich"];
        assert_eq!(plain["schemaVersion"], 2);
        assert_eq!(plain["rootIds"], rich["rootIds"]);
        let identities = |document: &Value| {
            nodes(document)
                .iter()
                .map(|node| {
                    (
                        node["id"].as_str().unwrap().to_owned(),
                        node["__typename"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(identities(plain), identities(rich));
        assert_eq!(
            nodes(plain).len(),
            identities(plain)
                .iter()
                .map(|(id, _)| id)
                .collect::<BTreeSet<_>>()
                .len()
        );
        assert_eq!(renderer.parses(), 1);
        assert_eq!(renderer.calls(), if prepare { 2 } else { 1 });
        assert_eq!(renderer.images.inspections(), usize::from(prepare));
        assert_eq!(renderer.images.transforms(), 0);
        if prepare {
            assert!(rich["diagnostics"].as_array().unwrap().is_empty());
            assert_eq!(rich["fonts"].as_array().unwrap().len(), 1);
            assert_eq!(rich["fonts"][0]["mimeType"], "font/otf");
            assert_eq!(rich["fonts"][0]["encoding"], "BASE64");
            let code = kind(rich, "FencedCodeNode");
            let lines = code["h"]["lines"].as_array().unwrap();
            let reconstructed: Vec<String> = lines
                .iter()
                .map(|line| {
                    line["tokens"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|token| token["text"].as_str().unwrap())
                        .collect()
                })
                .collect();
            assert_eq!(reconstructed.join("\n"), code["source"].as_str().unwrap());
            assert_eq!(lines.len(), 3);
            for node in nodes(rich).iter().filter(|node| node["svg"].is_object()) {
                assert!(node["svg"]["markup"].as_str().unwrap().starts_with("<svg"));
                assert!(node["svg"]["width"].as_f64().unwrap() > 0.0);
                assert!(node["svg"]["height"].as_f64().unwrap() > 0.0);
            }
            assert_eq!(
                nodes(rich)
                    .iter()
                    .filter(|node| node["svg"].is_object())
                    .count(),
                4
            );
            let image = kind(rich, "ImageNode");
            assert_eq!(image["presentation"]["width"], 640);
            assert_eq!(image["presentation"]["height"], 480);
            assert!(
                image["presentation"]["url"]
                    .as_str()
                    .unwrap()
                    .contains("/images/1280/")
            );
            assert!(
                image["presentation"]["originalUrl"]
                    .as_str()
                    .unwrap()
                    .contains("/assets/photo.png")
            );
            assert!(
                !image["presentation"]["candidates"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(plain, rich);
            assert!(rich.get("fonts").is_none());
        }
        let again = query(&router, QUERY, json!({"id":"1", "prepare":prepare})).await;
        assert_eq!(again, response);
        assert_eq!(renderer.calls(), if prepare { 2 } else { 1 });
    }
}

#[tokio::test]
async fn node_local_preparation_failure_retains_source_and_global_failure_isolates_the_body() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Local",
        &[],
        "before\n\n$\\badCommand{SECRET}$\n\n```mermaid\nSECRET invalid\n```\n\nafter",
    );
    article(root.path(), 2, "Global", &[], &"x\n\n".repeat(20_001));
    article(root.path(), 3, "Other", &[], "other text");
    let (_, _, router) = app(root.path()).await;
    let response = query(&router, QUERY, json!({"id":"1", "prepare":true})).await;
    assert!(response.get("errors").is_none(), "{response:#}");
    let document = &response["data"]["post"]["rich"];
    assert_eq!(document["diagnostics"].as_array().unwrap().len(), 2);
    for diagnostic in document["diagnostics"].as_array().unwrap() {
        assert_eq!(diagnostic["severity"], "WARNING");
        assert!(!diagnostic["message"].as_str().unwrap().contains("SECRET"));
        let node = nodes(document)
            .iter()
            .find(|node| node["id"] == diagnostic["nodeId"])
            .unwrap();
        assert!(node["source"].as_str().unwrap().contains("SECRET"));
        assert!(node["svg"].is_null());
    }
    let response = query(&router, "{bad:post(id:\"2\"){id title doc:document{schemaVersion}} good:post(id:\"3\"){id title document{schemaVersion}}}", json!({})).await;
    assert_eq!(response["data"]["bad"]["title"], "Global");
    assert!(response["data"]["bad"]["doc"].is_null());
    assert_eq!(response["data"]["good"]["document"]["schemaVersion"], 2);
    assert_eq!(response["errors"][0]["path"], json!(["bad", "doc"]));
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "STRUCTURE_FAILED"
    );
}

#[tokio::test]
async fn repeated_image_enrichment_is_charged_to_the_aggregate_body_limit() {
    let root = tempfile::tempdir().unwrap();
    let destination = format!("/assets/photo.png?query={}", "a".repeat(4000));
    article(
        root.path(),
        1,
        "Enrichment",
        &[],
        &format!("![alt]({destination})\n\n").repeat(120),
    );
    images::asset(root.path(), "photo.png", &images::png(640, 480, false));
    let (_, renderer, router) = app(root.path()).await;
    let response = query(&router, "{post(id:\"1\"){title plain:document{schemaVersion} rich:document{nodes{... on ImageNode{presentation{url candidates{url width}}}}}}}", json!({})).await;
    assert_eq!(response["data"]["post"]["title"], "Enrichment");
    assert_eq!(response["data"]["post"]["plain"]["schemaVersion"], 2);
    assert!(response["data"]["post"]["rich"].is_null());
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "STRUCTURE_FAILED"
    );
    assert_eq!(renderer.parses(), 1);
    assert_eq!(renderer.images.inspections(), 1);
    assert_eq!(renderer.images.transforms(), 0);
}
