use chigiri::{FieldContext, Id};
use std::sync::Arc;

use crate::{
    graphql::{Context, Error, Types},
    metadata::Article,
};

pub struct PostFields;
impl generated::PostResolver<Context, Types, Error> for PostFields {
    async fn id<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Id, Error> {
        Ok(post.id.clone().into())
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
    async fn updated_at<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<String, Error> {
        Ok(post.updated_at.clone())
    }
    async fn source_path<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<String, Error> {
        Ok(post.source_path.clone())
    }
    async fn document<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        post: &'a Arc<Article>,
    ) -> Result<Option<Arc<crate::document::Document>>, Error> {
        let profile = super::document::profile(ctx)?;
        let context = ctx.context();
        context
            .renderer
            .document(post.clone(), &context.memo, profile)
            .await
            .map(Some)
            .map_err(|_| Error::Structure)
    }
}
