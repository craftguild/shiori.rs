use super::{Error, Format, Info, MAX_BYTES, Variant, metadata};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use image::{
    DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits,
    metadata::Orientation,
};
use img_parts::{DynImage, ImageICC};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};

pub(super) struct Decoded {
    pub info: Info,
    pub source: Format,
    pixels: image::RgbaImage,
    orientation: Orientation,
    icc: Option<Vec<u8>>,
}

pub(super) struct Header {
    pub info: Info,
    source: Format,
    alpha: bool,
    orientation: Orientation,
    icc: Option<Vec<u8>>,
}

pub(super) fn header(bytes: &[u8]) -> Result<Header, Error> {
    let source = metadata::classify(bytes)?;
    let format = match source {
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Png => ImageFormat::Png,
        Format::Webp => ImageFormat::WebP,
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| Error::Decode)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > 16_384
        || height > 16_384
        || u64::from(width) * u64::from(height) > 64_000_000
    {
        return Err(Error::Decode);
    }
    if !matches!(
        decoder.color_type(),
        image::ColorType::Rgb8
            | image::ColorType::Rgba8
            | image::ColorType::L8
            | image::ColorType::La8
    ) {
        return Err(Error::Unsupported);
    }
    let icc = decoder.icc_profile().map_err(|_| Error::Unsupported)?;
    if icc.is_some()
        && !matches!(
            decoder.color_type(),
            image::ColorType::Rgb8 | image::ColorType::Rgba8
        )
    {
        return Err(Error::Unsupported);
    }
    if icc
        .as_ref()
        .is_some_and(|profile| !metadata::valid_rgb_icc(profile))
    {
        return Err(Error::Unsupported);
    }
    if let Some(exif) = decoder.exif_metadata().map_err(|_| Error::Unsupported)?
        && !metadata::supported_exif_color(&exif, icc.is_some())
    {
        return Err(Error::Unsupported);
    }
    let orientation = decoder.orientation().map_err(|_| Error::Decode)?;
    let swaps = matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let info = if swaps {
        Info {
            width: height,
            height: width,
        }
    } else {
        Info { width, height }
    };
    Ok(Header {
        info,
        source,
        alpha: decoder.color_type().has_alpha(),
        orientation,
        icc,
    })
}

impl Header {
    pub fn webp_available(&self, width: u32) -> bool {
        (width <= 16_383 && self.info.height_at(width) <= 16_383)
            || (self.source == Format::Webp
                && width == self.info.width
                && self.orientation == Orientation::NoTransforms)
    }
    pub fn fallback(&self) -> Format {
        match self.source {
            Format::Webp if self.alpha => Format::Png,
            Format::Webp => Format::Jpeg,
            other => other,
        }
    }
    pub fn decode(self, bytes: &[u8]) -> Result<Decoded, Error> {
        let format = match self.source {
            Format::Jpeg => ImageFormat::Jpeg,
            Format::Png => ImageFormat::Png,
            Format::Webp => ImageFormat::WebP,
        };
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let mut image =
            DynamicImage::from_decoder(reader.into_decoder().map_err(|_| Error::Decode)?)
                .map_err(|_| Error::Decode)?;
        image.apply_orientation(self.orientation);
        let pixels = image.into_rgba8();
        Ok(Decoded {
            info: self.info,
            source: self.source,
            pixels,
            orientation: self.orientation,
            icc: self.icc,
        })
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<Decoded, Error> {
    header(bytes)?.decode(bytes)
}

impl Decoded {
    pub fn encode(self, original: Vec<u8>, width: u32, format: Format) -> Result<Variant, Error> {
        self.encode_with_limit(original, width, format, MAX_BYTES)
    }
    pub(super) fn encode_with_limit(
        self,
        original: Vec<u8>,
        width: u32,
        format: Format,
        limit: usize,
    ) -> Result<Variant, Error> {
        let same_size = width == self.info.width;
        let height = self.info.height_at(width);
        let retain = same_size
            && self.source == format
            && self.orientation == Orientation::NoTransforms
            && original.len() <= limit;
        if same_size
            && self.source == Format::Webp
            && format == Format::Webp
            && self.orientation == Orientation::NoTransforms
            && (width > 16_383 || height > 16_383)
        {
            return if original.len() <= limit {
                finish(original, format, width, height)
            } else {
                Err(Error::OutputLimit)
            };
        }
        let encoded = match self.encode_pixels(width, height, format, limit) {
            Ok(encoded) if retain && original.len() <= encoded.len() => original,
            Ok(encoded) => encoded,
            Err(Error::OutputLimit) if retain => original,
            Err(error) => return Err(error),
        };
        finish(encoded, format, width, height)
    }
    fn encode_pixels(
        self,
        width: u32,
        height: u32,
        format: Format,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        let same_size = width == self.info.width;
        let pixels = if same_size {
            self.pixels.into_raw()
        } else {
            let source = Image::from_vec_u8(
                self.info.width,
                self.info.height,
                self.pixels.into_raw(),
                PixelType::U8x4,
            )
            .map_err(|_| Error::Internal)?;
            let mut destination = Image::new(width, height, PixelType::U8x4);
            Resizer::new()
                .resize(
                    &source,
                    &mut destination,
                    &ResizeOptions::new()
                        .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                        .use_alpha(true),
                )
                .map_err(|_| Error::Internal)?;
            destination.into_vec()
        };
        let mut encoded = match format {
            Format::Webp => {
                if width > 16_383 || height > 16_383 {
                    return Err(Error::Unsupported);
                }
                let encoded = webp::Encoder::from_rgba(&pixels, width, height)
                    .encode_simple(self.source == Format::Png, 80.0)
                    .map_err(|_| Error::Internal)?;
                if encoded.len() > limit {
                    return Err(Error::OutputLimit);
                }
                encoded.to_vec()
            }
            Format::Png => {
                let mut output = Bounded::new(limit);
                let result = image::codecs::png::PngEncoder::new(&mut output).write_image(
                    &pixels,
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                );
                result.map_err(|_| output.failure())?;
                output.data
            }
            Format::Jpeg => {
                let rgb: Vec<_> = pixels
                    .chunks_exact(4)
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                let mut output = Bounded::new(limit);
                let result = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80)
                    .encode(&rgb, width, height, image::ExtendedColorType::Rgb8);
                result.map_err(|_| output.failure())?;
                output.data
            }
        };
        if let Some(icc) = self.icc {
            let mut container = DynImage::from_bytes(encoded.into())
                .map_err(|_| Error::Internal)?
                .ok_or(Error::Internal)?;
            container.set_icc_profile(Some(icc.into()));
            if container.len() > limit {
                return Err(Error::OutputLimit);
            }
            let mut writer = Bounded::new(limit);
            let result = container.encoder().write_to(&mut writer);
            result.map_err(|_| writer.failure())?;
            encoded = writer.data;
        }
        if encoded.len() > limit {
            return Err(Error::OutputLimit);
        }
        Ok(encoded)
    }
}

fn finish(encoded: Vec<u8>, format: Format, width: u32, height: u32) -> Result<Variant, Error> {
    if encoded.len() > MAX_BYTES {
        return Err(Error::OutputLimit);
    }
    let etag = format!("\"{:x}\"", Sha256::digest(&encoded));
    Ok(Variant {
        // Byte accounting uses payload length. Shrink spare Vec capacity
        // before sharing it so the retained backing allocation agrees.
        bytes: encoded.into_boxed_slice().into(),
        format,
        etag,
        width,
        height,
    })
}

struct Bounded {
    data: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl Bounded {
    fn new(limit: usize) -> Self {
        Self {
            data: vec![],
            limit,
            exceeded: false,
        }
    }
    fn failure(&self) -> Error {
        if self.exceeded {
            Error::OutputLimit
        } else {
            Error::Internal
        }
    }
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.data.len().saturating_add(bytes.len()) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("encoded image exceeds limit"));
        }
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
