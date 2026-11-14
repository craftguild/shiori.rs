use chigiri::{FieldContext, Input};
use std::collections::BTreeMap;
use std::sync::Arc;
use time::{Date, Month, OffsetDateTime};

use crate::{
    graphql::{Connection, Context, Error, MonthlyArchive, PageInfo, Tag, Types},
    metadata::{Article, parse_id},
};

pub struct QueryFields;

fn optional<T>(input: Input<T>) -> Option<T> {
    match input {
        Input::Value(v) => Some(v),
        Input::Null | Input::Undefined => None,
    }
}

struct Pagination {
    first: usize,
    after: Option<i64>,
    before: Option<i64>,
}

impl Pagination {
    fn new(
        first: Option<i32>,
        after: Input<chigiri::Id>,
        before: Input<chigiri::Id>,
    ) -> Result<Self, Error> {
        let first = first.unwrap_or(20);
        if !(1..=100).contains(&first) {
            return Err(Error::Input("first must be between 1 and 100"));
        }
        let after = optional(after);
        let before = optional(before);
        if after.is_some() && before.is_some() {
            return Err(Error::Input("after and before cannot be combined"));
        }
        let before = before
            .map(|id| parse_id(id.as_str()))
            .transpose()
            .map_err(|_| Error::Input("Invalid pagination cursor"))?;
        let after = after
            .map(|id| parse_id(id.as_str()))
            .transpose()
            .map_err(|_| Error::Input("Invalid pagination cursor"))?;
        Ok(Self {
            first: first as usize,
            after,
            before,
        })
    }

    fn connection<'a>(
        &self,
        articles: impl Iterator<Item = &'a Arc<Article>> + Clone,
    ) -> Connection {
        let total = articles.clone().count() as i32;
        // Input is already filtered and descending. For before, retain the
        // nearest newer window, rather than the first/newest window.
        let eligible = articles.clone().filter(|article| {
            self.after.is_none_or(|boundary| article.id < boundary)
                && self.before.is_none_or(|boundary| article.id > boundary)
        });
        let skip = if self.before.is_some() {
            eligible.clone().count().saturating_sub(self.first)
        } else {
            0
        };
        let nodes: Vec<_> = eligible.skip(skip).take(self.first).cloned().collect();
        let start = nodes.first().map(|article| article.id.into());
        let end = nodes.last().map(|article| article.id.into());
        let has_previous = nodes
            .first()
            .is_some_and(|first| articles.clone().any(|article| article.id > first.id));
        let has_next = nodes
            .last()
            .is_some_and(|last| articles.clone().any(|article| article.id < last.id));
        Connection {
            nodes,
            total,
            page: PageInfo {
                start,
                end,
                has_previous,
                has_next,
            },
        }
    }
}

fn month_range(year: i32, month: i32) -> Result<std::ops::Range<i64>, Error> {
    if !(1970..=9999).contains(&year) {
        return Err(Error::Input("year must be between 1970 and 9999"));
    }
    let month = u8::try_from(month)
        .ok()
        .and_then(|month| Month::try_from(month).ok())
        .ok_or(Error::Input("month must be between 1 and 12"))?;
    let start = Date::from_calendar_date(year, month, 1)
        .map_err(|_| Error::Input("Invalid calendar month"))?
        .midnight()
        .assume_utc()
        .unix_timestamp()
        * 1000;
    // UTC Unix days have a fixed length. An integer upper bound also handles
    // December 9999 without constructing an unsupported year-10000 date.
    let end = start + i64::from(month.length(year)) * 86_400_000;
    Ok(start..end)
}

impl generated::QueryResolver<Context, Types, Error> for QueryFields {
    async fn post<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        args: generated::QueryPostArgs,
    ) -> Result<Option<Arc<Article>>, Error> {
        let id = parse_id(args.id.as_str()).map_err(|_| Error::Input("Invalid article ID"))?;
        Ok(ctx.context().snapshot.articles.get(&id).cloned())
    }
    async fn posts<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        args: generated::QueryPostsArgs,
    ) -> Result<Connection, Error> {
        let pagination = Pagination::new(args.first, args.after, args.before)?;
        let tag = optional(args.tag).map(|s| s.trim().to_owned());
        if tag.as_ref().is_some_and(|t| t.is_empty()) {
            return Err(Error::Input("tag cannot be empty"));
        }
        if let Some(query) = optional(args.query) {
            let query = crate::search::SearchQuery::new(&query)?;
            let matches = ctx
                .context()
                .catalog
                .search
                .select(ctx.context().snapshot.clone(), query, tag)
                .await?;
            return Ok(pagination.connection(matches.iter()));
        }
        let snapshot = &ctx.context().snapshot;
        let filtered = snapshot.articles.values().rev().filter(|article| {
            tag.as_ref()
                .is_none_or(|tag| article.metadata.tags.contains(tag))
        });
        Ok(pagination.connection(filtered))
    }
    async fn posts_by_month<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
        args: generated::QueryPostsByMonthArgs,
    ) -> Result<Connection, Error> {
        let pagination = Pagination::new(args.first, args.after, args.before)?;
        let range = month_range(args.year, args.month)?;
        let articles = ctx
            .context()
            .snapshot
            .articles
            .range(range)
            .rev()
            .map(|(_, article)| article);
        Ok(pagination.connection(articles))
    }
    async fn monthly_archives<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
    ) -> Result<Vec<MonthlyArchive>, Error> {
        let mut counts = BTreeMap::new();
        for article in ctx.context().snapshot.articles.values() {
            // All catalog IDs were validated at ingestion.
            let date =
                OffsetDateTime::from_unix_timestamp_nanos(i128::from(article.id) * 1_000_000)
                    .expect("validated catalog timestamp");
            *counts
                .entry((date.year(), i32::from(u8::from(date.month()))))
                .or_insert(0) += 1;
        }
        Ok(counts
            .into_iter()
            .rev()
            .map(|((year, month), count)| MonthlyArchive { year, month, count })
            .collect())
    }
    async fn tags<'a>(&'a self, ctx: FieldContext<'a, Context>) -> Result<Vec<Tag>, Error> {
        Ok(ctx
            .context()
            .snapshot
            .tags
            .iter()
            .map(|(name, count)| Tag {
                name: name.clone(),
                count: *count as i32,
            })
            .collect())
    }
}
