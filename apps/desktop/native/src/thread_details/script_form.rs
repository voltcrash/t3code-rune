use super::super::super::action;
use super::{Desktop, El, Request, Stored, label, script_icon};
use rune::{ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon, row, spacer, text_input, tok};
use rune_kit::{Dialog, Kit, Switch};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::workspace::thread_details) struct Labels {
    pub(super) add: String,
    add_title: String,
    edit_title: String,
    description: String,
    name: String,
    command: String,
    preview: String,
    icon: String,
    setup: String,
    wait: String,
    settle: String,
    stop: String,
    auto: String,
    cancel: String,
    save: String,
    changes: String,
    saving: String,
    delete: String,
    remove: String,
    confirm: String,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Input {
    name: String,
    command: String,
    icon: String,
    preview_url: Option<String>,
    run_on_worktree_create: bool,
    wait_for_setup: bool,
    run_on_settle: bool,
    stop_on_settle: bool,
    auto_open_preview: bool,
}

pub(in crate::workspace::thread_details) struct Form {
    key: Option<String>,
    name: TextEdit,
    command: TextEdit,
    preview: TextEdit,
    input: Input,
    choosing_icon: bool,
    confirming: bool,
    pending: Option<u64>,
    error: Option<String>,
}

impl Form {
    fn new(key: Option<&str>, stored: &Stored) -> Self {
        Self {
            key: key.map(str::to_owned),
            name: TextEdit::new(&stored.name),
            command: TextEdit::new(&stored.command),
            preview: TextEdit::new(&stored.preview_url),
            input: Input {
                icon: if stored.icon.is_empty() {
                    "play".into()
                } else {
                    stored.icon.clone()
                },
                run_on_worktree_create: stored.run_on_worktree_create,
                wait_for_setup: stored.run_on_worktree_create && stored.r#async == Some(false),
                run_on_settle: stored.run_on_settle,
                stop_on_settle: stored.stop_on_settle,
                auto_open_preview: stored.auto_open_preview,
                ..Default::default()
            },
            choosing_icon: false,
            confirming: false,
            pending: None,
            error: None,
        }
    }
}

impl Desktop {
    pub(crate) fn script_editor_open(&self) -> bool {
        self.workspace.details.script_form.is_some()
    }
    pub(super) fn edit_script(&mut self, key: Option<&str>, tx: &mut Tx) {
        if !self.can_edit_scripts() {
            return;
        }
        let Some(rows) = self.current_scripts() else {
            return;
        };
        let form = if let Some(key) = key {
            let Some(s) = rows.iter().find(|s| s.key == key) else {
                return;
            };
            Form::new(Some(key), &s.script)
        } else {
            Form::new(None, &Stored::default())
        };
        self.workspace.details.menu = None;
        self.workspace.details.script_form = Some(form);
        tx.focus_when_shown("script-name", true);
    }
    fn close_script_form(&mut self) {
        let Some(form) = self.workspace.details.script_form.as_mut() else {
            return;
        };
        if form.pending.is_some() {
            return;
        }
        if form.confirming {
            form.confirming = false
        } else {
            self.workspace.details.script_form = None
        }
    }
    fn save_script_form(&mut self) {
        if !self.can_edit_scripts() {
            return;
        }
        let Some((watch, project)) = self.script_target() else {
            return;
        };
        let project = project.to_owned();
        let Some(form) = self.workspace.details.script_form.as_ref() else {
            return;
        };
        if form.pending.is_some() {
            return;
        }
        let input = (!form.confirming).then(|| Input {
            name: form.name.text.clone(),
            command: form.command.text.clone(),
            preview_url: (!form.preview.text.trim().is_empty())
                .then(|| form.preview.text.trim().to_owned()),
            auto_open_preview: !form.preview.text.trim().is_empty() && form.input.auto_open_preview,
            ..form.input.clone()
        });
        let key = form.key.clone();
        let request = self.next_request_id();
        if self.write_request(
            Some(request),
            Request::SaveProjectScript {
                watch,
                project: &project,
                key: key.as_deref(),
                input: input.as_ref(),
            },
        ) && let Some(f) = self.workspace.details.script_form.as_mut()
        {
            f.pending = Some(request);
            f.error = None;
        }
    }
    pub(crate) fn script_saved(&mut self, watch: u64, id: Option<u64>, error: Option<String>) {
        if self.workspace.feed != Some(watch) || id.is_none() {
            return;
        }
        let Some(form) = self
            .workspace
            .details
            .script_form
            .as_mut()
            .filter(|f| f.pending == id)
        else {
            return;
        };
        if error.is_some() {
            form.pending = None;
            form.error = error;
            form.confirming = false;
        } else {
            self.workspace.details.script_form = None;
        }
    }
}

type Field = fn(&mut Form) -> &mut TextEdit;
fn field(
    edit: &TextEdit,
    key: &str,
    caption: &str,
    theme: &tok::Theme,
    enabled: bool,
    multi: bool,
    get: Field,
) -> El {
    let input = text_input(edit, TextStyle::new(14., 400.))
        .key(key)
        .label(caption)
        .w_full()
        .h(if multi { 58. } else { 32. })
        .color(theme.text)
        .disabled(!enabled)
        .on_edit(move |s: &mut Desktop| get(s.workspace.details.script_form.as_mut().unwrap()));
    col()
        .w_full()
        .gap(5.)
        .child(label(caption, 13., 500., theme.text))
        .child(
            col()
                .w_full()
                .px(8.)
                .border(1., theme.separator)
                .radius(6.)
                .child(if multi { input } else { input.single_line() }),
        )
}

pub(crate) fn overlay(
    s: &Desktop,
    theme: &tok::Theme,
    kit: &Kit,
    height: f32,
    overlays: &mut Vec<El>,
) {
    let Some(f) = s.workspace.details.script_form.as_ref() else {
        return;
    };
    let Some(settings) = s.workspace.details.settings.as_ref() else {
        return;
    };
    let l = &settings.labels.form;
    let enabled = s.can_edit_scripts() && f.pending.is_none();
    let title = if f.confirming {
        &l.remove
    } else if f.key.is_some() {
        &l.edit_title
    } else {
        &l.add_title
    };
    let mut dialog = Dialog::new("project-script-form", title)
        .open(true)
        .width(512.)
        .fit()
        .on_close(Desktop::close_script_form);
    if f.confirming {
        dialog = dialog.child(
            label(
                &format!("{} — {}", f.name.text, l.confirm),
                14.,
                400.,
                theme.text_secondary,
            )
            .px(16.)
            .wrap(),
        );
    } else {
        dialog = dialog.child(
            label(&l.description, 14., 400., theme.text_secondary)
                .px(16.)
                .pb(12.)
                .wrap(),
        );
        let mut body = col()
            .key("script-fields")
            .w_full()
            .h((height - 220.).clamp(105., 480.))
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .px(16.)
            .gap(12.);
        let choose = el()
            .key("script-icon")
            .role("button")
            .label(&l.icon)
            .help(&l.icon)
            .focusable(true)
            .disabled(!enabled)
            .size(36., 36.)
            .center()
            .radius(6.)
            .border(1., theme.separator)
            .child(icon(script_icon(&f.input.icon), 18.).color(theme.text))
            .on_press(|s: &mut Desktop| {
                if let Some(f) = s.workspace.details.script_form.as_mut() {
                    f.choosing_icon = !f.choosing_icon
                }
            });
        body = body.child(row().w_full().gap(8.).center_y().child(choose).child(
            col().grow(1.).min_w(0.).child(field(
                &f.name,
                "script-name",
                &l.name,
                theme,
                enabled,
                false,
                |f| &mut f.name,
            )),
        ));
        if f.choosing_icon {
            let mut options = row().gap(6.);
            for kind in ["play", "test", "lint", "configure", "build", "debug"] {
                options = options.child(
                    el().key(format!("script-icon-{kind}"))
                        .role("button")
                        .focusable(true)
                        .label(kind)
                        .size(32., 32.)
                        .center()
                        .radius(5.)
                        .hover(theme.hover)
                        .child(icon(script_icon(kind), 18.).color(theme.text))
                        .on_press_tx(move |s: &mut Desktop, tx| {
                            if let Some(f) = s.workspace.details.script_form.as_mut() {
                                f.input.icon = kind.into();
                                f.choosing_icon = false;
                                tx.focus_when_shown("script-icon", true);
                            }
                        }),
                );
            }
            body = body.child(options);
        }
        body = body
            .child(field(
                &f.command,
                "script-command",
                &l.command,
                theme,
                enabled,
                true,
                |f| &mut f.command,
            ))
            .child(field(
                &f.preview,
                "script-preview",
                &l.preview,
                theme,
                enabled,
                false,
                |f| &mut f.preview,
            ));
        type Toggle = fn(&mut Input) -> &mut bool;
        let flags: [(&str, bool, bool, Toggle); 5] = [
            (&l.setup, f.input.run_on_worktree_create, true, |i| {
                &mut i.run_on_worktree_create
            }),
            (
                &l.wait,
                f.input.wait_for_setup,
                f.input.run_on_worktree_create,
                |i| &mut i.wait_for_setup,
            ),
            (&l.settle, f.input.run_on_settle, true, |i| {
                &mut i.run_on_settle
            }),
            (&l.stop, f.input.stop_on_settle, true, |i| {
                &mut i.stop_on_settle
            }),
            (
                &l.auto,
                f.input.auto_open_preview,
                !f.preview.text.trim().is_empty(),
                |i| &mut i.auto_open_preview,
            ),
        ];
        for (caption, checked, available, flag) in flags {
            body = body.child(
                row()
                    .w_full()
                    .center_y()
                    .min_h(36.)
                    .gap(8.)
                    .px(10.)
                    .radius(6.)
                    .border(1., theme.separator)
                    .child(label(caption, 13., 400., theme.text).grow(1.).wrap())
                    .child(
                        Switch::new(checked)
                            .web()
                            .label(caption)
                            .hide_label()
                            .disabled(!enabled || !available)
                            .on_toggle(move |s: &mut Desktop| {
                                if let Some(f) = s.workspace.details.script_form.as_mut() {
                                    let value = flag(&mut f.input);
                                    *value = !*value;
                                }
                            })
                            .build(kit),
                    ),
            );
        }
        if let Some(err) = &f.error {
            body = body.child(label(err, 13., 400., kit.c.danger.fg).wrap())
        }
        dialog = dialog.child(body);
    }
    let mut footer = row().w_full().padding(16.).gap(8.);
    if f.key.is_some() && !f.confirming {
        footer = footer.child(action(&l.delete, theme, enabled, |s: &mut Desktop| {
            if let Some(f) = s.workspace.details.script_form.as_mut() {
                f.confirming = true
            }
        }));
    }
    let caption = if f.pending.is_some() {
        &l.saving
    } else if f.confirming {
        &l.remove
    } else if f.key.is_some() {
        &l.changes
    } else {
        &l.save
    };
    footer = footer
        .child(spacer())
        .child(action(
            &l.cancel,
            theme,
            f.pending.is_none(),
            Desktop::close_script_form,
        ))
        .child(action(
            caption,
            theme,
            enabled
                && (f.confirming
                    || (!f.name.text.trim().is_empty() && !f.command.text.trim().is_empty())),
            Desktop::save_script_form,
        ));
    dialog.child(footer).build(kit, overlays)
}
