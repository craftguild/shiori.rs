use super::*;
use crate::rendering::RenderFailure;
use oriha_core::render::normalize_link_destination;
use oriha_core::{BlockOutputStatus, RichText, StructuredBlockOutput, StructuredTableCell};

// Consumes the recursive producer DTO. Only the resulting flat graph is cached.
pub fn project(
    output: StructuredBlockOutput,
    anchors: Vec<String>,
) -> Result<Arc<Document>, RenderFailure> {
    if output.status != BlockOutputStatus::Complete || output.schema_version != 1 {
        return Err(RenderFailure);
    }
    let mut document = Document {
        schema_version: output.schema_version,
        root_ids: output
            .blocks
            .iter()
            .map(|b| b.as_ref().map(|b| b.id.clone()))
            .collect(),
        blocks: vec![],
        inlines: vec![],
        references: vec![],
        footnotes: vec![],
        anchors,
    };
    let mut pending: Vec<_> = output.blocks.into_iter().rev().flatten().collect();
    while let Some(mut node) = pending.pop() {
        document.check_node()?;
        let child_ids = node
            .children
            .iter()
            .map(|b| b.as_ref().map(|b| b.id.clone()))
            .collect();
        let mut rich_text_ids = vec![];
        let mut header = vec![];
        let mut rows = vec![];
        match &mut node.content {
            StructuredBlockProperties::Paragraph { rich_text, .. }
            | StructuredBlockProperties::Heading { rich_text, .. } => {
                rich_text_ids = document.inlines(std::mem::take(rich_text))?;
            }
            StructuredBlockProperties::GfmTable {
                header: source_header,
                rows: source_rows,
                ..
            } => {
                header = document.cells(std::mem::take(source_header))?;
                for cells in std::mem::take(source_rows) {
                    rows.push(Row {
                        cells: document.cells(cells)?,
                    });
                }
            }
            _ => {}
        }
        document.blocks.push(Arc::new(Block {
            id: node.id,
            text: node.text,
            child_ids,
            content: node.content,
            rich_text_ids,
            header,
            rows,
        }));
        pending.extend(node.children.into_iter().rev().flatten());
    }
    for (label, reference) in output.references {
        document.references.push(Reference {
            label,
            destination: reference.destination,
            title: reference.title,
        });
    }
    for footnote in output.footnotes {
        let rich_text_ids = document.inlines(footnote.rich_text)?;
        document.footnotes.push(Footnote {
            label: footnote.label,
            rich_text_ids,
        });
    }
    if document.blocks.len() + document.inlines.len() > MAX_NODES {
        return Err(RenderFailure);
    }
    Ok(Arc::new(document))
}
impl Document {
    fn check_node(&self) -> Result<(), RenderFailure> {
        if self.blocks.len() + self.inlines.len() >= MAX_NODES {
            Err(RenderFailure)
        } else {
            Ok(())
        }
    }
    fn cells(&mut self, source: Vec<StructuredTableCell>) -> Result<Vec<Cell>, RenderFailure> {
        source
            .into_iter()
            .map(|cell| {
                Ok(Cell {
                    text: cell.text,
                    rich_text_ids: self.inlines(cell.rich_text)?,
                })
            })
            .collect()
    }
    fn inlines(
        &mut self,
        source: Vec<Option<RichText>>,
    ) -> Result<Vec<Option<String>>, RenderFailure> {
        enum Work {
            Node(RichText, usize),
            Slot(Option<RichText>, Option<usize>, usize),
        }
        // A temporary adjacency vector is bounded by the producer output, then moved.
        let mut roots = vec![None; source.len()];
        let mut pending: Vec<_> = source
            .into_iter()
            .enumerate()
            .rev()
            .map(|(slot, node)| Work::Slot(node, None, slot))
            .collect();
        while let Some(work) = pending.pop() {
            match work {
                Work::Slot(None, _, _) => {}
                Work::Slot(Some(node), parent, slot) => {
                    self.check_node()?;
                    let index = self.inlines.len();
                    let id = format!("i{index}");
                    if let Some(parent) = parent {
                        Arc::get_mut(&mut self.inlines[parent])
                            .expect("private graph")
                            .child_ids[slot] = Some(id.clone());
                    } else {
                        roots[slot] = Some(id.clone());
                    }
                    self.inlines.push(Arc::new(Inline {
                        id,
                        text: None,
                        child_ids: vec![],
                        content: RichTextProperties::Text {},
                        normalized_url: None,
                        data: vec![],
                    }));
                    pending.push(Work::Node(node, index));
                }
                Work::Node(mut node, index) => {
                    let normalized_url = match &node.content {
                        RichTextProperties::Link { url, .. }
                        | RichTextProperties::Image { url, .. } => {
                            Some(normalize_link_destination(url))
                        }
                        _ => None,
                    };
                    let data = match &mut node.content {
                        RichTextProperties::Annotation { data, .. } => std::mem::take(data)
                            .into_iter()
                            .map(|(key, value)| Entry { key, value })
                            .collect(),
                        _ => vec![],
                    };
                    let child_ids = vec![None; node.children.len()];
                    let target = Arc::get_mut(&mut self.inlines[index]).expect("private graph");
                    target.text = node.text;
                    target.content = node.content;
                    target.normalized_url = normalized_url;
                    target.data = data;
                    target.child_ids = child_ids;
                    pending.extend(
                        node.children
                            .into_iter()
                            .enumerate()
                            .rev()
                            .map(|(slot, node)| Work::Slot(node, Some(index), slot)),
                    );
                }
            }
        }
        Ok(roots)
    }
}
