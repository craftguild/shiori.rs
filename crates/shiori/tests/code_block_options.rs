mod common;
use common::*;

#[tokio::test]
async fn code_source_and_tokens_are_literal_and_presentation_remains_consumer_owned() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Code",
        &[],
        "```rust\nlet text = \"<script>&\";\n```\n\n    echo '<script>&'\n",
    );
    let (_, renderer, router) = app(root.path()).await;
    let query = "{post(id:\"1\"){document{nodes{__typename ... on FencedCodeNode{source language highlighting{lines{tokens{kind text}}}} ... on IndentedCodeNode{source language highlighting{lines{tokens{kind text}}}}}}}}";
    let first = data(&router, query).await;
    let nodes = first["post"]["document"]["nodes"].as_array().unwrap();
    let code: Vec<_> = nodes
        .iter()
        .filter(|node| node["source"].is_string())
        .collect();
    assert_eq!(code.len(), 2);
    for node in code {
        let text: Vec<String> = node["highlighting"]["lines"]
            .as_array()
            .unwrap()
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
        assert_eq!(text.join("\n"), node["source"].as_str().unwrap());
        assert!(text.join("\n").contains("<script>&"));
    }
    assert_eq!(renderer.calls(), 1);
    assert_eq!(first, data(&router, query).await);
    assert_eq!(renderer.calls(), 1);
}
