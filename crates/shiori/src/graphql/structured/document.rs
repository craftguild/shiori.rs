use super::*;
use oriha_core::{RichTextProperties as I, StructuredBlockProperties as B};
object!(StructuredDocumentResolver, Arc<Document>, parent;
    schema_version: i32 => parent.schema_version as i32;
    root_ids: Vec<Option<Id>> => ids(&parent.root_ids);
    blocks: Vec<generated::StructuredBlock<Types>> => parent.blocks.iter().map(|node| match &node.content {
        B::Document { .. } => generated::StructuredBlock::BlockDocument(node.clone()),
        B::Paragraph { .. } => generated::StructuredBlock::BlockParagraph(node.clone()),
        B::Heading { .. } => generated::StructuredBlock::BlockHeading(node.clone()),
        B::FencedCode { .. } => generated::StructuredBlock::BlockFencedCode(node.clone()),
        B::IndentedCode { .. } => generated::StructuredBlock::BlockIndentedCode(node.clone()),
        B::MathBlock { .. } => generated::StructuredBlock::BlockMathBlock(node.clone()),
        B::ThematicBreak { .. } => generated::StructuredBlock::BlockThematicBreak(node.clone()),
        B::HtmlBlock { .. } => generated::StructuredBlock::BlockHtmlBlock(node.clone()),
        B::LinkReferenceDefinition { .. } => generated::StructuredBlock::BlockLinkReferenceDefinition(node.clone()),
        B::BlockQuote { .. } => generated::StructuredBlock::BlockBlockQuote(node.clone()),
        B::List { .. } => generated::StructuredBlock::BlockList(node.clone()),
        B::ListItem { .. } => generated::StructuredBlock::BlockListItem(node.clone()),
        B::GfmTable { .. } => generated::StructuredBlock::BlockGfmTable(node.clone()),
        B::BlankLine { .. } => generated::StructuredBlock::BlockBlankLine(node.clone()),
        B::Callout { .. } => generated::StructuredBlock::BlockCallout(node.clone()),
        B::ForeignHtml { .. } => generated::StructuredBlock::BlockForeignHtml(node.clone()),
    }).collect();
    inlines: Vec<generated::StructuredInline<Types>> => parent.inlines.iter().map(|node| match &node.content {
        I::Annotation { .. } => generated::StructuredInline::InlineAnnotation(node.clone()),
        I::Text { .. } => generated::StructuredInline::InlineText(node.clone()),
        I::Emoji { .. } => generated::StructuredInline::InlineEmoji(node.clone()),
        I::Code { .. } => generated::StructuredInline::InlineCode(node.clone()),
        I::Math { .. } => generated::StructuredInline::InlineMath(node.clone()),
        I::Html { .. } => generated::StructuredInline::InlineHtml(node.clone()),
        I::SoftBreak { .. } => generated::StructuredInline::InlineSoftBreak(node.clone()),
        I::HardBreak { .. } => generated::StructuredInline::InlineHardBreak(node.clone()),
        I::Emphasis { .. } => generated::StructuredInline::InlineEmphasis(node.clone()),
        I::Strong { .. } => generated::StructuredInline::InlineStrong(node.clone()),
        I::Strikethrough { .. } => generated::StructuredInline::InlineStrikethrough(node.clone()),
        I::Link { .. } => generated::StructuredInline::InlineLink(node.clone()),
        I::Image { .. } => generated::StructuredInline::InlineImage(node.clone()),
        I::FootnoteReference { .. } => generated::StructuredInline::InlineFootnoteReference(node.clone()),
    }).collect();
    references: Vec<Reference> => parent.references.clone();
    footnotes: Vec<Footnote> => parent.footnotes.clone();
    anchors: Vec<String> => parent.anchors.clone();
);
object!(StructuredTableCellResolver, Cell, parent;
    text: String => parent.text.clone();
    rich_text_ids: Vec<Option<Id>> => ids(&parent.rich_text_ids);
);
object!(StructuredTableRowResolver, Row, parent;
    cells: Vec<Cell> => parent.cells.clone();
);
object!(StructuredEntryResolver, Entry, parent;
    key: String => parent.key.clone();
    value: String => parent.value.clone();
);
object!(StructuredReferenceResolver, Reference, parent;
    label: String => parent.label.clone();
    destination: String => parent.destination.clone();
    title: Option<String> => parent.title.clone();
);
object!(StructuredFootnoteResolver, Footnote, parent;
    label: String => parent.label.clone();
    rich_text_ids: Vec<Option<Id>> => ids(&parent.rich_text_ids);
);
