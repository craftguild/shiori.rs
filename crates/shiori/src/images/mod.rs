//! Lazy local-image delivery. Filesystem, codecs and process-local state belong here.
mod accept;
mod cache;
mod codec;
pub(crate) mod http;
mod metadata;
mod service;
mod source;
#[cfg(test)]
pub(crate) mod tests;

pub use service::ImageService;
pub(crate) use source::{AssetPath, Dependency, SourceState};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const WIDTHS: [u32; 6] = [320, 640, 960, 1280, 1920, 2560];
const POLICY: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
}
impl Format {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Pixels(u32),
    Auto,
}
impl Width {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        WIDTHS
            .into_iter()
            .find(|width| width.to_string() == value)
            .map(Self::Pixels)
            .ok_or(Error::Path)
    }
    pub fn effective(self, original: u32) -> u32 {
        match self {
            Self::Pixels(width) => width.min(original),
            Self::Auto => original.min(2560),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Path,
    Missing,
    Unsupported,
    Decode,
    OutputLimit,
    Unstable,
    DirectoryChanged,
    Busy,
    Internal,
    NotAcceptable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "image {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Info {
    pub width: u32,
    pub height: u32,
}
impl Info {
    pub(crate) fn height_at(&self, width: u32) -> u32 {
        ((u64::from(self.height) * u64::from(width) + u64::from(self.width) / 2)
            / u64::from(self.width))
        .max(1) as u32
    }
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub bytes: axum::body::Bytes,
    pub format: Format,
    pub etag: String,
    pub width: u32,
    pub height: u32,
}
