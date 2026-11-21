use crate::workspace::label;
use crate::{Desktop, Request};
use rune::{El, Icon, Key, KeyEvent, Tx, el, icon, row, tok};
use rune_kit::{Kit, Placement, popover};
use uuid::Uuid;

#[derive(Clone, serde::Deserialize)]
pub(crate) struct OptionRow {
    pub(super) mode: String,
    pub(super) label: String,
    description: String,
    pub(super) denied: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub(super) struct Selection {
    pub instance: String,
    pub mode: String,
    #[serde(skip_serializing)]
    pub label: String,
}

impl OptionRow {
    fn description(&self) -> &str {
        self.denied.as_deref().unwrap_or(&self.description)
    }

    fn glyph(mode: &str) -> &'static Icon {
        match mode {
            "approval-required" => &LOCK,
            "auto-accept-edits" => &PEN,
            "auto" => &SHIELD_CHECK,
            _ => &SHIELD_OFF,
        }
    }

    fn key(&self) -> String {
        format!("launch-permission-{}", self.mode)
    }
}

enum Target {
    Launch(String),
    Thread {
        watch: u64,
        operation: String,
        instance: String,
    },
}

pub(super) struct Menu {
    target: Target,
    options: Vec<OptionRow>,
    focused: String,
    hovered: Option<String>,
    status: Option<String>,
}

impl Menu {
    fn focus_option(&mut self, tx: &mut Tx) {
        if let Some(row) = self
            .options
            .iter()
            .find(|p| p.mode == self.focused && p.denied.is_none())
            .or_else(|| self.options.iter().find(|p| p.denied.is_none()))
        {
            self.focused = row.mode.clone();
            tx.focus(row.key(), true);
        } else {
            tx.focus("launch-permission-panel", true);
        }
    }
}

impl Desktop {
    fn show_permissions(&mut self, tx: &mut Tx) {
        if self.pending || !self.can_operate || self.palette.is_some() {
            return;
        }
        self.models.close();
        let mut menu = if let Some(project) = self.new_threads.active.clone() {
            let Some((options, focused)) = self.new_threads.picker_options() else {
                return;
            };
            Menu {
                target: Target::Launch(project),
                options,
                focused,
                hovered: None,
                status: None,
            }
        } else {
            let Some(thread) = self.workspace.visible_thread() else {
                return;
            };
            let Some(watch) = self.workspace.terminal_watch() else {
                return;
            };
            if self.workspace.questions.contains_key(&thread.id) {
                return;
            }
            let Some(permission) = &thread.permission else {
                return;
            };
            let instance = permission.instance.clone();
            let focused = self
                .workspace
                .drafts
                .get(&thread.id)
                .and_then(|draft| draft.permission.as_ref())
                .filter(|pick| pick.instance == instance)
                .unwrap_or(permission)
                .mode
                .clone();
            let operation = Uuid::new_v4().to_string();
            if !self.write_request(
                None,
                Request::ThreadPermissions {
                    watch,
                    operation: operation.clone(),
                },
            ) {
                return;
            }
            Menu {
                target: Target::Thread {
                    watch,
                    operation,
                    instance,
                },
                options: vec![],
                focused,
                hovered: None,
                status: Some("Loading permissions…".into()),
            }
        };
        menu.focus_option(tx);
        self.permission_menu = Some(menu);
    }

    pub(super) fn close_stale_permissions(&mut self) {
        let Some(menu) = &self.permission_menu else {
            return;
        };
        let visible = match &menu.target {
            Target::Launch(project) => self.new_threads.active.as_ref() == Some(project),
            Target::Thread {
                watch, instance, ..
            } => {
                self.new_threads.active.is_none()
                    && self.workspace.terminal_watch() == Some(*watch)
                    && self.workspace.visible_thread().is_some_and(|t| {
                        !self.workspace.questions.contains_key(&t.id)
                            && t.permission
                                .as_ref()
                                .is_some_and(|p| &p.instance == instance)
                    })
            }
        };
        if !visible {
            self.permission_menu = None;
        }
    }

    pub(super) fn receive_permissions(
        &mut self,
        watch: u64,
        operation: &str,
        instance: &str,
        options: Vec<OptionRow>,
        error: Option<String>,
        tx: &mut Tx,
    ) {
        let Some(menu) = self.permission_menu.as_mut() else {
            return;
        };
        let Target::Thread {
            watch: expected,
            operation: request,
            instance: provider,
        } = &menu.target
        else {
            return;
        };
        if *expected != watch || request != operation {
            return;
        }
        if provider != instance && error.is_none() {
            menu.status =
                Some("The thread’s provider changed. Reopen permissions to retry.".into());
            return;
        }
        menu.options = options;
        menu.status = error;
        menu.focus_option(tx);
    }

    fn select_permission(&mut self, mode: &str, tx: &mut Tx) {
        let Some(menu) = &self.permission_menu else {
            return;
        };
        let Some(row) = menu
            .options
            .iter()
            .find(|p| p.denied.is_none() && p.mode == mode)
        else {
            return;
        };
        match &menu.target {
            Target::Launch(_) => {
                if !self.new_threads.select_permission(mode) {
                    return;
                }
                tx.focus("new-thread-prompt", true);
            }
            Target::Thread {
                watch, instance, ..
            } => {
                if self.workspace.terminal_watch() != Some(*watch) {
                    return;
                }
                let Some(thread) = self.workspace.visible_thread() else {
                    return;
                };
                if thread
                    .permission
                    .as_ref()
                    .is_none_or(|p| &p.instance != instance)
                {
                    return;
                }
                let id = thread.id.clone();
                if let Some(draft) = self.workspace.drafts.get_mut(&id) {
                    draft.select_permission(Selection {
                        instance: instance.clone(),
                        mode: row.mode.clone(),
                        label: row.label.clone(),
                    });
                }
                tx.focus("message-input", true);
            }
        }
        self.permission_menu = None;
    }

    pub(crate) fn permission_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if tx.focused() == Some("thread-terminal-grid")
            || self.palette.is_some()
            || (self.new_threads.active.is_none() && self.workspace.visible_thread().is_none())
        {
            return false;
        }
        if event.mods.shift && !event.mods.alt && event.is_command('a') {
            if !tx.key_repeat() {
                self.show_permissions(tx);
            }
            return true;
        }
        false
    }

    fn permission_menu_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(menu) = self.permission_menu.as_mut() else {
            return false;
        };
        match event.key {
            Key::Escape => {
                self.permission_menu = None;
                tx.focus("launch-permission-button", true);
            }
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End => {
                let allowed: Vec<_> = menu
                    .options
                    .iter()
                    .filter(|row| row.denied.is_none())
                    .collect();
                if allowed.is_empty() {
                    return true;
                }
                let i = allowed
                    .iter()
                    .position(|p| p.mode == menu.focused)
                    .unwrap_or(0);
                let next = match event.key {
                    Key::Home => 0,
                    Key::End => allowed.len() - 1,
                    Key::Left | Key::Up => i.saturating_sub(1),
                    _ => (i + 1).min(allowed.len() - 1),
                };
                let option = allowed[next];
                tx.focus(option.key(), true);
                menu.focused = option.mode.clone();
                menu.hovered = None;
            }
            _ => return false,
        }
        true
    }
}

pub(super) fn context_button(
    caption: &str,
    glyph: &'static Icon,
    theme: &tok::Theme,
    compact: bool,
) -> El {
    let button = row()
        .role("button")
        .center_y()
        .gap(5.)
        .px(6.)
        .h(26.)
        .radius(5.)
        .hover(theme.text.alpha(0.07))
        .help(caption)
        .child(icon(glyph, 12.).color(theme.text_secondary));
    if compact {
        button
    } else {
        button.child(label(caption, 12., 400., theme.text_secondary).truncate())
    }
}

pub(super) fn branch_label(branch: &str, theme: &tok::Theme, compact: bool) -> El {
    let row = row()
        .min_w(0.)
        .shrink()
        .center_y()
        .gap(5.)
        .help(branch)
        .child(icon(&BRANCH, 12.).color(theme.text_secondary));
    if compact {
        row.label(branch)
    } else {
        row.child(
            label(branch, 12., 400., theme.text_secondary)
                .min_w(0.)
                .truncate(),
        )
    }
}

pub(super) fn control(
    s: &Desktop,
    selected: Option<(&str, &str)>,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
    locked: bool,
    compact: bool,
) -> El {
    let caption = selected.map_or("Choose permissions", |(_, caption)| caption);
    let trigger = context_button(
        caption,
        selected.map_or(&LOCK, |(mode, _)| OptionRow::glyph(mode)),
        theme,
        compact,
    )
    .key("launch-permission-button")
    .label(format!("Permissions: {caption}"))
    .help("Permissions")
    .focusable(!locked)
    .disabled(locked)
    .on_press_tx(Desktop::show_permissions);
    let menu = s.permission_menu.as_ref().filter(|_| !locked);
    if let Some(menu) = menu {
        let shown = menu.hovered.as_ref().unwrap_or(&menu.focused);
        let mut buttons = row()
            .role("radiogroup")
            .label("Permissions")
            .focus_group()
            .w_full()
            .radius(6.)
            .padding(2.)
            .fill(theme.text.alpha(0.06));
        for option in &menu.options {
            let mode = option.mode.clone();
            let hover = option.mode.clone();
            let checked = selected.is_some_and(|(m, _)| m == mode);
            let available = option.denied.is_none();
            let mut button = el()
                .key(option.key())
                .role("radio")
                .label(&option.label)
                .checked(checked)
                .description(option.description())
                .help(option.description())
                .focusable(available)
                .focus_selected(checked)
                .center()
                .grow(1.)
                .h(24.)
                .radius(4.)
                .hover(theme.text.alpha(0.09))
                .child(icon(OptionRow::glyph(&option.mode), 14.).color(theme.text_secondary))
                .on_hover_change(move |s: &mut Desktop, inside, _| {
                    if let Some(menu) = &mut s.permission_menu {
                        if inside {
                            menu.hovered = Some(hover.clone());
                        } else if menu.hovered.as_ref() == Some(&hover) {
                            menu.hovered = None;
                        }
                    }
                });
            if checked {
                button = button.fill(theme.text.alpha(0.12));
            }
            button = if available {
                button.on_press_tx(move |s: &mut Desktop, tx| {
                    s.select_permission(&mode, tx);
                })
            } else {
                button.disabled(true).opacity(0.4)
            };
            buttons = buttons.child(button);
        }
        let mut panel = rune::col()
            .key("launch-permission-panel")
            .w(288.)
            .padding(8.)
            .role("dialog")
            .focusable(true)
            .label("Permissions")
            .focus_return("launch-permission-button")
            .modal(true, |s: &mut Desktop| s.permission_menu = None)
            .on_key(Desktop::permission_menu_key)
            .child(
                label("Permissions", 12., 500., theme.text_secondary)
                    .w(272.)
                    .pb(6.),
            )
            .child(buttons);
        if let Some(status) = &menu.status {
            panel = panel.child(label(status, 12., 400., theme.text_secondary).wrap());
        } else if let Some(option) = menu.options.iter().find(|p| &p.mode == shown) {
            panel = panel
                .child(label(&option.label, 14., 500., theme.text).pt(6.))
                .child(
                    label(option.description(), 12., 400., theme.text_secondary)
                        .wrap()
                        .min_h(32.),
                );
        }
        overlays.push(popover::anchored(
            kit,
            popover::raised(kit, panel, kit.radius.md),
            "launch-permission-button",
            Placement::TopEnd,
            4.,
        ));
    }
    trigger.expanded(menu.is_some())
}

static LOCK: Icon = Icon {
    name: "lock",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></g>"#,
};

static PEN: Icon = Icon {
    name: "pen",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 21h8"></path><path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"></path></g>"#,
};

static SHIELD_CHECK: Icon = Icon {
    name: "shield-check",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"></path><path d="m9 12 2 2 4-4"></path></g>"#,
};

static SHIELD_OFF: Icon = Icon {
    name: "shield-off",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m2 2 20 20"></path><path d="M5 5a1 1 0 0 0-1 1v7c0 5 3.5 7.5 7.67 8.94a1 1 0 0 0 .67.01c2.35-.82 4.48-1.97 5.9-3.71"></path><path d="M9.309 3.652A12.252 12.252 0 0 0 11.24 2.28a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1v7a9.784 9.784 0 0 1-.08 1.264"></path></g>"#,
};

static BRANCH: Icon = Icon {
    name: "branch",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6a9 9 0 0 0-9 9V3"></path><circle cx="18" cy="6" r="3"></circle><circle cx="6" cy="18" r="3"></circle></g>"#,
};
