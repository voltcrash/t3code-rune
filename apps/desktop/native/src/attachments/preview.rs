use crate::{Desktop, image_decode};
use rune::{El, SurfaceFit, el, icon, row, tok};
use std::{
    cell::{Cell, OnceCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Default)]
pub(super) struct Thumbnail(Rc<Tile>);

#[derive(Default)]
struct Tile {
    raster: OnceCell<image_decode::Raster>,
    visible: Cell<bool>,
}

impl Thumbnail {
    pub(super) fn for_path(path: &Path) -> Option<Self> {
        // JavaScript trims BOM and retains NEXT LINE when the uploader looks up the filename.
        let name = path
            .file_name()?
            .to_str()?
            .trim_matches(|c: char| c == '\u{feff}' || (c != '\u{85}' && c.is_whitespace()));
        let extension = name.rsplit('.').next()?.to_ascii_lowercase();
        matches!(
            extension.as_str(),
            "gif" | "jpe" | "jpeg" | "jpg" | "png" | "webp"
        )
        .then(Self::default)
    }

    pub(super) fn tile(
        &self,
        path: &str,
        name: &str,
        theme: &tok::Theme,
        app: &Desktop,
        remove: El,
    ) -> El {
        let workspace = &app.workspace;
        if self.0.visible.get()
            && let Some(queue) = &app.image_jobs
        {
            image_decode::queue_local(&self.0.raster, queue, Path::new(path), 128);
        }
        let mut tile = el()
            .key(format!("preview-file-{path}"))
            .size(64., 64.)
            .clip_rounded(8.)
            .radius(8.)
            .fill(theme.bg)
            .border(1., theme.separator)
            .center()
            .help(name);
        if let Some((raster, image)) = self
            .0
            .raster
            .get()
            .and_then(|r| Some((r, r.get()?.as_ref()?)))
        {
            let thumb = Arc::downgrade(raster);
            let path = PathBuf::from(path);
            let name = name.to_owned();
            tile = tile
                .pixels_fit(image.pixels(), SurfaceFit::Fill)
                .role("button")
                .focusable(true)
                .label(workspace.attachment_labels.preview.replace("{name}", &name))
                .on_press(move |s: &mut Desktop| {
                    *s.workspace.draft_image.get_mut() = Some(Preview {
                        name: name.clone(),
                        path: path.clone(),
                        thumbnail: thumb.clone(),
                        full: OnceCell::new(),
                    });
                });
        } else {
            tile = tile
                .role("image")
                .label(name)
                .child(icon(&rune_kit::glyphs::FILE_TEXT, 16.).color(theme.text_secondary));
            if self.0.raster.get().is_none() {
                let slot = self.0.clone();
                let viewport = workspace.attachment_viewport.clone();
                tile = tile.on_drawn_layout_tx(move |_: &mut Desktop, [x, y, w, h], tx| {
                    let [left, top, width, height] = viewport.get();
                    let visible =
                        x < left + width && x + w > left && y < top + height && y + h > top;
                    if slot.visible.replace(visible) == visible {
                        tx.live_only();
                    }
                });
            }
        }
        el().size(64., 64.)
            .child(tile)
            .child(remove.absolute(39., 3.).fill(theme.bg.alpha(0.92)))
    }
}
pub(crate) struct Preview {
    name: String,
    thumbnail: std::sync::Weak<std::sync::OnceLock<Option<rune::Image>>>,
    path: PathBuf,
    full: OnceCell<image_decode::Raster>,
}

impl Preview {
    pub(super) fn dialog(
        &self,
        files: &[super::Selection],
        app: &Desktop,
        theme: &tok::Theme,
    ) -> Option<El> {
        let thumb = self.thumbnail.upgrade()?;
        if !files
            .iter()
            .filter_map(|f| f.thumbnail.as_ref()?.0.raster.get())
            .any(|r| Arc::ptr_eq(r, &thumb))
        {
            return None;
        }
        if let Some(queue) = &app.image_jobs {
            image_decode::queue_local(&self.full, queue, &self.path, 4096);
        }
        let image = self
            .full
            .get()
            .and_then(|r| r.get())
            .and_then(Option::as_ref)
            .or_else(|| thumb.get().and_then(Option::as_ref))?;
        Some(crate::workspace::images::dialog(
            &self.name,
            image,
            row().self_end(),
            &app.workspace.attachment_labels.close,
            theme,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::Thumbnail;

    #[test]
    fn image_names_match_the_worker_upload_types() {
        for name in [
            "x.jpe",
            "photo.jpe\u{feff}",
            "x.JPEG ",
            ".png",
            "GIF",
            "image.webp",
        ] {
            assert!(
                Thumbnail::for_path(std::path::Path::new(name)).is_some(),
                "{name}"
            );
        }
        for name in [
            "x.jfif",
            "photo.jpg\u{85}",
            "x.apng",
            "photo.png.zip",
            "icon.svg",
            "notes",
        ] {
            assert!(
                Thumbnail::for_path(std::path::Path::new(name)).is_none(),
                "{name}"
            );
        }
    }
}
