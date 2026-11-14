use super::{Context, Error, Types};
use crate::preview::ContentChange;
use chigiri::{FieldContext, SubscriptionSource};

pub struct SubscriptionFields;
impl generated::SubscriptionResolver<Context, Types, Error> for SubscriptionFields {
    type ContentChangedEvent = ContentChange;

    async fn subscribe_content_changed<'a>(
        &'a self,
        ctx: FieldContext<'a, Context>,
    ) -> Result<SubscriptionSource<ContentChange, Error>, Error> {
        let preview = ctx
            .context()
            .catalog
            .preview()
            .ok_or(Error::WatchDisabled)?;
        Ok(SubscriptionSource::new(preview.source()))
    }

    async fn resolve_content_changed_event<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        event: &'a ContentChange,
    ) -> Result<ContentChange, Error> {
        Ok(event.clone())
    }
}

pub struct ChangeFields;
impl generated::ContentChangeResolver<Context, Types, Error> for ChangeFields {
    async fn revision<'a>(
        &'a self,
        _: FieldContext<'a, Context>,
        parent: &'a ContentChange,
    ) -> Result<chigiri::Id, Error> {
        Ok(parent.0.clone().into())
    }
}
