use crate::{Desktop, image_decode};
use rune::{El, SurfaceFit, el, icon, row, tok};
use std::{
    cell::OnceCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Default)]
pub(super) struct Thumbnail(Rc<OnceCell<image_decode::Raster>>);

impl Thumbnail {
    pub(super) fn for_path(path: &Path) -> Option<Self> {
        matches!(
            image::ImageFormat::from_path(path),
            Ok(image::ImageFormat::Gif
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Png
                | image::ImageFormat::WebP)
        )
        .then(Self::default)
    }

    pub(super) fn tile(
        &self,
        path: &str,
        name: &str,
        theme: &tok::Theme,
        workspace: &crate::Workspace,
        remove: El,
    ) -> El {
        let mut tile = el()
            .key(format!("preview-file-{path}"))
            .size(64., 64.)
            .clip_rounded(8.)
            .radius(8.)
            .fill(theme.bg)
            .border(1., theme.separator)
            .center()
            .help(name);
        if let Some((raster, image)) = self.0.get().and_then(|r| Some((r, r.get()?.as_ref()?))) {
            let thumb = Arc::downgrade(raster);
            let path = PathBuf::from(path);
            let name = name.to_owned();
            tile = tile
                .pixels_fit(image.pixels(), SurfaceFit::Fill)
                .role("button")
                .focusable(true)
                .label(workspace.attachment_labels.preview.replace("{name}", &name))
                .on_press(move |s: &mut Desktop| {
                    if let Some(sender) = &s.image_jobs {
                        s.workspace.draft_image = Some(Preview {
                            name: name.clone(),
                            thumbnail: thumb.clone(),
                            full: image_decode::local(sender, &path, 4096),
                        });
                    }
                });
        } else {
            tile = tile
                .role("image")
                .label(name)
                .child(icon(&rune_kit::glyphs::FILE_TEXT, 16.).color(theme.text_secondary));
            if self.0.get().is_none() {
                let slot = self.0.clone();
                let viewport = workspace.attachment_viewport.clone();
                let path = PathBuf::from(path);
                tile = tile.on_drawn_layout_tx(move |s: &mut Desktop, [x, y, w, h], tx| {
                    let [left, top, width, height] = viewport.get();
                    if x < left + width
                        && x + w > left
                        && y < top + height
                        && y + h > top
                        && let Some(sender) = &s.image_jobs
                    {
                        slot.get_or_init(|| image_decode::local(sender, &path, 128));
                    }
                    tx.live_only();
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
    full: image_decode::Raster,
}

impl Preview {
    pub(crate) fn dialog(&self, theme: &tok::Theme) -> Option<El> {
        let thumb = self.thumbnail.upgrade()?;
        let image = self
            .full
            .get()
            .and_then(Option::as_ref)
            .or_else(|| thumb.get().and_then(Option::as_ref))?;
        Some(crate::workspace::images::dialog(
            &self.name,
            image,
            row().self_end(),
            theme,
        ))
    }
}
