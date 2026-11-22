use super::{Desktop, Request, action, label};
use rune::{El, Image, col, el, tok};
use serde::Deserialize;
use std::{collections::HashMap, rc::Rc};
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Attachment {
    pub(super) name: String,
    pub(super) image: Option<String>,
}

struct Preview {
    key: String,
    name: String,
    operation: String,
    image: Option<Image>,
    failed: bool,
}

struct Entry {
    operation: String,
    order: u64,
    mounted: Rc<()>,
    loaded: bool,
    image: Option<Image>,
}

#[derive(Default)]
pub(super) struct Images {
    entries: HashMap<String, Entry>,
    next: u64,
    expanded: Option<Preview>,
}

impl Desktop {
    fn load_image(&mut self, watch: u64, key: String, retry: bool, mounted: Rc<()>) {
        let Some(pane) = self.workspace.conversation.as_mut() else {
            return;
        };
        if pane.watch != Some(watch) || (!retry && pane.images.entries.contains_key(&key)) {
            return;
        }
        let mut evicted = None;
        if pane.images.entries.len() >= 12 && !pane.images.entries.contains_key(&key) {
            let oldest = pane
                .images
                .entries
                .iter()
                .filter(|(k, entry)| {
                    // A mounted thumbnail holds the other reference, including another row showing this image.
                    Rc::strong_count(&entry.mounted) == 1
                        && !pane
                            .images
                            .expanded
                            .as_ref()
                            .is_some_and(|preview| preview.key == **k)
                })
                .min_by_key(|(_, e)| e.order)
                .map(|(key, _)| key.clone());
            if let Some(old) = oldest
                && let Some(e) = pane.images.entries.remove(&old)
                && !e.loaded
            {
                evicted = Some((old, e.operation));
            }
        }
        pane.images.next += 1;
        let operation = Uuid::new_v4().to_string();
        pane.images.entries.insert(
            key.clone(),
            Entry {
                operation: operation.clone(),
                order: pane.images.next,
                mounted,
                loaded: false,
                image: None,
            },
        );
        pane.revision += 1;
        if let Some((key, operation)) = evicted {
            self.write_request(
                None,
                Request::InspectImage {
                    full: false,
                    watch,
                    key,
                    operation,
                    open: false,
                },
            );
        }
        self.write_request(
            None,
            Request::InspectImage {
                full: false,
                watch,
                key,
                operation,
                open: true,
            },
        );
    }

    pub(crate) fn image_result(
        &mut self,
        watch: u64,
        key: &str,
        operation: &str,
        image: Option<Image>,
    ) {
        if let Some(pane) = self.workspace.conversation.as_mut()
            && pane.watch == Some(watch)
        {
            if let Some(preview) = pane.images.expanded.as_mut()
                && preview.operation == operation
                && preview.key == key
            {
                preview.failed = image.is_none();
                preview.image = image;
            } else if let Some(entry) = pane.images.entries.get_mut(key)
                && entry.operation == operation
            {
                entry.loaded = true;
                entry.image = image;
                pane.revision += 1;
            }
        }
    }

    fn expand_image(&mut self, watch: u64, key: String, name: String) {
        let Some(pane) = self.workspace.conversation.as_mut() else {
            return;
        };
        if pane.watch != Some(watch) {
            return;
        }
        let operation = Uuid::new_v4().to_string();
        pane.images.expanded = Some(Preview {
            key: key.clone(),
            name,
            operation: operation.clone(),
            image: None,
            failed: false,
        });
        self.write_request(
            None,
            Request::InspectImage {
                watch,
                key,
                operation,
                full: true,
                open: true,
            },
        );
    }

    pub(crate) fn close_image(&mut self) -> bool {
        if self.workspace.draft_image.take().is_some() {
            return true;
        }
        if let Some(pane) = self.workspace.conversation.as_mut()
            && let Some(preview) = pane.images.expanded.take()
        {
            if let Some(watch) = pane.watch {
                self.write_request(
                    None,
                    Request::InspectImage {
                        watch,
                        key: preview.key,
                        operation: preview.operation,
                        full: true,
                        open: false,
                    },
                );
            }
            return true;
        }
        false
    }
}

pub(super) fn thumbnail(
    app: &Desktop,
    key: &str,
    name: &str,
    theme: &tok::Theme,
    max_height: f32,
) -> El {
    let pane = app.workspace.conversation.as_ref().unwrap();
    let entry = pane.images.entries.get(key);
    let image = entry.and_then(|e| e.image.as_ref());
    let failed =
        entry.is_some_and(|e| e.loaded && e.image.as_ref().is_none_or(|i| i.error().is_some()));
    let ready = image.and_then(Image::size);
    let mut thumb = el()
        .key(format!(
            "thumbnail-{}-{key}",
            pane.watch.unwrap_or_default()
        ))
        .role("button")
        .no_window_drag()
        .radius(8.)
        .clip_rounded(8.)
        .fill(theme.text.alpha(0.05))
        .center();
    let w = 256.;
    let [width, height] = ready
        .map(|[x, y]| {
            let scale = (w / x as f32).min(max_height / y as f32).min(1.);
            [x as f32 * scale, y as f32 * scale]
        })
        .unwrap_or([152., 72.]);
    thumb = thumb.w_full().max_w(width).aspect_ratio(width / height);
    if let Some(watch) = pane.watch {
        let mounted = entry.map(|e| e.mounted.clone()).unwrap_or_default();
        let visible = mounted.clone();
        let id = key.to_owned();
        thumb = thumb.on_visible(move |s: &mut Desktop, show, _| {
            if show {
                s.load_image(watch, id.clone(), false, visible.clone());
            }
        });
        if failed || ready.is_some() {
            let id = key.to_owned();
            let title = name.to_owned();
            thumb = thumb
                .focusable(true)
                .label(if failed {
                    format!("Retry {name}")
                } else {
                    format!("Open {name}")
                })
                .on_press(move |s: &mut Desktop| {
                    if failed {
                        s.load_image(watch, id.clone(), true, mounted.clone());
                    } else {
                        s.expand_image(watch, id.clone(), title.clone());
                    }
                });
        }
    }
    if let Some(image) = image
        && ready.is_some()
    {
        thumb.pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
    } else {
        thumb.child(
            label(
                if failed { "Retry image" } else { name },
                12.,
                400.,
                theme.text_secondary,
            )
            .wrap(),
        )
    }
}

pub(super) fn overlay(app: &Desktop, theme: &tok::Theme, overlays: &mut Vec<El>) {
    if let Some(preview) = app
        .workspace
        .draft_image
        .as_ref()
        .and_then(|p| p.dialog(theme))
    {
        overlays.push(preview);
        return;
    }
    let Some(pane) = app.workspace.conversation.as_ref() else {
        return;
    };
    let Some(preview) = &pane.images.expanded else {
        return;
    };
    let name = &preview.name;
    let Some(image) = preview
        .image
        .as_ref()
        .filter(|i| i.size().is_some())
        .or_else(|| {
            pane.images
                .entries
                .get(&preview.key)
                .and_then(|e| e.image.as_ref())
        })
    else {
        return;
    };
    let mut controls = rune::row().self_end().gap(8.);
    if (preview.failed || preview.image.as_ref().is_some_and(|i| i.error().is_some()))
        && let Some(watch) = pane.watch
    {
        let key = preview.key.clone();
        let name = name.clone();
        controls = controls.child(action("Retry full image", theme, true, move |s| {
            s.expand_image(watch, key.clone(), name.clone())
        }));
    }
    overlays.push(dialog(name, image, controls, theme));
}

pub(crate) fn dialog(name: &str, image: &Image, controls: El, theme: &tok::Theme) -> El {
    let close = action("Close image preview", theme, true, |s| {
        s.close_image();
    });
    col()
        .absolute(0., 0.)
        .w_full()
        .h_full()
        .padding(20.)
        .gap(10.)
        .key("image-preview")
        .label(name)
        .role("dialog")
        .fill(theme.bg.alpha(0.96))
        .child(controls.child(close))
        .child(
            el().grow(1.)
                .min_h(0.)
                .w_full()
                .label(name)
                .role("image")
                .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
        )
        .modal(true, |s: &mut Desktop| {
            s.close_image();
        })
}
