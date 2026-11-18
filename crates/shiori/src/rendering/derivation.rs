use super::{Profile, RenderFailure, RenderResult, images};
use crate::{
    config::Config,
    document::{Document, Image},
    images::ImageService,
    metadata::Article,
};
use oriha_core::{
    BlockOutput, BlockOutputMode, DocumentNodeKind, FeatureFlags, OrihaEngine, RenderInput,
};
use std::sync::Arc;

pub(super) fn derive(
    config: &Config,
    article: &Article,
    service: &Arc<ImageService>,
    parsed: &oriha_core::ParsedDocument,
    profile: Profile,
) -> RenderResult {
    let BlockOutput::Structured(output) = OrihaEngine::default().derive_parsed_blocks_with_options(
        parsed,
        BlockOutputMode::Structured,
        profile.structured,
    ) else {
        return Err(RenderFailure);
    };
    for diagnostic in &output.diagnostics {
        tracing::warn!(event="document_diagnostic",article_id=%article.id,code=%diagnostic.code,node_id=?diagnostic.block_id);
    }
    let mut document = Document::new(output)?;
    if profile.images {
        let runtime = tokio::runtime::Handle::current();
        let prepared = runtime.block_on(images::prepare(config, parsed.document(), service))?;
        #[cfg(test)]
        service.checkpoint("prepared");
        let valid =
            prepared.cacheable && runtime.block_on(service.valid(prepared.dependencies.clone()));
        document.images = prepared.project(!valid);
        document.cacheable = valid;
        if valid && !runtime.block_on(service.valid(prepared.dependencies.clone())) {
            document.images = prepared.project(true);
            document.cacheable = false;
        }
        document.dependencies = prepared.dependencies;
        for node in &document.data.nodes {
            if let DocumentNodeKind::Image {
                destination,
                normalized_destination,
                ..
            } = &node.content
            {
                document
                    .images
                    .entry(destination.clone())
                    .or_insert_with(|| {
                        Arc::new(Image {
                            url: normalized_destination.clone(),
                            original_url: normalized_destination.clone(),
                            candidates: Vec::new(),
                            sizes: None,
                            width: None,
                            height: None,
                        })
                    });
            }
        }
    }
    if !document.within_output_budget() {
        tracing::warn!(event="document_output_limit",article_id=%article.id);
        return Err(RenderFailure);
    }
    Ok(Arc::new(document))
}

pub(super) fn parse(article: &Article) -> oriha_core::ParsedDocument {
    OrihaEngine::default().parse(RenderInput {
        markdown: article.body.clone(),
        features: FeatureFlags {
            math: true,
            mermaid: true,
            plantuml: true,
            raw_html: false,
            ..FeatureFlags::default()
        },
        locale: Some("en".into()),
        ..Default::default()
    })
}
