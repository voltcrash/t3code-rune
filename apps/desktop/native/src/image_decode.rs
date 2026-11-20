use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use rune::{Image, ImageCache};
use std::io::Cursor;

fn thumbnail(cache: &ImageCache, bytes: &[u8]) -> Option<Image> {
    // Decode one source at a time. Keep only the 2× thumbnail in the timeline cache.
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
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    if image.width() > 512 || image.height() > 512 {
        image = image.thumbnail(512, 512);
    }
    image.apply_orientation(orientation);
    let pixels = image.into_rgba8();
    Some(cache.rgba(pixels.width(), pixels.height(), pixels.into_raw()))
}

pub(crate) struct Job {
    pub watch: u64,
    pub key: String,
    pub operation: String,
    pub full: bool,
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
                .and_then(|bytes| {
                    if job.full {
                        Some(cache.load(bytes))
                    } else {
                        thumbnail(&cache, &bytes)
                    }
                });
            let image = image.filter(|image| image.wait().is_ok());
            handle.update(move |s, _| {
                s.image_result(job.watch, &job.key, &job.operation, image);
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
        let thumb = thumbnail(&cache, png.get_ref()).unwrap();
        assert_eq!(thumb.wait(), Ok([512, 256]));
    }
}
