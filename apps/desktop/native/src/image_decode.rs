use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use rune::{Image, ImageCache};
use std::io::Cursor;

fn thumbnail(cache: &ImageCache, bytes: &[u8], size: u32) -> Option<Image> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().ok()?;
    limits.reserve(decoder.total_bytes()).ok()?;
    decoder.set_limits(limits).ok()?;
    let orientation = decoder.orientation().ok()?;
    let image = DynamicImage::from_decoder(decoder).ok()?;
    Some(resize(cache, image, size, orientation))
}

fn resize(
    cache: &ImageCache,
    mut image: DynamicImage,
    size: u32,
    orientation: image::metadata::Orientation,
) -> Image {
    if image.width() > size || image.height() > size {
        image = image.thumbnail(size, size);
    }
    image.apply_orientation(orientation);
    let pixels = image.into_rgba8();
    cache.rgba(pixels.width(), pixels.height(), pixels.into_raw())
}

fn avif_badge(cache: &ImageCache, bytes: &[u8], size: u32) -> Option<Image> {
    use avif_rust::{NativeDecodeLimits, decode_frame_bytes_strict_with_limits};
    const MIB: usize = 1024 * 1024;
    // Bound the AV1 frame, its auxiliary planes, and decoder scratch before allocating pixels.
    let limits = NativeDecodeLimits::new(
        4 * MIB,
        8192,
        8192,
        16 * MIB,
        64 * MIB,
        4 * MIB,
        MIB,
        1024,
        1024,
        256,
        8,
        1,
    )
    .with_max_live_allocation_bytes(128 * MIB)
    .ok()?;
    let frame = decode_frame_bytes_strict_with_limits(bytes, &limits).ok()?;
    let pixels = frame.frame().to_rgba8().ok()?;
    let image = image::RgbaImage::from_raw(
        pixels.width.try_into().ok()?,
        pixels.height.try_into().ok()?,
        pixels.rgba,
    )?;
    Some(resize(
        cache,
        image.into(),
        size,
        image::metadata::Orientation::NoTransforms,
    ))
}

fn project_icon(cache: &ImageCache, bytes: &[u8]) -> Option<Image> {
    const SIZE: u32 = 48;
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Avif) => return avif_badge(cache, bytes, SIZE),
        Ok(_) => return thumbnail(cache, bytes, SIZE),
        Err(_) => {}
    }
    let xml = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}')
        .trim_start();
    if !xml.starts_with('<') {
        return None;
    }
    let (w, h, pixels) = rune::image::svg_rgba(xml.as_bytes(), SIZE).ok()?;
    Some(cache.rgba(w, h, pixels))
}

pub(crate) enum Kind {
    Registry,
    Preview,
    Thumbnail,
    Project { retryable: bool },
}

pub(crate) struct Job {
    pub watch: u64,
    pub key: String,
    pub operation: String,
    pub kind: Kind,
    pub data: Option<String>,
}

pub(crate) fn start(handle: rune::AppHandle<super::Desktop>) -> std::sync::mpsc::Sender<Job> {
    let (send, recv) = std::sync::mpsc::channel::<Job>();
    std::thread::spawn(move || {
        use base64::Engine;
        let cache = ImageCache::new(std::sync::Arc::new(|| {}));
        for job in recv {
            let image = job
                .data
                .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
                .and_then(|bytes| match job.kind {
                    Kind::Preview => Some(cache.load(bytes)),
                    Kind::Thumbnail => thumbnail(&cache, &bytes, 512),
                    Kind::Registry | Kind::Project { .. } => project_icon(&cache, &bytes),
                });
            let image = image.filter(|image| image.wait().is_ok());
            handle.update(move |s, _| {
                match job.kind {
                    Kind::Registry => {
                        s.registry_icon_result(job.watch, &job.key, &job.operation, image)
                    }
                    Kind::Project { retryable } => s.project_favicon_result(
                        job.watch,
                        &job.key,
                        &job.operation,
                        image,
                        retryable,
                    ),
                    _ => s.image_result(job.watch, &job.key, &job.operation, image),
                }
                s.write_request(
                    None,
                    super::Request::ImageHandled {
                        operation: job.operation,
                        watch: job.watch,
                    },
                );
            });
        }
    });
    send
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    #[test]
    fn a_large_screenshot_retains_only_thumbnail_pixels_in_the_timeline() {
        let source = DynamicImage::new_rgb8(2048, 1024);
        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, ImageFormat::Png).unwrap();
        let cache = ImageCache::new(std::sync::Arc::new(|| {}));
        let thumb = thumbnail(&cache, png.get_ref(), 512).unwrap();
        assert_eq!(thumb.wait(), Ok([512, 256]));
        let badge = project_icon(&cache, png.get_ref()).unwrap();
        assert_eq!(badge.wait(), Ok([48, 24]));
        let small = source.thumbnail(96, 48).into_rgba8();
        for format in [ImageFormat::Ico, ImageFormat::Tiff, ImageFormat::Bmp] {
            let mut file = Cursor::new(Vec::new());
            small.write_to(&mut file, format).unwrap();
            assert_eq!(
                project_icon(&cache, file.get_ref())
                    .unwrap_or_else(|| panic!("decoding {format:?}"))
                    .wait(),
                Ok([48, 24])
            );
        }
        let avif = include_bytes!("../testdata/project-icon.avif");
        assert_eq!(project_icon(&cache, avif).unwrap().wait(), Ok([32, 16]));
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 20"><circle cx="5" cy="5" r="4"/></svg>"#;
        assert_eq!(project_icon(&cache, svg).unwrap().wait(), Ok([24, 48]));
        assert!(project_icon(&cache, b"not an image").is_none());
    }
}
