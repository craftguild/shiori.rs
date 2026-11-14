//! Conservative container eligibility checks before decoding pixels.
use super::{Error, Format};

pub(super) fn classify(bytes: &[u8]) -> Result<Format, Error> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)?;
        Ok(Format::Png)
    } else if bytes.starts_with(b"\xff\xd8") {
        jpeg(bytes)?;
        Ok(Format::Jpeg)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        let mut offset = 12;
        while offset + 8 <= bytes.len() {
            let tag = &bytes[offset..offset + 4];
            let length =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let data = bytes
                .get(offset + 8..offset + 8 + length)
                .ok_or(Error::Decode)?;
            if tag == b"ANIM"
                || tag == b"ANMF"
                || (tag == b"VP8X" && data.first().is_some_and(|b| b & 2 != 0))
            {
                return Err(Error::Unsupported);
            }
            offset = offset
                .checked_add(8 + length + (length % 2))
                .ok_or(Error::Decode)?;
        }
        Ok(Format::Webp)
    } else {
        Err(Error::Unsupported)
    }
}

fn png(bytes: &[u8]) -> Result<(), Error> {
    let mut offset = 8;
    let mut ihdr = false;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let tag = &bytes[offset + 4..offset + 8];
        let data = bytes
            .get(offset + 8..offset + 8 + length)
            .ok_or(Error::Decode)?;
        match tag {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(Error::Decode);
                }
                if data[8] > 8 {
                    return Err(Error::Unsupported);
                }
                ihdr = true;
            }
            b"acTL" => return Err(Error::Unsupported),
            // Unknown HDR/custom chromaticity cannot be silently dropped.
            b"cICP" | b"mDCv" | b"cLLi" => return Err(Error::Unsupported),
            b"gAMA" if data != 45455u32.to_be_bytes() => return Err(Error::Unsupported),
            b"cHRM" => {
                let srgb: Vec<u8> = [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000]
                    .into_iter()
                    .flat_map(u32::to_be_bytes)
                    .collect();
                if data != srgb {
                    return Err(Error::Unsupported);
                }
            }
            _ => {}
        }
        offset = offset.checked_add(12 + length).ok_or(Error::Decode)?;
        if tag == b"IEND" {
            break;
        }
    }
    if ihdr { Ok(()) } else { Err(Error::Decode) }
}

fn jpeg(bytes: &[u8]) -> Result<(), Error> {
    let mut offset = 2;
    while offset < bytes.len() {
        if bytes[offset] != 255 {
            return Err(Error::Decode);
        }
        while bytes.get(offset) == Some(&255) {
            offset += 1;
        }
        let marker = *bytes.get(offset).ok_or(Error::Decode)?;
        offset += 1;
        if marker == 0xda {
            return Err(Error::Decode);
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let size = bytes.get(offset..offset + 2).ok_or(Error::Decode)?;
        let size = u16::from_be_bytes(size.try_into().unwrap()) as usize;
        if size < 2 {
            return Err(Error::Decode);
        }
        let data = bytes.get(offset + 2..offset + size).ok_or(Error::Decode)?;
        if matches!(marker,0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if data.len() < 6 {
                return Err(Error::Decode);
            }
            return if data[0] == 8 && matches!(data[5], 1 | 3) {
                Ok(())
            } else {
                Err(Error::Unsupported)
            };
        }
        offset += size;
    }
    Err(Error::Decode)
}

pub(super) fn valid_rgb_icc(profile: &[u8]) -> bool {
    if profile.len() < 132
        || profile.len() > 16 * 1024 * 1024
        || profile.get(36..40) != Some(b"acsp")
        || profile.get(16..20) != Some(b"RGB ")
        || !matches!(profile[8], 2 | 4)
        || !matches!(&profile[20..24], b"XYZ " | b"Lab ")
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        u32::from_be_bytes(profile[range].try_into().unwrap()) as usize
    };
    if number(0..4) != profile.len() {
        return false;
    }
    let tags = number(128..132);
    if tags == 0 || tags > (profile.len() - 132) / 12 {
        return false;
    }
    (0..tags).all(|i| {
        let offset = 132 + i * 12;
        let start = number(offset + 4..offset + 8);
        let len = number(offset + 8..offset + 12);
        start >= 128
            && len >= 8
            && start
                .checked_add(len)
                .is_some_and(|end| end <= profile.len())
    })
}

// Dropping an EXIF-only non-sRGB declaration would change interpretation. Follow
// only the bounded TIFF directory graph needed for color declarations.
pub(super) fn supported_exif_color(exif: &[u8], has_icc: bool) -> bool {
    let data = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
    let little = match data.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return false,
    };
    let short = |offset: usize| -> Option<u16> {
        let bytes = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let long = |offset: usize| -> Option<u32> {
        let bytes = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    if short(2) != Some(42) {
        return false;
    }
    let Some(root) = long(4) else {
        return false;
    };
    let mut pending = vec![(root, false)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((offset, interop)) = pending.pop() {
        if offset == 0 {
            continue;
        }
        if !seen.insert(offset) || seen.len() > 16 {
            return false;
        }
        let offset = offset as usize;
        let Some(count) = short(offset).map(usize::from) else {
            return false;
        };
        if offset < 8
            || count > 4096
            || offset
                .checked_add(2 + count * 12 + 4)
                .is_none_or(|end| end > data.len())
        {
            return false;
        }
        for index in 0..count {
            let entry = offset + 2 + index * 12;
            let (Some(tag), Some(kind), Some(count)) =
                (short(entry), short(entry + 2), long(entry + 4))
            else {
                return false;
            };
            match tag {
                0x8769 | 0xa005 => {
                    if kind != 4 || count != 1 || pending.len() >= 16 {
                        return false;
                    }
                    pending.push((long(entry + 8).unwrap(), tag == 0xa005));
                }
                0xa001 => {
                    if kind != 3 || count != 1 {
                        return false;
                    }
                    if !has_icc && short(entry + 8) != Some(1) {
                        return false;
                    }
                }
                1 if interop
                    && !has_icc
                    && (kind != 2
                        || count != 4
                        || data.get(entry + 8..entry + 12) != Some(b"R98\0")) =>
                {
                    return false;
                }
                _ => {}
            }
        }
        pending.push((long(offset + 2 + count * 12).unwrap(), false));
    }
    true
}
