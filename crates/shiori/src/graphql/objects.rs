use crate::{
    graphql::{Connection, Context, Error, MonthlyArchive, PageInfo, Tag, Types},
    metadata::Article,
};
use chigiri::{FieldContext, Id};
use std::sync::Arc;

pub struct ConnectionFields;
impl generated::PostConnectionResolver<Context, Types, Error> for ConnectionFields {
    async fn nodes<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a Connection,
    ) -> Result<Vec<Arc<Article>>, Error> {
        Ok(parent.nodes.clone())
    }
    async fn total_count<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a Connection,
    ) -> Result<i32, Error> {
        Ok(parent.total)
    }
    async fn page_info<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a Connection,
    ) -> Result<PageInfo, Error> {
        Ok(parent.page.clone())
    }
}
pub struct PageFields;
impl generated::PageInfoResolver<Context, Types, Error> for PageFields {
    async fn start_cursor<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a PageInfo,
    ) -> Result<Option<Id>, Error> {
        Ok(parent.start.clone())
    }
    async fn has_previous_page<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a PageInfo,
    ) -> Result<bool, Error> {
        Ok(parent.has_previous)
    }
    async fn end_cursor<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a PageInfo,
    ) -> Result<Option<Id>, Error> {
        Ok(parent.end.clone())
    }
    async fn has_next_page<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a PageInfo,
    ) -> Result<bool, Error> {
        Ok(parent.has_next)
    }
}
pub struct TagFields;
impl generated::TagResolver<Context, Types, Error> for TagFields {
    async fn name<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a Tag,
    ) -> Result<String, Error> {
        Ok(parent.name.clone())
    }
    async fn count<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a Tag,
    ) -> Result<i32, Error> {
        Ok(parent.count)
    }
}

pub struct ArchiveFields;
impl generated::MonthlyArchiveResolver<Context, Types, Error> for ArchiveFields {
    async fn year<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a MonthlyArchive,
    ) -> Result<i32, Error> {
        Ok(parent.year)
    }
    async fn month<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a MonthlyArchive,
    ) -> Result<i32, Error> {
        Ok(parent.month)
    }
    async fn count<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a MonthlyArchive,
    ) -> Result<i32, Error> {
        Ok(parent.count)
    }
}
