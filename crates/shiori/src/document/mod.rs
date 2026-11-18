//! The producer document is retained directly; image presentation belongs to shiori.
mod budget;
mod retained;

use crate::{images::Dependency, rendering::RenderFailure};
use oriha_core::{BlockOutputStatus, StructuredBlockOutput};
use std::{collections::HashMap, sync::Arc};

pub const MAX_NODES: usize = 100_000;
pub const MAX_DEPTH: usize = 512;
pub const MAX_BYTES: usize = 2_000_000;

#[derive(Clone, Debug)]
pub struct Document {
    pub data: StructuredBlockOutput,
    pub images: HashMap<String, Arc<Image>>,
    pub(crate) dependencies: Vec<Dependency>,
    pub(crate) cacheable: bool,
}

impl Document {
    pub fn new(data: StructuredBlockOutput) -> Result<Self, RenderFailure> {
        if data.status != BlockOutputStatus::Complete {
            return Err(RenderFailure);
        }
        data.validate(MAX_NODES, MAX_DEPTH, MAX_BYTES)
            .map_err(|_| RenderFailure)?;
        Ok(Self {
            data,
            images: HashMap::new(),
            dependencies: Vec::new(),
            cacheable: true,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub document: Arc<Document>,
    pub index: usize,
}
impl Node {
    pub fn value(&self) -> &oriha_core::DocumentNode {
        &self.document.data.nodes[self.index]
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub url: String,
    pub original_url: String,
    pub candidates: Vec<Candidate>,
    pub sizes: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Candidate {
    pub url: String,
    pub width: u32,
}
#[derive(Clone, Debug)]
pub struct AnnotationEntry {
    pub key: String,
    pub value: String,
}
