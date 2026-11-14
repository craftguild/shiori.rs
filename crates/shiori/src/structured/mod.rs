mod flatten;
mod retained;

use oriha_core::{RichTextProperties, StructuredBlockProperties};
use serde::Serialize;
use std::sync::Arc;

pub use flatten::project;
pub const MAX_NODES: usize = 100_000;

#[derive(Clone, Debug)]
pub struct Document {
    pub schema_version: u32,
    pub root_ids: Vec<Option<String>>,
    pub blocks: Vec<Arc<Block>>,
    pub inlines: Vec<Arc<Inline>>,
    pub references: Vec<Reference>,
    pub footnotes: Vec<Footnote>,
    pub anchors: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Block {
    pub id: String,
    pub text: Option<String>,
    pub child_ids: Vec<Option<String>>,
    // Rich text and table cells are moved into flat pools, never duplicated here.
    pub content: StructuredBlockProperties,
    pub rich_text_ids: Vec<Option<String>>,
    pub header: Vec<Cell>,
    pub rows: Vec<Row>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Inline {
    pub id: String,
    pub text: Option<String>,
    pub child_ids: Vec<Option<String>>,
    pub content: RichTextProperties,
    pub normalized_url: Option<String>,
    pub data: Vec<Entry>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    pub text: String,
    pub rich_text_ids: Vec<Option<String>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub cells: Vec<Cell>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub key: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Reference {
    pub label: String,
    pub destination: String,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Footnote {
    pub label: String,
    pub rich_text_ids: Vec<Option<String>>,
}

#[cfg(test)]
mod tests;
