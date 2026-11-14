use chigiri::{FieldContext, Id};
use std::sync::Arc;

use crate::{
    graphql::{Context, Error, Types},
    metadata::Article,
    rendering::{Asset, HtmlBlock},
};

pub struct PostFields;
impl generated::PostResolver<Context, Types, Error> for PostFields {
    async fn id<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Id, Error> {
        Ok(post.id.into())
    }
    async fn title<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<String, Error> {
        Ok(post.metadata.title.clone())
    }
    async fn description<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<String, Error> {
        Ok(post.metadata.description.clone())
    }
    async fn author<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Option<String>, Error> {
        Ok((!post.metadata.author.is_empty()).then(|| post.metadata.author.clone()))
    }
    async fn tags<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Vec<String>, Error> {
        Ok(post.metadata.tags.clone())
    }
    async fn created_at<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<String, Error> {
        Ok(post.created_at.clone())
    }
    async fn blocks<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Option<Vec<HtmlBlock>>, Error> {
        let context = ctx.context();
        let value = context
            .renderer
            .render(post.clone(), &context.memo)
            .await
            .map_err(|_| Error::Render)?;
        Ok(Some(value.blocks.clone()))
    }
    async fn assets<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Option<Vec<Asset>>, Error> {
        let context = ctx.context();
        let value = context
            .renderer
            .render(post.clone(), &context.memo)
            .await
            .map_err(|_| Error::Render)?;
        Ok(Some(value.assets.clone()))
    }
    async fn structured<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Option<Arc<crate::structured::Document>>, Error> {
        let context = ctx.context();
        context
            .renderer
            .structure(post.clone(), &context.memo)
            .await
            .map(Some)
            .map_err(|_| Error::Structure)
    }
}
