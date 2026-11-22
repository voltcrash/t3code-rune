use crate::Desktop;
use rune::{El, FileDialogOptions, Icon, TextStyle, col, el, icon, row, text, tok};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::TempPath;

pub(super) struct Selection {
    pub(super) path: String,
    _clipboard_file: Option<TempPath>,
}

impl Selection {
    pub(super) fn local(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            _clipboard_file: None,
        }
    }

    fn png(bytes: &[u8]) -> io::Result<Self> {
        let mut file = tempfile::Builder::new()
            .prefix("image-")
            .suffix(".png")
            .tempfile()?;
        file.write_all(bytes)?;
        let file = file.into_temp_path();
        let path = file.to_str().ok_or(io::ErrorKind::InvalidData)?.to_owned();
        Ok(Self {
            path,
            _clipboard_file: Some(file),
        })
    }
}

pub(super) fn paths(files: &[Selection]) -> Vec<String> {
    files.iter().map(|file| file.path.clone()).collect()
}

const FILE_LIMIT: usize = 100;
const TOO_MANY: &str = "You can attach up to 100 files per message.";

#[derive(Clone)]
pub(super) enum Target {
    Thread(String),
    Project(String),
}

impl Target {
    fn edit(&self, app: &mut Desktop, change: impl FnOnce(&mut Vec<Selection>)) {
        if app.pending || !app.can_operate {
            return;
        }
        match self {
            Self::Project(id) if app.new_threads.active.as_ref() == Some(id) => {
                if let Some(files) = app.new_threads.editable_files() {
                    change(files)
                }
            }
            Self::Thread(id)
                if app.new_threads.active.is_none()
                    && !app.workspace.show_projects
                    && !app.workspace.questions.contains_key(id)
                    && app
                        .workspace
                        .conversation
                        .as_ref()
                        .is_some_and(|c| c.id == *id) =>
            {
                if let Some(draft) = app.workspace.drafts.get_mut(id) {
                    draft.edit_files(change)
                }
            }
            _ => {}
        }
    }

    fn add(&self, app: &mut Desktop, paths: &[PathBuf]) {
        let mut error = None;
        self.edit(app, |files| {
            for path in paths {
                let Some(path) = path.to_str() else {
                    error = Some("Choose a file whose name can be displayed as text.");
                    continue;
                };
                if files.iter().any(|existing| existing.path == path) {
                    continue;
                }
                if files.len() >= FILE_LIMIT {
                    error = Some(TOO_MANY);
                    break;
                }
                files.push(Selection::local(path));
            }
        });
        if let Some(error) = error {
            app.error = Some(error.into())
        }
    }

    pub(super) fn paste(&self, app: &mut Desktop, content: &rune::ClipboardContent) -> bool {
        let paths = content.files();
        let image = content.image();
        if !paths.is_empty() {
            let contains_image = paths.iter().any(|p| {
                image::ImageFormat::from_path(p).is_ok()
                    || p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
            });
            if image.is_none() && !contains_image && !content.text().is_empty() {
                return false;
            }
            self.add(app, paths);
            return true;
        }
        let Some(image) = image else {
            return false;
        };
        let mut error = None;
        self.edit(app, |files| {
            if files.len() >= FILE_LIMIT {
                error = Some(TOO_MANY.to_owned());
            } else {
                match Selection::png(image.png()) {
                    Ok(file) => files.push(file),
                    Err(e) => error = Some(e.to_string()),
                }
            }
        });
        if error.is_some() {
            app.error = error;
        }
        true
    }

    pub(super) fn button(&self, theme: &tok::Theme, locked: bool) -> El {
        let target = self.clone();
        el().key("attach-files")
            .label("Attach files")
            .help("Attach files")
            .role("button")
            .size(28., 28.)
            .radius(6.)
            .center()
            .hover(theme.text.alpha(0.08))
            .child(icon(&PAPERCLIP, 16.).color(theme.text_secondary))
            .disabled(locked)
            .focusable(!locked)
            .on_press_tx(move |app: &mut Desktop, tx| {
                let generation = app.request_id;
                let target = target.clone();
                tx.open_file(
                    FileDialogOptions {
                        title: "Attach files".into(),
                        ..Default::default()
                    },
                    move |app: &mut Desktop, result, _| {
                        if generation != app.request_id {
                            return;
                        }
                        match result {
                            Ok(Some(path)) => target.add(app, &[path]),
                            Ok(None) => {}
                            Err(_) => app.error = Some("Couldn’t open the file picker.".into()),
                        }
                    },
                );
            })
    }

    pub(super) fn drop_area(self, surface: El, locked: bool) -> El {
        if locked {
            surface
        } else {
            surface.on_drop_files(move |app: &mut Desktop, paths, _| self.add(app, paths))
        }
    }

    pub(super) fn list(&self, files: &[Selection], theme: &tok::Theme, locked: bool) -> Option<El> {
        if files.is_empty() {
            return None;
        }
        let mut list = col()
            .key("composer-files")
            .w_full()
            .h((files.len().min(4) * 28) as f32)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            });
        for file in files {
            let path = &file.path;
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            let remove = format!("Remove {name}");
            let path = path.clone();
            let target = self.clone();
            list = list.child(
                row()
                    .w_full()
                    .h(28.)
                    .gap(8.)
                    .center_y()
                    .child(icon(&rune_kit::glyphs::FILE_TEXT, 16.).color(theme.text_secondary))
                    .child(
                        text(name)
                            .font(TextStyle::new(14., 400.))
                            .color(theme.text)
                            .min_w(0.)
                            .grow(1.)
                            .truncate(),
                    )
                    .child(
                        el().key(format!("remove-file-{path}"))
                            .role("button")
                            .label(&remove)
                            .help(&remove)
                            .size(22., 22.)
                            .radius(5.)
                            .center()
                            .hover(theme.text.alpha(0.08))
                            .child(icon(&REMOVE, 14.).color(theme.text_secondary))
                            .disabled(locked)
                            .focusable(!locked)
                            .on_press(move |app: &mut Desktop| {
                                target.edit(app, |files| files.retain(|f| f.path != path));
                            }),
                    ),
            );
        }
        Some(list)
    }
}

const PAPERCLIP: Icon = Icon {
    name: "paperclip",
    view: 24.,
    body: r#"<path d="m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const REMOVE: Icon = Icon {
    name: "remove-attachment",
    view: 24.,
    body: r#"<path d="m6 6 12 12M18 6 6 18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};

#[cfg(test)]
mod tests {
    use super::*;
    use rune::ClipboardContent;

    #[test]
    fn paste_keeps_text_editable_and_stages_images_only_in_the_active_writable_draft() {
        let mut app = Desktop::new();
        app.can_operate = true;
        app.new_threads
            .configure("project".into(), vec![], Default::default(), false, "main");
        let target = Target::Project("project".into());
        let mixed = ClipboardContent::new("share this").with_files([PathBuf::from("notes.txt")]);
        assert!(!target.paste(&mut app, &mixed));
        assert!(app.new_threads.editable_files().unwrap().is_empty());

        let mut png = io::Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let image = rune::ClipboardImage::from_png(bytes.clone()).unwrap();
        let clipboard = ClipboardContent::new("").with_image(image);
        app.can_operate = false;
        target.paste(&mut app, &clipboard);
        assert!(app.new_threads.editable_files().unwrap().is_empty());
        app.can_operate = true;
        assert!(target.paste(&mut app, &clipboard));
        let files = app.new_threads.editable_files().unwrap();
        let path = PathBuf::from(&files[0].path);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        files.clear();
        assert!(!path.exists());
    }
}
