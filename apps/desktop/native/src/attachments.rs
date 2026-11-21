use crate::Desktop;
use rune::{El, FileDialogOptions, Icon, TextStyle, col, el, icon, row, text, tok};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(super) enum Target {
    Thread(String),
    Project(String),
}

impl Target {
    fn edit(&self, app: &mut Desktop, change: impl FnOnce(&mut Vec<String>)) {
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
                if files.iter().any(|existing| existing == path) {
                    continue;
                }
                if files.len() >= 100 {
                    error = Some("You can attach up to 100 files per message.");
                    break;
                }
                files.push(path.into());
            }
        });
        if let Some(error) = error {
            app.error = Some(error.into())
        }
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

    pub(super) fn list(&self, files: &[String], theme: &tok::Theme, locked: bool) -> Option<El> {
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
        for path in files {
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
                                target.edit(app, |files| files.retain(|p| *p != path));
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
