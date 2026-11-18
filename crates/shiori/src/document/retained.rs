//! Retained capacities are charged to the existing bounded derivation cache.
use super::*;
use oriha_core::{CodeHighlighting, DocumentNodeKind as N, SvgContent};
use std::mem::size_of;

trait Heap {
    fn heap(&self) -> usize;
}
impl Heap for String {
    fn heap(&self) -> usize {
        self.capacity()
    }
}
impl<T: Heap> Heap for Vec<T> {
    fn heap(&self) -> usize {
        self.capacity() * size_of::<T>() + self.iter().map(Heap::heap).sum::<usize>()
    }
}
impl<T: Heap> Heap for Option<T> {
    fn heap(&self) -> usize {
        self.as_ref().map_or(0, Heap::heap)
    }
}
impl Heap for CodeHighlighting {
    fn heap(&self) -> usize {
        self.lines.capacity() * size_of::<oriha_core::CodeLine>()
            + self
                .lines
                .iter()
                .map(|line| {
                    line.tokens.capacity() * size_of::<oriha_core::SyntaxToken>()
                        + line
                            .tokens
                            .iter()
                            .map(|token| token.text.capacity())
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
}
impl Heap for SvgContent {
    fn heap(&self) -> usize {
        self.markup.heap() + self.font_ids.heap()
    }
}
impl Heap for oriha_core::DocumentNode {
    fn heap(&self) -> usize {
        let bytes = match &self.content {
            N::Document { child_ids }
            | N::BlockQuote { child_ids }
            | N::TableCell { child_ids }
            | N::Emphasis { child_ids }
            | N::Strong { child_ids }
            | N::Strikethrough { child_ids }
            | N::Paragraph { child_ids, .. } => child_ids.heap(),
            N::Heading {
                child_ids, anchor, ..
            } => child_ids.heap() + anchor.heap(),
            N::FencedCode {
                source,
                language,
                info,
                highlighting,
                svg,
                ..
            } => source.heap() + language.heap() + info.heap() + highlighting.heap() + svg.heap(),
            N::IndentedCode {
                source,
                language,
                info,
                highlighting,
                ..
            } => source.heap() + language.heap() + info.heap() + highlighting.heap(),
            N::Math { source, svg } | N::MathBlock { source, svg, .. } => {
                source.heap() + svg.heap()
            }
            N::HtmlBlock { source }
            | N::LinkReferenceDefinition { source }
            | N::Code { source }
            | N::Html { source, .. } => source.heap(),
            N::Text { text } => text.heap(),
            N::Emoji { text, shortcode } => text.heap() + shortcode.heap(),
            N::List {
                child_ids,
                marker,
                start,
                ..
            } => child_ids.heap() + marker.heap() + start.heap(),
            N::ListItem {
                child_ids, marker, ..
            } => child_ids.heap() + marker.heap(),
            N::GfmTable {
                row_ids,
                alignments,
            } => {
                row_ids.heap()
                    + alignments.capacity() * size_of::<oriha_core::BlockTableAlignment>()
            }
            N::TableRow { cell_ids, .. } => cell_ids.heap(),
            N::Callout {
                child_ids,
                callout_kind,
                title,
            } => child_ids.heap() + callout_kind.heap() + title.heap(),
            N::ForeignHtml { source, origin, .. } => source.heap() + origin.heap(),
            N::Annotation {
                child_ids,
                annotation_kind,
                annotation_id,
                classes,
                data,
            } => {
                // A conservative per-entry bound includes B-tree nodes/edges,
                // including the mostly empty allocation for a one-entry map.
                child_ids.heap()
                    + annotation_kind.heap()
                    + annotation_id.heap()
                    + classes.heap()
                    + data.len() * 16 * size_of::<(String, String)>()
                    + data
                        .iter()
                        .map(|(key, value)| key.heap() + value.heap())
                        .sum::<usize>()
            }
            N::Link {
                child_ids,
                destination,
                normalized_destination,
                title,
            } => {
                child_ids.heap() + destination.heap() + normalized_destination.heap() + title.heap()
            }
            N::Image {
                alt_ids,
                destination,
                normalized_destination,
                title,
            } => alt_ids.heap() + destination.heap() + normalized_destination.heap() + title.heap(),
            N::FootnoteReference {
                label,
                definition_id,
                anchor,
                definition_anchor,
                ..
            } => label.heap() + definition_id.heap() + anchor.heap() + definition_anchor.heap(),
            N::ReferenceDefinition {
                label,
                destination,
                normalized_destination,
                title,
            } => label.heap() + destination.heap() + normalized_destination.heap() + title.heap(),
            N::FootnoteDefinition {
                label,
                child_ids,
                anchor,
                backlink_ids,
                ..
            } => label.heap() + child_ids.heap() + anchor.heap() + backlink_ids.heap(),
            N::ThematicBreak {} | N::BlankLine {} | N::SoftBreak {} | N::HardBreak {} => 0,
        };
        self.id.heap() + bytes
    }
}
impl Heap for Candidate {
    fn heap(&self) -> usize {
        self.url.heap()
    }
}
impl Image {
    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + 2 * size_of::<usize>()
            + self.url.heap()
            + self.original_url.heap()
            + self.candidates.heap()
            + self.sizes.heap()
    }
}
impl Document {
    pub fn retained_bytes(&self) -> usize {
        let data = &self.data;
        size_of::<Self>()
            + 2 * size_of::<usize>()
            + data.root_ids.heap()
            + data.nodes.heap()
            + data.reference_ids.heap()
            + data.footnote_ids.heap()
            + data.fonts.capacity() * size_of::<oriha_core::DocumentFont>()
            + data
                .fonts
                .iter()
                .map(|font| {
                    font.id.heap()
                        + font.family.heap()
                        + font.mime_type.heap()
                        + font.content.heap()
                })
                .sum::<usize>()
            + data.diagnostics.capacity() * size_of::<oriha_core::RenderDiagnostic>()
            + data
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    diagnostic.code.heap() + diagnostic.message.heap() + diagnostic.block_id.heap()
                })
                .sum::<usize>()
            + self.images.capacity() * (size_of::<(String, Arc<Image>)>() + 16)
            + self
                .images
                .iter()
                .map(|(key, image)| key.heap() + image.retained_bytes())
                .sum::<usize>()
            + self.dependencies.capacity() * size_of::<Dependency>()
            + self
                .dependencies
                .iter()
                .map(|dependency| dependency.retained_bytes() - size_of::<Dependency>())
                .sum::<usize>()
    }
}
