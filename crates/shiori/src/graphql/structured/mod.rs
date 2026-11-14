mod blocks;
mod document;
mod inlines;
use super::{Context, Error, Types};
use crate::structured::*;
use chigiri::{FieldContext, Id};
use std::sync::Arc;
pub struct Fields;
fn ids(values: &[Option<String>]) -> Vec<Option<Id>> {
    values.iter().map(|id| id.clone().map(Into::into)).collect()
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
    ($resolver:ident, $domain:ty, $parent:ident; $( $field:ident: $result:ty => $value:expr; )*) => {
        object!($resolver, $domain, $parent;
            id: Id => $parent.id.clone().into();
            text: Option<String> => $parent.text.clone();
            child_ids: Vec<Option<Id>> => ids(&$parent.child_ids);
            $( $field: $result => $value; )*
        );
    };
}
pub(super) use node;
