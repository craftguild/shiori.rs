use super::Error;
use crate::metadata::{Article, valid_key};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::cmp::Reverse;

pub struct Boundary(pub i64, pub String);
impl Boundary {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let invalid = || Error::Input("Invalid pagination cursor");
        if value.len() > 16_384 {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value.strip_prefix("v1.").ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        let (date, key): (String, String) =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if !valid_key(&key) {
            return Err(invalid());
        }
        let parsed =
            time::OffsetDateTime::parse(&date, &time::format_description::well_known::Rfc3339)
                .map_err(|_| invalid())?;
        if crate::timestamps::format(parsed.to_offset(time::UtcOffset::UTC))
            .map_err(|_| invalid())?
            != date
        {
            return Err(invalid());
        }
        let millis = parsed
            .unix_timestamp_nanos()
            .div_euclid(1_000_000)
            .try_into()
            .map_err(|_| invalid())?;
        Ok(Self(millis, key))
    }
    pub fn order(&self) -> (Reverse<i64>, &str) {
        (Reverse(self.0), &self.1)
    }
}
pub fn order(article: &Article) -> (Reverse<i64>, &str) {
    (Reverse(article.created_ms), &article.id)
}
pub fn encode(article: &Article) -> chigiri::Id {
    let value =
        serde_json::to_vec(&(&article.created_at, &article.id)).expect("serializable cursor");
    format!("v1.{}", URL_SAFE_NO_PAD.encode(value)).into()
}
