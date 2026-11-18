use super::*;
object!(DocumentResolver, Arc<Document>, parent;
    schema_version: i32 => parent.data.schema_version as i32;
    root_ids: Vec<Id> => ids(&parent.data.root_ids);
    reference_ids: Vec<Id> => ids(&parent.data.reference_ids);
    footnote_ids: Vec<Id> => ids(&parent.data.footnote_ids);
    nodes: Vec<generated::DocumentNode<Types>> => parent.data.nodes.iter().enumerate().map(|(index, node)| {
        let value = Node { document: parent.clone(), index };
        // A new producer variant requires an explicit concrete GraphQL projection.
        match &node.content {
            N::Document { .. } => generated::DocumentNode::DocumentRootNode(value),
            N::Paragraph { .. } => generated::DocumentNode::ParagraphNode(value),
            N::Heading { .. } => generated::DocumentNode::HeadingNode(value),
            N::FencedCode { .. } => generated::DocumentNode::FencedCodeNode(value),
            N::IndentedCode { .. } => generated::DocumentNode::IndentedCodeNode(value),
            N::MathBlock { .. } => generated::DocumentNode::MathBlockNode(value),
            N::ThematicBreak { .. } => generated::DocumentNode::ThematicBreakNode(value),
            N::HtmlBlock { .. } => generated::DocumentNode::HtmlBlockNode(value),
            N::LinkReferenceDefinition { .. } => generated::DocumentNode::LinkReferenceDefinitionNode(value),
            N::BlockQuote { .. } => generated::DocumentNode::BlockQuoteNode(value),
            N::List { .. } => generated::DocumentNode::ListNode(value),
            N::ListItem { .. } => generated::DocumentNode::ListItemNode(value),
            N::GfmTable { .. } => generated::DocumentNode::GfmTableNode(value),
            N::TableRow { .. } => generated::DocumentNode::TableRowNode(value),
            N::TableCell { .. } => generated::DocumentNode::TableCellNode(value),
            N::BlankLine { .. } => generated::DocumentNode::BlankLineNode(value),
            N::Callout { .. } => generated::DocumentNode::CalloutNode(value),
            N::ForeignHtml { .. } => generated::DocumentNode::ForeignHtmlNode(value),
            N::Text { .. } => generated::DocumentNode::TextNode(value),
            N::Emoji { .. } => generated::DocumentNode::EmojiNode(value),
            N::Code { .. } => generated::DocumentNode::CodeNode(value),
            N::Math { .. } => generated::DocumentNode::MathNode(value),
            N::Html { .. } => generated::DocumentNode::HtmlNode(value),
            N::SoftBreak { .. } => generated::DocumentNode::SoftBreakNode(value),
            N::HardBreak { .. } => generated::DocumentNode::HardBreakNode(value),
            N::Annotation { .. } => generated::DocumentNode::AnnotationNode(value),
            N::Emphasis { .. } => generated::DocumentNode::EmphasisNode(value),
            N::Strong { .. } => generated::DocumentNode::StrongNode(value),
            N::Strikethrough { .. } => generated::DocumentNode::StrikethroughNode(value),
            N::Link { .. } => generated::DocumentNode::LinkNode(value),
            N::Image { .. } => generated::DocumentNode::ImageNode(value),
            N::FootnoteReference { .. } => generated::DocumentNode::FootnoteReferenceNode(value),
            N::ReferenceDefinition { .. } => generated::DocumentNode::ReferenceDefinitionNode(value),
            N::FootnoteDefinition { .. } => generated::DocumentNode::FootnoteDefinitionNode(value),
        }
    }).collect();
    fonts: Vec<Font> => (0..parent.data.fonts.len()).map(|index| Font { document: parent.clone(), index }).collect();
    diagnostics: Vec<Diagnostic> => (0..parent.data.diagnostics.len()).map(|index| Diagnostic { document: parent.clone(), index }).collect();
);
