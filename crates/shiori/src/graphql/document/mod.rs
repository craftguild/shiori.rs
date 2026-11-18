mod blocks;
mod inlines;
mod projection;
mod resources;
mod selection;
pub(super) use selection::profile;

use super::{Context, Error, Types};
use crate::document::{AnnotationEntry, Candidate, Document, Image, Node};
use chigiri::{FieldContext, Id};
use oriha_core::DocumentNodeKind as N;
use std::sync::Arc;

pub struct Fields;
fn ids(values: &[String]) -> Vec<Id> {
    values.iter().cloned().map(Into::into).collect()
}
macro_rules! object {
    ($resolver:ident, $domain:ty, $parent:ident; $( $field:ident: $result:ty => $value:expr; )*) => {
        impl generated::$resolver<Context, Types, Error> for Fields {
            $(async fn $field<'a>(&'a self, _: FieldContext<'a, Context>, $parent: &'a $domain) -> Result<$result, Error> { Ok($value) })*
        }
    };
}
pub(super) use object;
macro_rules! node {
    ($resolver:ident, $parent:ident; $( $field:ident: $result:ty => $value:expr; )*) => {
        object!($resolver, Node, $parent;
            id: Id => $parent.value().id.clone().into();
            $( $field: $result => $value; )*
        );
    };
}
pub(super) use node;

#[derive(Clone)]
pub struct Line {
    pub node: Node,
    pub index: usize,
}
#[derive(Clone)]
pub struct Token {
    pub line: Line,
    pub index: usize,
}
#[derive(Clone)]
pub struct Font {
    pub document: Arc<Document>,
    pub index: usize,
}
#[derive(Clone)]
pub struct Diagnostic {
    pub document: Arc<Document>,
    pub index: usize,
}

fn highlighting(node: &Node) -> Result<Option<&oriha_core::CodeHighlighting>, Error> {
    match &node.value().content {
        N::FencedCode { highlighting, .. } | N::IndentedCode { highlighting, .. } => {
            Ok(highlighting.as_ref())
        }
        _ => Err(Error::Structure),
    }
}
fn svg(node: &Node) -> Result<Option<&oriha_core::SvgContent>, Error> {
    match &node.value().content {
        N::FencedCode { svg, .. } | N::MathBlock { svg, .. } | N::Math { svg, .. } => {
            Ok(svg.as_ref())
        }
        _ => Err(Error::Structure),
    }
}
impl Line {
    fn value(&self) -> Result<&oriha_core::CodeLine, Error> {
        highlighting(&self.node)?
            .and_then(|value| value.lines.get(self.index))
            .ok_or(Error::Structure)
    }
}
impl Token {
    fn value(&self) -> Result<&oriha_core::SyntaxToken, Error> {
        self.line
            .value()?
            .tokens
            .get(self.index)
            .ok_or(Error::Structure)
    }
}
