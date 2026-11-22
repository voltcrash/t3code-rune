mod preview;
use crate::Desktop;
pub(crate) use preview::Preview;
use rune::{El, FileDialogOptions, Icon, TextStyle, col, el, icon, row, text, tok};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tempfile::TempPath;

#[derive(serde::Deserialize)]
pub(crate) struct Labels {
    preview: String,
    remove: String,
    pub(crate) close: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            preview: "Preview {name}".into(),
            remove: "Remove {name}".into(),
            close: "Close image preview".into(),
        }
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(transparent)]
pub(super) struct Selection {
    pub(super) path: String,
    #[serde(skip)]
    clipboard_file: Option<Rc<TempPath>>,
    #[serde(skip)]
    thumbnail: Option<preview::Thumbnail>,
}

impl Selection {
    pub(super) fn local(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            clipboard_file: None,
            thumbnail: preview::Thumbnail::for_path(Path::new(path)),
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
            clipboard_file: Some(Rc::new(file)),
            ..Self::local(&path)
        })
    }
}

impl Desktop {
    pub(super) fn hold_clipboard_files(&mut self, id: u64, request: &crate::Request<'_>) {
        let (crate::Request::LaunchThread { files, .. }
        | crate::Request::SendMessage { files, .. }) = request
        else {
            return;
        };
        let leases: Vec<_> = files
            .iter()
            .filter_map(|f| f.clipboard_file.clone())
            .collect();
        if !leases.is_empty() {
            self.clipboard_uploads.insert(id, leases);
        }
    }
}

fn describes_paths(text: &str, paths: &[PathBuf]) -> bool {
    let mut lines = text.lines();
    paths.iter().all(|path| {
        lines.next().is_some_and(|line| {
            path == Path::new(line)
                || url::Url::parse(line)
                    .ok()
                    .and_then(|url| url.to_file_path().ok())
                    .as_ref()
                    == Some(path)
        })
    }) && lines.next().is_none()
}

const FILE_LIMIT: usize = 100;
const TOO_MANY: &str = "You can attach up to 100 files per message.";

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Target {
    Thread(String),
    Project(String),
}

pub(super) struct PendingPaste {
    target: Target,
    request: u64,
    batch: u64,
    reads: usize,
    submit: bool,
}

impl Desktop {
    fn begin_plain_paste(&mut self, target: &Target) -> u64 {
        if let Some(paste) = &mut self.clipboard_paste
            && paste.request == self.request_id
            && paste.target == *target
        {
            paste.reads += 1;
            return paste.batch;
        }
        let batch = self.next_request_id();
        self.clipboard_paste = Some(PendingPaste {
            target: target.clone(),
            request: self.request_id,
            batch,
            reads: 1,
            submit: false,
        });
        batch
    }

    fn finish_plain_paste(&mut self, batch: u64, active: bool) -> Option<bool> {
        let paste = self.clipboard_paste.as_mut().filter(|p| p.batch == batch)?;
        if !active || self.request_id != paste.request {
            self.clipboard_paste = None;
            return None;
        }
        paste.reads -= 1;
        if paste.reads > 0 {
            return Some(false);
        }
        self.clipboard_paste.take().map(|p| p.submit)
    }

    pub(super) fn submit_after_paste(&mut self) -> bool {
        if self
            .clipboard_paste
            .as_ref()
            .is_some_and(|p| p.request == self.request_id && p.target.active(self))
        {
            self.clipboard_paste.as_mut().unwrap().submit = true;
            return true;
        }
        false
    }
}

impl Target {
    fn active(&self, app: &Desktop) -> bool {
        match self {
            Self::Project(id) => app.new_threads.active.as_ref() == Some(id),
            Self::Thread(id) => {
                app.new_threads.active.is_none()
                    && !app.workspace.show_projects
                    && !app.workspace.questions.contains_key(id)
                    && app
                        .workspace
                        .conversation
                        .as_ref()
                        .is_some_and(|c| c.id == *id)
            }
        }
    }

    pub(super) fn input(&self, field: El) -> El {
        let paste = self.clone();
        let plain = self.clone();
        field
            .on_input_paste(move |app: &mut Desktop, content, _| paste.paste(app, content))
            .on_input_key(move |app: &mut Desktop, key, tx| {
                const PASTE_AS_TEXT: rune::Chord = rune::Chord::new(rune::ChordKey::Char('v'))
                    .with(rune::ChordMod::Primary)
                    .with(rune::ChordMod::Shift);
                if !PASTE_AS_TEXT.matches_tx(key, tx) {
                    return false;
                }
                let target = plain.clone();
                let focus = tx.focused().map(str::to_owned);
                let batch = app.begin_plain_paste(&target);
                tx.read_clipboard(move |app: &mut Desktop, content, tx| {
                    let active =
                        !app.pending && tx.focused() == focus.as_deref() && target.active(app);
                    let Some(submit) = app.finish_plain_paste(batch, active) else {
                        return;
                    };
                    if content.text().is_empty() {
                        if !target.paste(app, content) {
                            return;
                        }
                    } else {
                        tx.paste_text(content.text());
                    }
                    if submit {
                        tx.send_key(rune::KeyEvent {
                            key: rune::Key::Enter,
                            mods: Default::default(),
                        });
                    }
                });
                true
            })
    }

    fn edit(&self, app: &mut Desktop, change: impl FnOnce(&mut Vec<Selection>)) {
        if app.pending || !app.can_operate || !self.active(app) {
            return;
        }
        match self {
            Self::Project(_) => {
                if let Some(files) = app.new_threads.editable_files() {
                    change(files)
                }
            }
            Self::Thread(id) => {
                if let Some(draft) = app.workspace.drafts.get_mut(id) {
                    draft.edit_files(change)
                }
            }
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
            if !contains_image
                && !content.text().is_empty()
                && !describes_paths(content.text(), paths)
            {
                return false;
            }
            self.add(app, paths);
            return true;
        }
        let Some(image) = image.filter(|_| content.text().is_empty()) else {
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

    pub(super) fn list(
        &self,
        files: &[Selection],
        theme: &tok::Theme,
        locked: bool,
        app: &Desktop,
        dialog: &mut Option<El>,
    ) -> Option<El> {
        if files.is_empty() {
            return None;
        }
        let workspace = &app.workspace;
        *dialog = workspace
            .draft_image
            .borrow()
            .as_ref()
            .and_then(|p| p.dialog(files, app, theme));
        // A keyed group keeps scrolled tiles under one accessible parent.
        let mut tiles = row()
            .key("composer-image-grid")
            .w_full()
            .flex_wrap()
            .gap(8.);
        let mut rows = col().key("composer-file-rows").w_full();
        let mut documents = 0;
        for file in files {
            let path = &file.path;
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            let caption = workspace.attachment_labels.remove.replace("{name}", name);
            let target = self.clone();
            let remove_path = path.clone();
            let remove = el()
                .key(format!("remove-file-{path}"))
                .role("button")
                .label(&caption)
                .help(&caption)
                .size(22., 22.)
                .radius(5.)
                .center()
                .hover(theme.text.alpha(0.08))
                .child(icon(&REMOVE, 14.).color(theme.text_secondary))
                .disabled(locked)
                .focusable(!locked)
                .on_press(move |app: &mut Desktop| {
                    target.edit(app, |files| files.retain(|f| f.path != remove_path))
                });
            if let Some(thumb) = &file.thumbnail {
                tiles = tiles.child(thumb.tile(path, name, theme, app, remove));
            } else {
                documents += 1;
                rows = rows.child(
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
                        .child(remove),
                );
            }
        }
        let mut list = col()
            .key("composer-files")
            .drawn_rect_into(&workspace.attachment_viewport)
            .w_full()
            .max_h(if documents == files.len() { 112. } else { 136. })
            .gap(4.)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            });
        if documents < files.len() {
            list = list.child(tiles);
        }
        if documents > 0 {
            list = list.child(rows);
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
    fn submit_waits_for_all_clipboard_reads_from_the_current_composer() {
        let mut app = Desktop::new();
        app.new_threads
            .configure("project".into(), vec![], Default::default(), false, "main");
        let project = Target::Project("project".into());
        let first = app.begin_plain_paste(&project);
        assert_eq!(first, app.begin_plain_paste(&project));
        assert!(app.submit_after_paste());
        assert_eq!(app.finish_plain_paste(first, true), Some(false));
        assert_eq!(app.finish_plain_paste(first, true), Some(true));
        assert!(!app.submit_after_paste());

        let old = app.begin_plain_paste(&project);
        assert!(app.submit_after_paste());
        app.request_id = app.next_request_id();
        assert!(!app.submit_after_paste());
        let new = app.begin_plain_paste(&project);
        assert_eq!(app.finish_plain_paste(old, true), None);
        assert!(app.submit_after_paste());
        assert_eq!(app.finish_plain_paste(new, false), None);
        assert!(!app.submit_after_paste());
    }

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

        let pdf = std::env::temp_dir().join("copied report.pdf");
        let uri = url::Url::from_file_path(&pdf).unwrap();
        let copied = ClipboardContent::new(format!("{uri}\r\n")).with_files([pdf.clone()]);
        assert!(target.paste(&mut app, &copied));
        let files = app.new_threads.editable_files().unwrap();
        assert_eq!(files[0].path, pdf.to_str().unwrap());
        files.clear();

        let mut png = io::Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let image = rune::ClipboardImage::from_png(bytes.clone()).unwrap();
        let with_text = ClipboardContent::new("editable cell contents").with_image(image.clone());
        assert!(!target.paste(&mut app, &with_text));
        assert!(app.new_threads.editable_files().unwrap().is_empty());
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

    #[test]
    fn a_pruned_draft_keeps_its_clipboard_file_until_the_upload_reply() {
        let (_reader, writer) = std::io::pipe().unwrap();
        #[cfg(unix)]
        let writer = std::os::fd::OwnedFd::from(writer);
        #[cfg(windows)]
        let writer = std::os::windows::io::OwnedHandle::from(writer);
        let mut app = Desktop::new();
        app.output = Some(writer.into());
        app.new_threads
            .configure("project".into(), vec![], Default::default(), false, "main");
        let file = Selection::png(b"staged clipboard bytes").unwrap();
        let path = PathBuf::from(&file.path);
        let files = app.new_threads.editable_files().unwrap();
        files.push(file);
        let request = crate::Request::LaunchThread {
            project: "project".into(),
            model: "model".into(),
            runtime: "runtime".into(),
            runtime_override: None,
            text: String::new(),
            files: files.clone(),
            operation: "launch".into(),
            base: None,
        };
        app.send(request);
        let id = app.request_id;
        app.new_threads.forget("project");
        app.environment = Some("old-environment".into());
        app.receive(crate::WorkerMessage {
            id: None,
            reply: crate::Reply::LocalClosed,
        });
        assert!(app.environment.is_none());
        app.receive(crate::WorkerMessage {
            id: Some(id + 1),
            reply: crate::Reply::Unchanged,
        });
        assert_eq!(std::fs::read(&path).unwrap(), b"staged clipboard bytes");
        app.receive(crate::WorkerMessage {
            id: Some(id),
            reply: crate::Reply::Unchanged,
        });
        assert!(!path.exists());
    }
}
