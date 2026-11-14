use super::{Error, Format};

// RFC media-range precedence is decided before quality. A specific q=0 must
// exclude a representation even when a broader wildcard has a positive q.
pub(super) fn negotiate(
    header: Option<&str>,
    fallback: Format,
    webp_available: bool,
) -> Result<Format, Error> {
    let Some(header) = header else {
        return Ok(fallback);
    };
    let ranges: Vec<_> = header.split(',').filter_map(range).collect();
    let score = |format: Format| {
        ranges
            .iter()
            .filter_map(|(media, quality, explicit)| {
                let specificity = if media == format.mime() {
                    2
                } else if media == "image/*" {
                    1
                } else if media == "*/*" {
                    0
                } else {
                    return None;
                };
                Some((specificity, *quality, *explicit))
            })
            .max_by_key(|&(specificity, quality, _)| (specificity, quality))
            .map(|(_, quality, explicit)| (quality, explicit))
            .unwrap_or((0, false))
    };
    let conventional = score(fallback);
    let webp = if webp_available {
        score(Format::Webp)
    } else {
        (0, false)
    };
    if conventional.0 == 0 && webp.0 == 0 {
        return Err(Error::NotAcceptable);
    }
    // An unrestricted generic client gets the conventional representation.
    if webp.0 > conventional.0 || (webp.0 == conventional.0 && webp.1) {
        Ok(Format::Webp)
    } else {
        Ok(fallback)
    }
}

fn range(raw: &str) -> Option<(String, u16, bool)> {
    let mut parts = raw.trim().split(';');
    let media = parts.next()?.trim().to_ascii_lowercase();
    let mut quality = 1000;
    for parameter in parts {
        let (name, value) = parameter.trim().split_once('=')?;
        if name.eq_ignore_ascii_case("q") {
            quality = qvalue(value.trim()).unwrap_or(0);
        } else {
            return None;
        } // No representation parameters are supplied by this endpoint.
    }
    let explicit = media == "image/webp";
    Some((media, quality, explicit))
}

fn qvalue(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    match whole {
        "0" => Some(fraction.parse::<u16>().unwrap_or(0) * 10u16.pow((3 - fraction.len()) as u32)),
        "1" if fraction.bytes().all(|c| c == b'0') => Some(1000),
        _ => None,
    }
}
