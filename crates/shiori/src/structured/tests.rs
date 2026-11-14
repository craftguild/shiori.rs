use super::*;
use oriha_core::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn inline(kind: &str, properties: Value) -> RichText {
    serde_json::from_value(
        json!({"kind":kind,"properties":properties,"text":"value λ","children":[]}),
    )
    .unwrap()
}
fn output(blocks: Vec<Option<StructuredBlock>>) -> StructuredBlockOutput {
    StructuredBlockOutput {
        schema_version: 1,
        mode: BlockOutputMode::Structured,
        status: BlockOutputStatus::Complete,
        blocks,
        references: BTreeMap::from([(
            "ref".into(),
            StructuredReference {
                destination: "/target".into(),
                title: Some("title".into()),
            },
        )]),
        footnotes: vec![],
        diagnostics: vec![],
    }
}
fn restore_inline(doc: &Document, id: &Option<String>) -> Option<RichText> {
    let id = id.as_ref()?;
    let node = doc.inlines.iter().find(|n| &n.id == id).unwrap();
    let mut content = node.content.clone();
    if let RichTextProperties::Annotation { data, .. } = &mut content {
        *data = node
            .data
            .iter()
            .map(|p| (p.key.clone(), p.value.clone()))
            .collect();
    }
    Some(RichText {
        text: node.text.clone(),
        content,
        children: node
            .child_ids
            .iter()
            .map(|id| restore_inline(doc, id))
            .collect(),
    })
}
fn restore_cell(doc: &Document, cell: &Cell) -> StructuredTableCell {
    StructuredTableCell {
        text: cell.text.clone(),
        rich_text: cell
            .rich_text_ids
            .iter()
            .map(|id| restore_inline(doc, id))
            .collect(),
    }
}
fn restore_block(doc: &Document, id: &Option<String>) -> Option<StructuredBlock> {
    let id = id.as_ref()?;
    let node = doc.blocks.iter().find(|n| &n.id == id).unwrap();
    let mut content = node.content.clone();
    match &mut content {
        StructuredBlockProperties::Paragraph { rich_text, .. }
        | StructuredBlockProperties::Heading { rich_text, .. } => {
            *rich_text = node
                .rich_text_ids
                .iter()
                .map(|id| restore_inline(doc, id))
                .collect()
        }
        StructuredBlockProperties::GfmTable { header, rows, .. } => {
            *header = node.header.iter().map(|c| restore_cell(doc, c)).collect();
            *rows = node
                .rows
                .iter()
                .map(|r| r.cells.iter().map(|c| restore_cell(doc, c)).collect())
                .collect();
        }
        _ => {}
    }
    Some(StructuredBlock {
        id: node.id.clone(),
        text: node.text.clone(),
        content,
        children: node
            .child_ids
            .iter()
            .map(|id| restore_block(doc, id))
            .collect(),
    })
}

#[test]
fn every_variant_property_nullable_slot_and_order_round_trips_losslessly() {
    let inlines = [
        (
            "annotation",
            json!({"kind":"mark","id":"ann","classes":["a","b"],"data":{"a":"λ","z":"last"}}),
        ),
        ("text", json!({})),
        ("emoji", json!({"shortcode":"smile"})),
        ("code", json!({})),
        ("math", json!({})),
        ("html", json!({"tagfilter":true})),
        ("softBreak", json!({})),
        ("hardBreak", json!({})),
        ("emphasis", json!({})),
        ("strong", json!({})),
        ("strikethrough", json!({})),
        ("link", json!({"url":"/日本語 a","title":"title"})),
        ("image", json!({"url":"/image","title":null})),
        ("footnoteReference", json!({"label":"note"})),
    ]
    .into_iter()
    .map(|(k, p)| Some(inline(k, p)))
    .chain([None])
    .collect::<Vec<_>>();
    let cell = StructuredTableCell {
        text: "cell".into(),
        rich_text: inlines.clone(),
    };
    let variants = vec![
        StructuredBlockProperties::Document {},
        StructuredBlockProperties::Paragraph {
            rich_text: inlines.clone(),
            tight: true,
        },
        StructuredBlockProperties::Heading {
            level: 3,
            rich_text: inlines.clone(),
        },
        StructuredBlockProperties::FencedCode {
            language: Some("rust".into()),
            info: "rust more".into(),
            closed: true,
            indent: 3,
        },
        StructuredBlockProperties::IndentedCode {
            language: None,
            info: "".into(),
            closed: false,
            indent: 4,
        },
        StructuredBlockProperties::MathBlock {
            display: true,
            closed: false,
        },
        StructuredBlockProperties::ThematicBreak {},
        StructuredBlockProperties::HtmlBlock {},
        StructuredBlockProperties::LinkReferenceDefinition {
            source: "[x]: /url".into(),
        },
        StructuredBlockProperties::BlockQuote {},
        StructuredBlockProperties::List {
            ordered: true,
            loose: true,
            marker: ".".into(),
            start: Some("99999999999999999999".into()),
        },
        StructuredBlockProperties::ListItem {
            marker: "-".into(),
            checked: Some(false),
        },
        StructuredBlockProperties::GfmTable {
            alignments: vec![
                BlockTableAlignment::None,
                BlockTableAlignment::Left,
                BlockTableAlignment::Center,
                BlockTableAlignment::Right,
            ],
            header: vec![cell.clone()],
            rows: vec![vec![], vec![cell]],
        },
        StructuredBlockProperties::BlankLine {},
        StructuredBlockProperties::Callout {
            kind: "custom".into(),
            title: Some("Title".into()),
        },
        StructuredBlockProperties::ForeignHtml {
            origin: "app".into(),
            trust_level: BlockTrustLevel::Sanitized,
        },
    ];
    let mut source = output(
        variants
            .into_iter()
            .enumerate()
            .map(|(i, content)| {
                Some(StructuredBlock {
                    id: format!("b{i}"),
                    text: Some("aggregate".into()),
                    content,
                    children: vec![None],
                })
            })
            .chain([None])
            .collect(),
    );
    source.footnotes.push(StructuredFootnote {
        label: "n".into(),
        rich_text: inlines,
    });
    let flat = project(source.clone(), vec!["canonical".into()]).unwrap();
    assert_eq!(flat.root_ids.len(), source.blocks.len());
    assert_eq!(flat.inlines[0].id, "i0");
    let blocks: Vec<_> = flat
        .root_ids
        .iter()
        .map(|id| restore_block(&flat, id))
        .collect();
    assert_eq!(
        serde_json::to_value(blocks).unwrap(),
        serde_json::to_value(source.blocks).unwrap()
    );
    assert_eq!(flat.references[0].label, "ref");
    assert_eq!(flat.references[0].destination, "/target");
    assert_eq!(flat.footnotes[0].label, "n");
    assert_eq!(
        serde_json::to_value(
            flat.footnotes[0]
                .rich_text_ids
                .iter()
                .map(|id| restore_inline(&flat, id))
                .collect::<Vec<_>>()
        )
        .unwrap(),
        serde_json::to_value(source.footnotes[0].rich_text.clone()).unwrap()
    );
    assert_eq!(flat.anchors, vec!["canonical"]);
    let link = flat
        .inlines
        .iter()
        .find(|n| matches!(n.content, RichTextProperties::Link { .. }))
        .unwrap();
    assert_eq!(
        link.normalized_url.as_deref(),
        Some("/%E6%97%A5%E6%9C%AC%E8%AA%9E%20a")
    );
}

#[test]
fn exact_combined_node_boundary_fails_instead_of_truncating_and_counts_capacity() {
    for (count, ok) in [(MAX_NODES - 1, true), (MAX_NODES, false)] {
        let rich_text = (0..count)
            .map(|_| Some(inline("text", json!({}))))
            .collect();
        let source = output(vec![Some(StructuredBlock {
            id: "b".into(),
            text: None,
            content: StructuredBlockProperties::Paragraph {
                rich_text,
                tight: false,
            },
            children: vec![],
        })]);
        let flat = project(source, vec![]);
        assert_eq!(flat.is_ok(), ok);
        if let Ok(flat) = flat {
            assert_eq!(flat.blocks.len() + flat.inlines.len(), MAX_NODES);
        }
    }
    let mut empty = (*project(output(vec![]), vec![]).unwrap()).clone();
    let before = empty.retained_bytes();
    let mut reserved = String::with_capacity(8192);
    reserved.push('x');
    empty.anchors.push(reserved);
    assert!(empty.retained_bytes() >= before + 8192);
}
