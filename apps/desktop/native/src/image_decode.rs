use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use rune::ImageCache;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak, mpsc};

fn thumbnail(bytes: &[u8], size: u32) -> Option<image::RgbaImage> {
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
    Some(resize(image, size, orientation))
}

fn resize(
    mut image: DynamicImage,
    size: u32,
    orientation: image::metadata::Orientation,
) -> image::RgbaImage {
    if image.width() > size || image.height() > size {
        image = image.thumbnail(size, size);
    }
    image.apply_orientation(orientation);
    image.into_rgba8()
}

fn avif_badge(bytes: &[u8], size: u32) -> Option<image::RgbaImage> {
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
        image.into(),
        size,
        image::metadata::Orientation::NoTransforms,
    ))
}

fn badge(bytes: &[u8]) -> Option<image::RgbaImage> {
    const SIZE: u32 = 48;
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Avif) => return avif_badge(bytes, SIZE),
        Ok(_) => return thumbnail(bytes, SIZE),
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
    image::RgbaImage::from_raw(w, h, pixels)
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

pub(crate) type Raster = Arc<OnceLock<Option<rune::Image>>>;

pub(crate) enum Request {
    Remote(Job),
    Local {
        path: PathBuf,
        edge: u32,
        result: Weak<OnceLock<Option<rune::Image>>>,
    },
}

pub(crate) fn local(sender: &mpsc::Sender<Request>, path: &Path, edge: u32) -> Raster {
    let result = Raster::default();
    let _ = sender.send(Request::Local {
        path: path.to_owned(),
        edge,
        result: Arc::downgrade(&result),
    });
    result
}

fn local_thumbnail(path: &Path, size: u32) -> Option<image::RgbaImage> {
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    thumbnail(&bytes, size)
}

pub(crate) fn start(handle: rune::AppHandle<super::Desktop>, recv: mpsc::Receiver<Request>) {
    std::thread::spawn(move || {
        use base64::Engine;
        let cache = ImageCache::new(Arc::new(|| {}));
        for request in recv {
            let job = match request {
                Request::Local { path, edge, result } => {
                    if result.strong_count() == 0 {
                        continue;
                    }
                    let image = local_thumbnail(&path, edge)
                        .map(|pixels| {
                            cache.rgba(pixels.width(), pixels.height(), pixels.into_raw())
                        })
                        .filter(|image| image.wait().is_ok());
                    handle.update_if(move |_, _| {
                        result.upgrade().is_some_and(|slot| slot.set(image).is_ok())
                    });
                    continue;
                }
                Request::Remote(job) => job,
            };
            let mut mask = None;
            let cache_pixels = |pixels: image::RgbaImage| {
                cache.rgba(pixels.width(), pixels.height(), pixels.into_raw())
            };
            let image = job
                .data
                .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
                .and_then(|bytes| match job.kind {
                    Kind::Preview => Some(cache.load(bytes)),
                    Kind::Thumbnail => thumbnail(&bytes, 512).map(cache_pixels),
                    Kind::Project { .. } => badge(&bytes).map(cache_pixels),
                    Kind::Registry => {
                        mask = badge(&bytes);
                        None
                    }
                });
            let image = image.filter(|image| image.wait().is_ok());
            handle.update(move |s, _| {
                match job.kind {
                    Kind::Registry => {
                        s.registry_icon_result(job.watch, &job.key, &job.operation, mask)
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
        let thumb = thumbnail(png.get_ref(), 512).unwrap();
        assert_eq!(thumb.dimensions(), (512, 256));
        assert_eq!(badge(png.get_ref()).unwrap().dimensions(), (48, 24));
        let small = source.thumbnail(96, 48).into_rgba8();
        for format in [ImageFormat::Ico, ImageFormat::Tiff, ImageFormat::Bmp] {
            let mut file = Cursor::new(Vec::new());
            small.write_to(&mut file, format).unwrap();
            assert_eq!(
                badge(file.get_ref())
                    .unwrap_or_else(|| panic!("decoding {format:?}"))
                    .dimensions(),
                (48, 24)
            );
        }
        let avif = include_bytes!("../testdata/project-icon.avif");
        assert_eq!(badge(avif).unwrap().dimensions(), (32, 16));
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 20"><circle cx="5" cy="5" r="4"/></svg>"#;
        assert_eq!(badge(svg).unwrap().dimensions(), (24, 48));
        assert!(badge(b"not an image").is_none());
    }
}
