mod cursor;
mod document;
mod objects;
mod post;
mod query;
mod subscription;

use std::sync::Arc;

use chigiri::{
    BoxFuture, ContextFactory, Endpoint, FieldError, HttpRequestContext, PrepareError,
    PreparedContext,
};

use crate::{
    catalog::{Catalog, Snapshot},
    metadata::Article,
    rendering::{Renderer, RequestMemo},
};

pub struct Context {
    pub catalog: Arc<Catalog>,
    pub snapshot: Arc<Snapshot>,
    pub renderer: Arc<Renderer>,
    pub memo: RequestMemo,
}

struct Factory {
    catalog: Arc<Catalog>,
    renderer: Arc<Renderer>,
}
impl ContextFactory<Context, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<Context, ()>, PrepareError>> {
        Box::pin(async move {
            Ok(PreparedContext::execution_only(Context {
                catalog: self.catalog.clone(),
                snapshot: self.catalog.snapshot(),
                renderer: self.renderer.clone(),
                memo: RequestMemo::default(),
            }))
        })
    }
}

pub struct Types;
impl generated::SchemaTypes for Types {
    type Document = Arc<crate::document::Document>;
    type DocumentRootNode = crate::document::Node;
    type ParagraphNode = crate::document::Node;
    type HeadingNode = crate::document::Node;
    type FencedCodeNode = crate::document::Node;
    type IndentedCodeNode = crate::document::Node;
    type MathBlockNode = crate::document::Node;
    type ThematicBreakNode = crate::document::Node;
    type HtmlBlockNode = crate::document::Node;
    type LinkReferenceDefinitionNode = crate::document::Node;
    type BlockQuoteNode = crate::document::Node;
    type ListNode = crate::document::Node;
    type ListItemNode = crate::document::Node;
    type GfmTableNode = crate::document::Node;
    type TableRowNode = crate::document::Node;
    type TableCellNode = crate::document::Node;
    type BlankLineNode = crate::document::Node;
    type CalloutNode = crate::document::Node;
    type ForeignHtmlNode = crate::document::Node;
    type TextNode = crate::document::Node;
    type EmojiNode = crate::document::Node;
    type CodeNode = crate::document::Node;
    type MathNode = crate::document::Node;
    type HtmlNode = crate::document::Node;
    type SoftBreakNode = crate::document::Node;
    type HardBreakNode = crate::document::Node;
    type AnnotationNode = crate::document::Node;
    type EmphasisNode = crate::document::Node;
    type StrongNode = crate::document::Node;
    type StrikethroughNode = crate::document::Node;
    type LinkNode = crate::document::Node;
    type ImageNode = crate::document::Node;
    type FootnoteReferenceNode = crate::document::Node;
    type ReferenceDefinitionNode = crate::document::Node;
    type FootnoteDefinitionNode = crate::document::Node;
    type CodeHighlighting = crate::document::Node;
    type CodeLine = document::Line;
    type SyntaxToken = document::Token;
    type SvgContent = crate::document::Node;
    type DocumentFont = document::Font;
    type DocumentDiagnostic = document::Diagnostic;
    type DocumentSourceRange = oriha_core::SourceRange;
    type AnnotationEntry = crate::document::AnnotationEntry;
    type ImagePresentation = Arc<crate::document::Image>;
    type ImageCandidate = crate::document::Candidate;
    type Post = Arc<Article>;
    type PostConnection = Connection;
    type PageInfo = PageInfo;
    type Tag = Tag;
    type MonthlyArchive = MonthlyArchive;
    type ContentChange = crate::preview::ContentChange;
}

#[derive(Clone)]
pub struct Connection {
    pub nodes: Vec<Arc<Article>>,
    pub total: i32,
    pub page: PageInfo,
}
#[derive(Clone)]
pub struct PageInfo {
    pub start: Option<chigiri::Id>,
    pub has_previous: bool,
    pub end: Option<chigiri::Id>,
    pub has_next: bool,
}
pub struct Tag {
    pub name: String,
    pub count: i32,
}

pub struct MonthlyArchive {
    pub year: i32,
    pub month: i32,
    pub count: i32,
}

#[derive(Debug)]
pub enum Error {
    Input(&'static str),
    Render,
    Structure,
    WatchDisabled,
    Search,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => f.write_str(message),
            Self::Structure => f.write_str("article structure failed"),
            Self::Render => f.write_str("article rendering failed"),
            Self::WatchDisabled => f.write_str("live preview is disabled"),
            Self::Search => f.write_str("article search failed"),
        }
    }
}
impl std::error::Error for Error {}
fn public_error(error: Error) -> FieldError {
    match error {
        Error::Input(message) => FieldError::public(message).code("BAD_USER_INPUT"),
        Error::Structure => FieldError::public("Article structure failed").code("STRUCTURE_FAILED"),
        Error::Render => FieldError::public("Article rendering failed").code("RENDER_FAILED"),
        Error::Search => FieldError::public("Article search failed").code("SEARCH_FAILED"),
        Error::WatchDisabled => {
            FieldError::public("Live preview is disabled").code("WATCH_DISABLED")
        }
    }
}

pub fn endpoint(
    catalog: Arc<Catalog>,
    renderer: Arc<Renderer>,
) -> anyhow::Result<Endpoint<Context>> {
    endpoint_with_graphiql(catalog, renderer, false)
}

pub(crate) fn endpoint_with_graphiql(
    catalog: Arc<Catalog>,
    renderer: Arc<Renderer>,
    graphiql: bool,
) -> anyhow::Result<Endpoint<Context>> {
    let resolvers = generated::Resolvers::<Context, Types>::builder(public_error)
        .with_query(query::QueryFields)
        .with_subscription(subscription::SubscriptionFields)
        .with_content_change(subscription::ChangeFields)
        .with_post(post::PostFields)
        .with_post_connection(objects::ConnectionFields)
        .with_page_info(objects::PageFields)
        .with_tag(objects::TagFields)
        .with_monthly_archive(objects::ArchiveFields)
        .with_document(document::Fields)
        .with_document_root_node(document::Fields)
        .with_paragraph_node(document::Fields)
        .with_heading_node(document::Fields)
        .with_fenced_code_node(document::Fields)
        .with_indented_code_node(document::Fields)
        .with_math_block_node(document::Fields)
        .with_thematic_break_node(document::Fields)
        .with_html_block_node(document::Fields)
        .with_link_reference_definition_node(document::Fields)
        .with_block_quote_node(document::Fields)
        .with_list_node(document::Fields)
        .with_list_item_node(document::Fields)
        .with_gfm_table_node(document::Fields)
        .with_table_row_node(document::Fields)
        .with_table_cell_node(document::Fields)
        .with_blank_line_node(document::Fields)
        .with_callout_node(document::Fields)
        .with_foreign_html_node(document::Fields)
        .with_text_node(document::Fields)
        .with_emoji_node(document::Fields)
        .with_code_node(document::Fields)
        .with_math_node(document::Fields)
        .with_html_node(document::Fields)
        .with_soft_break_node(document::Fields)
        .with_hard_break_node(document::Fields)
        .with_annotation_node(document::Fields)
        .with_emphasis_node(document::Fields)
        .with_strong_node(document::Fields)
        .with_strikethrough_node(document::Fields)
        .with_link_node(document::Fields)
        .with_image_node(document::Fields)
        .with_footnote_reference_node(document::Fields)
        .with_reference_definition_node(document::Fields)
        .with_footnote_definition_node(document::Fields)
        .with_code_highlighting(document::Fields)
        .with_code_line(document::Fields)
        .with_syntax_token(document::Fields)
        .with_svg_content(document::Fields)
        .with_document_font(document::Fields)
        .with_document_diagnostic(document::Fields)
        .with_document_source_range(document::Fields)
        .with_annotation_entry(document::Fields)
        .with_image_presentation(document::Fields)
        .with_image_candidate(document::Fields)
        .build();
    let limits = chigiri::ExecutionLimits {
        max_list_items_per_field: 100_000,
        max_output_nodes: 1_000_000,
        ..Default::default()
    };
    Ok(Endpoint::builder(resolvers)
        .graphiql(graphiql)
        .context_factory(Factory { catalog, renderer })
        .execution_limits(limits)
        .body_limit(256 * 1024)
        .response_byte_limit(8 * 1024 * 1024)
        .execution_timeout(std::time::Duration::from_secs(30))
        .build()?)
}
