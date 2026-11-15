use super::{RenderFailure, RenderResult, images, project};
use crate::{config::Config, images::ImageService, metadata::Article};
use oriha_core::{
    BlockOutput, BlockOutputMode, BlockOutputStatus, CodeBlockOptions, FeatureFlags, OrihaEngine,
    RenderInput, RenderOptions,
};
use std::sync::Arc;

// Called on a bounded render worker. Await image admission on the runtime while
// its dedicated blocking workers perform reads/codecs; the hook is lookup only.
pub(super) fn derive(
    config: &Config,
    article: &Article,
    images: &Arc<ImageService>,
    parsed: &oriha_core::ParsedDocument,
) -> RenderResult {
    let runtime = tokio::runtime::Handle::current();
    let prepared = runtime.block_on(images::prepare(config, parsed.document(), images))?;
    #[cfg(test)]
    images.checkpoint("prepared");
    let valid = prepared.cacheable && runtime.block_on(images.valid(prepared.dependencies.clone()));
    let policy = if valid {
        prepared.policy.clone()
    } else {
        prepared.fallback()
    };
    let output = OrihaEngine::default()
        .add_hook(policy)
        .derive_parsed_blocks(parsed, BlockOutputMode::Rendered);
    let mut result = output_result(output, &article.id)?;
    let mut cacheable = valid;
    if valid && !runtime.block_on(images.valid(prepared.dependencies.clone())) {
        // Retain and derive the same parsed document; unstable files get original
        // URLs without stale candidate descriptors and are never cached as valid.
        result = output_result(
            OrihaEngine::default()
                .add_hook(prepared.fallback())
                .derive_parsed_blocks(parsed, BlockOutputMode::Rendered),
            &article.id,
        )?;
        cacheable = false;
    }
    let rendered = Arc::get_mut(&mut result).expect("new projection");
    rendered.dependencies = prepared.dependencies;
    rendered.cacheable = cacheable;
    Ok(result)
}

fn output_result(output: BlockOutput, id: &str) -> RenderResult {
    let BlockOutput::Rendered(output) = output else {
        return Err(RenderFailure);
    };
    for diagnostic in &output.diagnostics {
        tracing::warn!(event="render_diagnostic",article_id=id,diagnostic=?diagnostic);
    }
    if output.status != BlockOutputStatus::Complete {
        tracing::error!(event="render_failed",article_id=id,status=?output.status);
        return Err(RenderFailure);
    }
    project(output)
}

pub(super) fn parse(article: &Article) -> oriha_core::ParsedDocument {
    let engine = OrihaEngine::default();
    engine.parse(RenderInput {
        markdown: article.body.clone(),
        features: FeatureFlags {
            math: true,
            mermaid: true,
            plantuml: true,
            raw_html: false,
            ..FeatureFlags::default()
        },
        locale: Some("en".into()),
        render_options: RenderOptions {
            code_block: CodeBlockOptions {
                show_language: true,
                show_copy_button: false,
                show_line_numbers: true,
            },
            ..Default::default()
        },
        ..Default::default()
    })
}

pub(super) fn structure(
    parsed: &oriha_core::ParsedDocument,
) -> Result<Arc<crate::structured::Document>, RenderFailure> {
    let BlockOutput::Structured(output) =
        OrihaEngine::default().derive_parsed_blocks(parsed, BlockOutputMode::Structured)
    else {
        return Err(RenderFailure);
    };
    if output.status != BlockOutputStatus::Complete {
        tracing::warn!(event="structure_failed", status=?output.status);
        return Err(RenderFailure);
    }
    let anchors = oriha_core::render::builtin_html_anchors(parsed);
    crate::structured::project(output, anchors)
}
