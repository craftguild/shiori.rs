mod common;
use common::*;

#[tokio::test]
async fn blog_keeps_language_and_line_numbers_without_producing_copy_ui() {
    let root = tempfile::tempdir().unwrap();
    article(
        root.path(),
        1,
        "Code",
        &[],
        "```rust\nlet text = \"<script>&\";\n```\n\n    echo '<script>&'\n",
    );
    let (_, renderer, router) = app(root.path()).await;
    let query = "{post(id:\"1\"){blocks{id kind html}}}";
    let first = data(&router, query).await;
    let blocks = first["post"]["blocks"].as_array().unwrap();
    let code: Vec<_> = blocks
        .iter()
        .filter_map(|block| {
            let html = block["html"].as_str().unwrap();
            html.contains("oriha-code-block-wrapper").then_some(html)
        })
        .collect();
    assert_eq!(code.len(), 2);
    for html in code {
        assert!(html.contains("oriha-code-block-language"));
        assert!(html.contains("oriha-code-line-number"));
        assert!(!html.contains("oriha-code-copy"));
        assert!(!html.contains("<button"));
        assert!(html.contains("&lt;script&gt;&amp;"));
        assert!(!html.contains("<script>"));
    }
    assert_eq!(renderer.calls(), 1);
    assert_eq!(first, data(&router, query).await);
    assert_eq!(renderer.calls(), 1);
}
