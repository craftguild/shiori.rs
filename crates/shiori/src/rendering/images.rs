//! Prepare typed image presentation once per selected document profile.
use super::RenderFailure;
use crate::{
    config::Config,
    images::{AssetPath, Dependency, ImageService, SourceState, WIDTHS},
};
use oriha_core::{
    ImageCandidate, ImageLayout, ImagePresentation,
    ir::{BlockKind, BlockNode, Document, InlineKind, InlineNode},
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
#[cfg(test)]
mod tests;

pub(super) struct Prepared {
    presentations: HashMap<String, ImagePresentation>,
    pub dependencies: Vec<Dependency>,
    pub cacheable: bool,
    originals: Vec<(String, String)>,
}
impl Prepared {
    pub fn project(&self, fallback: bool) -> HashMap<String, Arc<crate::document::Image>> {
        self.originals
            .iter()
            .map(|(destination, original)| {
                let prepared = (!fallback)
                    .then(|| self.presentations.get(destination))
                    .flatten();
                let dimensions = prepared.and_then(ImagePresentation::intrinsic_dimensions);
                let image = crate::document::Image {
                    url: prepared
                        .map_or_else(|| original.clone(), |value| value.source().to_owned()),
                    original_url: original.clone(),
                    candidates: prepared
                        .map(|value| {
                            value
                                .candidates()
                                .iter()
                                .map(|candidate| crate::document::Candidate {
                                    url: candidate.url().to_owned(),
                                    width: candidate.width(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    sizes: prepared
                        .and_then(ImagePresentation::sizes)
                        .map(str::to_owned),
                    width: dimensions.map(|value| value.0),
                    height: dimensions.map(|value| value.1),
                };
                (destination.clone(), Arc::new(image))
            })
            .collect()
    }
}

pub(super) async fn prepare(
    config: &Config,
    document: &Document,
    images: &Arc<ImageService>,
) -> Result<Prepared, RenderFailure> {
    let mut references = BTreeSet::new();
    for node in &document.blocks {
        block(node, &mut references);
    }
    for node in &document.footnotes {
        inlines(&node.children, &mut references);
    }
    let mut presentations = HashMap::new();
    let mut observed = HashMap::new();
    let mut originals = Vec::new();
    let mut retained = 0usize;
    for destination in references {
        let tail = destination
            .strip_prefix("/assets/")
            .expect("canonical references");
        let original = config.image_url(&destination).expect("local URL");
        // Bound preparation separately from the producer's output checks.
        retained = retained.saturating_add((destination.len() + original.len() + 128) * 9);
        if retained > 2_000_000 {
            return Err(RenderFailure);
        }
        let (info, version) =
            if let Ok(path) = AssetPath::parse(tail.split(['?', '#']).next().unwrap_or("")) {
                if !observed.contains_key(&path) {
                    let result = images.inspect(path.clone()).await;
                    observed.insert(path.clone(), result);
                }
                let (dependency, info) = &observed[&path];
                let version = if config.watch {
                    match &dependency.state {
                        SourceState::Hash(hash) => Some(
                            hash.iter()
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<String>(),
                        ),
                        SourceState::Missing => Some("missing".into()),
                        SourceState::Unstable => None,
                    }
                } else {
                    None
                };
                (info.clone(), version)
            } else {
                (None, None)
            };
        let original = versioned(&original, version.as_deref());
        let tail = versioned(tail, version.as_deref());
        let presentation = if let Some(info) = info {
            let maximum = info.width.min(2560);
            let mut candidates = Vec::new();
            for width in WIDTHS.into_iter().filter(|width| *width < maximum) {
                candidates.push(
                    ImageCandidate::new(
                        format!("{}images/{width}/{tail}", config.public_base),
                        width,
                    )
                    .map_err(|_| RenderFailure)?,
                );
            }
            candidates.push(
                ImageCandidate::new(format!("{}images/auto/{tail}", config.public_base), maximum)
                    .map_err(|_| RenderFailure)?,
            );
            ImagePresentation::new(
                format!("{}images/1280/{tail}", config.public_base),
                candidates,
                Some("100vw".into()),
                Some((info.width, info.height)),
                ImageLayout::Responsive,
            )
        } else {
            ImagePresentation::new(&original, vec![], None, None, ImageLayout::Responsive)
        }
        .map_err(|_| RenderFailure)?;
        originals.push((destination.clone(), original));
        presentations.insert(destination, presentation);
    }
    let dependencies: Vec<_> = observed
        .into_values()
        .map(|(dependency, _)| dependency)
        .collect();
    let cacheable = dependencies
        .iter()
        .all(|d| d.state != SourceState::Unstable);
    Ok(Prepared {
        presentations,
        dependencies,
        cacheable,
        originals,
    })
}

fn versioned(url: &str, version: Option<&str>) -> String {
    let Some(version) = version else {
        return url.to_owned();
    };
    let (base, fragment) = url
        .split_once('#')
        .map_or((url, None), |(base, fragment)| (base, Some(fragment)));
    let separator = if base.contains('?') { '&' } else { '?' };
    let mut value = format!("{base}{separator}__watch={version}");
    if let Some(fragment) = fragment {
        value.push('#');
        value.push_str(fragment);
    }
    value
}

fn block(node: &BlockNode, references: &mut BTreeSet<String>) {
    match &node.kind {
        BlockKind::Paragraph(p) => inlines(&p.children, references),
        BlockKind::Heading(h) => inlines(&h.children, references),
        BlockKind::GfmTable(t) => {
            for cell in &t.header {
                inlines(&cell.children, references);
            }
            for row in &t.rows {
                for cell in &row.cells {
                    inlines(&cell.children, references);
                }
            }
        }
        _ => {}
    }
    for child in &node.children {
        block(child, references);
    }
}
fn inlines(nodes: &[InlineNode], references: &mut BTreeSet<String>) {
    for node in nodes {
        match &node.kind {
            InlineKind::Image {
                destination, alt, ..
            } => {
                if destination.starts_with("/assets/") {
                    references.insert(destination.clone());
                }
                inlines(alt, references);
            }
            InlineKind::Annotation { children, .. }
            | InlineKind::Emphasis { children }
            | InlineKind::Strong { children }
            | InlineKind::Strikethrough { children }
            | InlineKind::Link { children, .. } => inlines(children, references),
            _ => {}
        }
    }
}
