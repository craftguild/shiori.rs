use std::collections::HashSet;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, macros::format_description};

pub const MAX_FILE_BYTES: usize = 1_000_000;
pub const MAX_FRONT_MATTER: usize = 64 * 1024;
pub const MAX_TAGS: usize = 1000;

#[derive(Clone, Debug, Serialize)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
struct FrontMatter {
    title: Value,
    #[serde(default)]
    author: Value,
    #[serde(default = "empty_description")]
    description: Value,
    #[serde(default = "empty_tags")]
    tags: Value,
}
fn empty_description() -> Value {
    Value::String(String::new())
}
fn empty_tags() -> Value {
    Value::Array(Vec::new())
}

#[derive(Clone, Debug)]
pub struct Article {
    pub id: i64,
    pub created_at: String,
    pub metadata: Metadata,
    pub body: String,
    pub hash: [u8; 32],
    pub source_bytes: usize,
}

pub fn parse_id(id: &str) -> Result<i64> {
    ensure!(
        !id.is_empty()
            && id.len() <= 19
            && !id.starts_with('0')
            && id.bytes().all(|b| b.is_ascii_digit()),
        "ID must be canonical positive epoch milliseconds"
    );
    let id: i64 = id.parse().context("ID is outside timestamp range")?;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(id) * 1_000_000)
        .context("ID is outside UTC timestamp range")?;
    Ok(id)
}

pub fn parse(id: i64, bytes: &[u8]) -> Result<Article> {
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "source exceeds {MAX_FILE_BYTES} bytes"
    );
    let text = std::str::from_utf8(bytes).context("article must be UTF-8")?;
    let mut lines = text.split_inclusive('\n');
    ensure!(
        lines.next().map(|l| l.trim_end_matches(['\r', '\n'])) == Some("---"),
        "article must begin with --- front matter"
    );
    let start = text.find('\n').context("unclosed front matter")? + 1;
    let mut end = start;
    let mut body_start = None;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            body_start = Some(end + line.len());
            break;
        }
        end += line.len();
        ensure!(
            end - start <= MAX_FRONT_MATTER,
            "front matter exceeds 64 KiB"
        );
    }
    let body_start = body_start.context("unclosed front matter")?;
    let mut budget = serde_saphyr::budget::Budget::default();
    budget.max_depth = 32;
    budget.max_nodes = 16_384;
    budget.max_events = 32_768;
    budget.max_aliases = 64;
    budget.max_anchors = 64;
    budget.max_documents = 1;
    budget.max_recorded_anchor_bytes = MAX_FRONT_MATTER;
    budget.max_total_scalar_bytes = MAX_FRONT_MATTER;
    let mut options = serde_saphyr::Options::default();
    options.budget = Some(budget);
    // Ignored metadata may contain YAML-native scalars. Known fields still pass
    // through exact Value string/array checks, so non-finite values cannot coerce.
    options.non_finite_float_policy = serde_saphyr::NonFiniteFloatPolicy::PassThrough;
    let value: FrontMatter = serde_saphyr::from_str_with_options(&text[start..end], options)
        .context("invalid YAML front matter")?;
    let title = value
        .title
        .as_str()
        .context("title must be a string")?
        .trim();
    ensure!(!title.is_empty(), "title cannot be empty");
    let author = if value.author.is_null() {
        ""
    } else {
        value
            .author
            .as_str()
            .context("author must be a string or null")?
            .trim()
    };
    let description = value
        .description
        .as_str()
        .context("description must be a string")?;
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    {
        let list = value
            .tags
            .as_array()
            .context("tags must be an array of strings")?;
        ensure!(list.len() <= MAX_TAGS, "tags exceeds {MAX_TAGS} entries");
        for value in list {
            let Some(tag) = value.as_str() else {
                bail!("every tag must be a string")
            };
            let tag = tag.trim();
            if !tag.is_empty() && seen.insert(tag) {
                tags.push(tag.to_owned());
            }
        }
    }
    let created_at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(id) * 1_000_000)?
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))?;
    Ok(Article {
        id,
        created_at,
        metadata: Metadata {
            title: title.to_owned(),
            author: author.to_owned(),
            description: description.to_owned(),
            tags,
        },
        body: text[body_start..].to_owned(),
        hash: Sha256::digest(bytes).into(),
        source_bytes: bytes.len(),
    })
}
