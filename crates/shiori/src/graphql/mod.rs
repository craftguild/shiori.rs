mod cursor;
mod objects;
mod post;
mod query;
mod structured;
mod subscription;

use std::sync::Arc;

use chigiri::{
    BoxFuture, ContextFactory, Endpoint, FieldError, HttpRequestContext, PrepareError,
    PreparedContext,
};

use crate::{
    catalog::{Catalog, Snapshot},
    metadata::Article,
    rendering::{Asset, HtmlBlock, Renderer, RequestMemo},
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
    type StructuredDocument = Arc<crate::structured::Document>;
    type BlockDocument = Arc<crate::structured::Block>;
    type BlockParagraph = Arc<crate::structured::Block>;
    type BlockHeading = Arc<crate::structured::Block>;
    type BlockFencedCode = Arc<crate::structured::Block>;
    type BlockIndentedCode = Arc<crate::structured::Block>;
    type BlockMathBlock = Arc<crate::structured::Block>;
    type BlockThematicBreak = Arc<crate::structured::Block>;
    type BlockHtmlBlock = Arc<crate::structured::Block>;
    type BlockLinkReferenceDefinition = Arc<crate::structured::Block>;
    type BlockBlockQuote = Arc<crate::structured::Block>;
    type BlockList = Arc<crate::structured::Block>;
    type BlockListItem = Arc<crate::structured::Block>;
    type BlockGfmTable = Arc<crate::structured::Block>;
    type BlockBlankLine = Arc<crate::structured::Block>;
    type BlockCallout = Arc<crate::structured::Block>;
    type BlockForeignHtml = Arc<crate::structured::Block>;
    type InlineAnnotation = Arc<crate::structured::Inline>;
    type InlineText = Arc<crate::structured::Inline>;
    type InlineEmoji = Arc<crate::structured::Inline>;
    type InlineCode = Arc<crate::structured::Inline>;
    type InlineMath = Arc<crate::structured::Inline>;
    type InlineHtml = Arc<crate::structured::Inline>;
    type InlineSoftBreak = Arc<crate::structured::Inline>;
    type InlineHardBreak = Arc<crate::structured::Inline>;
    type InlineEmphasis = Arc<crate::structured::Inline>;
    type InlineStrong = Arc<crate::structured::Inline>;
    type InlineStrikethrough = Arc<crate::structured::Inline>;
    type InlineLink = Arc<crate::structured::Inline>;
    type InlineImage = Arc<crate::structured::Inline>;
    type InlineFootnoteReference = Arc<crate::structured::Inline>;
    type StructuredTableCell = crate::structured::Cell;
    type StructuredTableRow = crate::structured::Row;
    type StructuredEntry = crate::structured::Entry;
    type StructuredReference = crate::structured::Reference;
    type StructuredFootnote = crate::structured::Footnote;
    type Post = Arc<Article>;
    type HtmlBlock = HtmlBlock;
    type RenderAsset = Asset;
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
        .with_html_block(objects::BlockFields)
        .with_render_asset(objects::AssetFields)
        .with_post_connection(objects::ConnectionFields)
        .with_page_info(objects::PageFields)
        .with_tag(objects::TagFields)
        .with_monthly_archive(objects::ArchiveFields)
        .with_structured_document(structured::Fields)
        .with_structured_table_cell(structured::Fields)
        .with_structured_table_row(structured::Fields)
        .with_structured_entry(structured::Fields)
        .with_structured_reference(structured::Fields)
        .with_structured_footnote(structured::Fields)
        .with_block_document(structured::Fields)
        .with_block_paragraph(structured::Fields)
        .with_block_heading(structured::Fields)
        .with_block_fenced_code(structured::Fields)
        .with_block_indented_code(structured::Fields)
        .with_block_math_block(structured::Fields)
        .with_block_thematic_break(structured::Fields)
        .with_block_html_block(structured::Fields)
        .with_block_link_reference_definition(structured::Fields)
        .with_block_block_quote(structured::Fields)
        .with_block_list(structured::Fields)
        .with_block_list_item(structured::Fields)
        .with_block_gfm_table(structured::Fields)
        .with_block_blank_line(structured::Fields)
        .with_block_callout(structured::Fields)
        .with_block_foreign_html(structured::Fields)
        .with_inline_annotation(structured::Fields)
        .with_inline_text(structured::Fields)
        .with_inline_emoji(structured::Fields)
        .with_inline_code(structured::Fields)
        .with_inline_math(structured::Fields)
        .with_inline_html(structured::Fields)
        .with_inline_soft_break(structured::Fields)
        .with_inline_hard_break(structured::Fields)
        .with_inline_emphasis(structured::Fields)
        .with_inline_strong(structured::Fields)
        .with_inline_strikethrough(structured::Fields)
        .with_inline_link(structured::Fields)
        .with_inline_image(structured::Fields)
        .with_inline_footnote_reference(structured::Fields)
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
