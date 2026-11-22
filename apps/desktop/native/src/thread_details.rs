use super::{Desktop, Request, label};
use rune::{El, Icon, Tx, col, el, icon, row, tok};
use rune_kit::{Kit, Menu, MenuItem, Placement, Popover, glyphs};
use serde::Deserialize;
use std::cell::Cell;
use std::collections::HashSet;

const WIDTH: f32 = 280.;
const GAP: f32 = 12.;
const CLEARANCE: f32 = 32.;

#[derive(Default, Deserialize)]
pub(crate) struct Hidden {
    threads: HashSet<String>,
    drafts: HashSet<String>,
}

impl Hidden {
    fn contains(&self, draft: bool, id: &str) -> bool {
        if draft { &self.drafts } else { &self.threads }.contains(id)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Labels {
    panel: String,
    toggle: String,
    context: String,
    checkout: String,
    locked: String,
    worktree: String,
    copy_path: String,
}

#[derive(Deserialize)]
pub(crate) struct Settings {
    #[serde(default)]
    hidden: Hidden,
    machine: Option<String>,
    windows: bool,
    environment: bool,
    #[serde(rename = "openIn", default)]
    open_in: bool,
    labels: Labels,
}

enum ContextAction {
    Checkout,
    Editor,
    Copy { point: (f32, f32) },
}

struct ContextMenu {
    target: (bool, String),
    action: ContextAction,
}

impl Settings {
    fn enabled(&self) -> bool {
        self.environment || self.open_in
    }

    pub(super) fn toggle_label(&self) -> &str {
        &self.labels.toggle
    }
}

#[derive(Default)]
pub(crate) struct Details {
    pub(crate) settings: Option<Settings>,
    pub(crate) editor: Option<editor::Catalog>,
    mounted: Cell<bool>,
    inline: Cell<bool>,
    popup: Option<(bool, String)>,
    menu: Option<ContextMenu>,
}

#[derive(Clone, Copy)]
pub(crate) struct Lane {
    pub(crate) left: f32,
    pub(crate) right: f32,
}

impl Lane {
    pub(crate) fn pad(self, element: El) -> El {
        element.pl(self.left).pr(self.right)
    }
}

pub(super) struct Layout {
    pub(super) lane: Lane,
    pub(super) banner_width: f32,
    pub(super) card: bool,
}

impl Desktop {
    fn details_target(&self) -> Option<(bool, &str)> {
        if !self.workspace.details.mounted.get() {
            return None;
        }
        if let Some(project) = self.new_threads.active.as_deref() {
            return Some((true, project));
        }
        self.workspace
            .visible_thread()
            .map(|t| (false, t.id.as_str()))
    }

    fn details_path(&self) -> Option<&str> {
        if let Some(id) = self.new_threads.active.as_deref() {
            if self.new_threads.details_checkout().0 {
                return None;
            }
            return self.workspace.project(id).map(|p| p.path.as_str());
        }
        self.thread_workspace_root()
    }

    fn copy_details_path(&mut self, tx: &mut Tx) {
        let Some(menu) = self.workspace.details.menu.take() else {
            return;
        };
        if self.details_target() != Some((menu.target.0, menu.target.1.as_str())) {
            return;
        }
        if let Some(path) = self.details_path() {
            let windows = self
                .workspace
                .details
                .settings
                .as_ref()
                .is_some_and(|s| s.windows);
            tx.copy(if windows {
                path.replace('/', "\\")
            } else {
                path.to_owned()
            });
        }
    }

    pub(crate) fn can_show_details(&self) -> bool {
        self.workspace
            .details
            .settings
            .as_ref()
            .is_some_and(Settings::enabled)
            && self.details_target().is_some()
    }

    pub(crate) fn toggle_details(&mut self, tx: &mut Tx) {
        let Some((draft, id)) = self.details_target() else {
            return;
        };
        let details = &self.workspace.details;
        let Some(settings) = details.settings.as_ref().filter(|s| s.enabled()) else {
            return;
        };
        let focus = if settings.environment {
            "details-context"
        } else if details.editor.as_ref().is_some_and(|c| c.can_choose) {
            "choose-editor"
        } else {
            "toggle-thread-details"
        };
        let focus_row = if details.inline.get() {
            let Some(watch) = self.workspace.feed else {
                return;
            };
            let visible = settings.hidden.contains(draft, id);
            let target = id.to_owned();
            self.write_request(
                None,
                Request::SetDetailsVisible {
                    watch,
                    draft,
                    target: &target,
                    visible,
                },
            );
            if let Some(settings) = self.workspace.details.settings.as_mut() {
                let hidden = if draft {
                    &mut settings.hidden.drafts
                } else {
                    &mut settings.hidden.threads
                };
                if visible {
                    hidden.remove(&target);
                } else {
                    hidden.insert(target);
                }
            }
            visible
        } else {
            let open = details
                .popup
                .as_ref()
                .is_none_or(|(is_draft, target)| *is_draft != draft || target != id);
            self.workspace.details.popup = open.then(|| (draft, id.to_owned()));
            open
        };
        self.workspace.details.menu = None;
        tx.focus_when_shown(
            if focus_row {
                focus
            } else {
                "toggle-thread-details"
            },
            true,
        );
    }

    fn open_details_menu(&mut self, action: ContextAction) {
        if let Some((draft, id)) = self.details_target() {
            self.workspace.details.menu = Some(ContextMenu {
                target: (draft, id.to_owned()),
                action,
            });
        }
    }
    pub(crate) fn dismiss_details(&mut self, tx: &mut Tx) -> bool {
        let details = &mut self.workspace.details;
        if !details.mounted.get() {
            return false;
        }
        if let Some(menu) = details.menu.take() {
            tx.focus_when_shown(
                if matches!(menu.action, ContextAction::Editor) {
                    "choose-editor"
                } else {
                    "details-context"
                },
                true,
            );
        } else if details.popup.take().is_some() {
            tx.focus_when_shown("toggle-thread-details", true);
        } else {
            return false;
        }
        true
    }

    pub(crate) fn reconcile_details(&mut self) -> bool {
        let target = self.details_target();
        let details = &self.workspace.details;
        let same = |(draft, id): &(bool, String)| target == Some((*draft, id.as_str()));
        let inline = details.inline.get();
        let close_popup = details.popup.as_ref().is_some_and(|p| !same(p) || inline);
        let close_menu = details.menu.as_ref().is_some_and(|m| {
            !same(&m.target)
                || close_popup
                || if inline {
                    details
                        .settings
                        .as_ref()
                        .is_none_or(|s| !s.enabled() || s.hidden.contains(m.target.0, &m.target.1))
                } else {
                    details.popup.is_none()
                }
        });
        if close_popup {
            self.workspace.details.popup = None;
        }
        if close_menu {
            self.workspace.details.menu = None;
        }
        close_popup || close_menu
    }
}

pub(super) fn layout(
    s: &Desktop,
    width: f32,
    height: f32,
    inset: f32,
    chat_mounted: bool,
) -> Layout {
    let details = &s.workspace.details;
    details.mounted.set(chat_mounted);
    let right = width - WIDTH - GAP - CLEARANCE;
    let inline = right - inset >= 640. && height - 2. * GAP >= 160.;
    details.inline.set(inline);
    let card = inline
        && details.settings.as_ref().is_some_and(|settings| {
            settings.enabled()
                && s.details_target()
                    .is_some_and(|(draft, id)| !settings.hidden.contains(draft, id))
        });
    if !card {
        return Layout {
            lane: Lane {
                left: inset,
                right: inset,
            },
            banner_width: width,
            card,
        };
    }
    let chat_width = (right - inset).min(s.chat_width.limit().unwrap_or(f32::MAX));
    let left = ((width - chat_width) / 2.)
        .min(right - chat_width)
        .max(inset);
    Layout {
        lane: Lane {
            left,
            right: width - left - chat_width,
        },
        banner_width: width - WIDTH - GAP,
        card,
    }
}

pub(super) const DETAILS_ICON: Icon = Icon {
    name: "thread-details",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M7 8h.01M10 8h7M7 12h.01M10 12h7M7 16h.01M10 16h7"/></g>"#,
};

#[path = "thread_details/editor.rs"]
pub(crate) mod editor;
#[path = "thread_details/icons.rs"]
mod icons;

fn context_row(s: &Desktop, settings: &Settings, theme: &tok::Theme) -> El {
    let labels = &settings.labels;
    let thread = s.workspace.visible_thread();
    let path = s.details_path();
    let (context, folder, editable) = if let Some(project) = s
        .new_threads
        .active
        .as_deref()
        .and_then(|id| s.workspace.project(id))
    {
        let (worktree, editable) = s.new_threads.details_checkout();
        (
            if worktree {
                &*labels.worktree
            } else {
                &*labels.checkout
            },
            if worktree {
                &super::WORKTREE_ICON
            } else {
                &glyphs::FOLDER
            },
            !project.scratch && editable && s.can_operate && !s.pending,
        )
    } else {
        let path = thread.and_then(|t| t.worktree.as_deref());
        (
            path.and_then(|p| p.rsplit(['/', '\\']).find(|part| !part.is_empty()))
                .unwrap_or(&labels.locked),
            if path.is_some() {
                &icons::FOLDER_GIT
            } else {
                &glyphs::FOLDER
            },
            false,
        )
    };
    let environment = s.environment.as_deref().unwrap_or(context);
    let summary = format!("{environment}, {context}");
    let machine = icons::machine(settings.machine.as_deref());
    let mut control = row()
        .key("details-context")
        .focusable(true)
        .w_full()
        .h(32.)
        .px(10.)
        .gap(10.)
        .center_y()
        .label(labels.context.replace("{context}", &summary))
        .help(path.unwrap_or(context))
        .child(icon(machine, 18.).color(theme.text_secondary))
        .child(
            label(environment, 13., 500., theme.text.alpha(0.8))
                .grow(1.)
                .truncate()
                .shrink(),
        )
        .child(icon(folder, 16.).color(theme.text_secondary));
    if path.is_some() {
        control = control.on_secondary_press(|s: &mut Desktop, press, _| {
            s.open_details_menu(ContextAction::Copy {
                point: (press.x, press.y),
            });
        });
    }
    if editable {
        control = control
            .role("button")
            .radius(8.)
            .hover(theme.hover)
            .child(icon(&glyphs::CHEVRON_DOWN, 16.).color(theme.text_secondary))
            .on_press(|s: &mut Desktop| s.open_details_menu(ContextAction::Checkout));
    } else {
        control = control.role("group");
    }
    el().key("details-context-trigger").w_full().child(control)
}

fn contents(s: &Desktop, settings: &Settings, theme: &tok::Theme) -> El {
    let mut rows = col().w(WIDTH).padding(9.);
    if settings.environment {
        rows = rows.child(context_row(s, settings, theme));
    }
    if settings.open_in
        && let Some(editor) = &s.workspace.details.editor
    {
        rows = rows.child(editor.row(s, theme));
    }
    rows
}

pub(super) fn menu(s: &Desktop, kit: &Kit, overlays: &mut Vec<El>) {
    let Some(ContextMenu { target, action }) = &s.workspace.details.menu else {
        return;
    };
    let Some(settings) = &s.workspace.details.settings else {
        return;
    };
    if s.details_target() != Some((target.0, target.1.as_str())) {
        return;
    }
    let mut menu = Menu::new("details-context")
        .open(true)
        .on_close(|s: &mut Desktop| s.workspace.details.menu = None);
    match action {
        ContextAction::Editor => {
            if let Some(editor) = &s.workspace.details.editor {
                editor.menu().build(kit, overlays);
            }
            return;
        }
        ContextAction::Copy { point } => {
            menu = menu.at(point.0, point.1).item(
                MenuItem::new(&settings.labels.copy_path)
                    .icon(&glyphs::COPY)
                    .disabled(s.details_path().is_none())
                    .on_select_tx(Desktop::copy_details_path),
            );
        }
        ContextAction::Checkout => {
            let (active, editable) = s.new_threads.details_checkout();
            let project = &target.1;
            for (worktree, text) in [
                (false, &settings.labels.checkout),
                (true, &settings.labels.worktree),
            ] {
                let project = project.clone();
                menu = menu.item(
                    MenuItem::new(text)
                        .checked(active == worktree)
                        .disabled(s.pending || !editable)
                        .on_select_tx(move |s: &mut Desktop, tx| {
                            s.workspace.details.menu = None;
                            if s.new_threads.active.as_ref() == Some(&project) && !s.pending {
                                s.new_threads.set_details_checkout(worktree);
                            }
                            tx.focus_when_shown("details-context", true);
                        }),
                );
            }
        }
    }
    menu.build(kit, overlays);
}

pub(super) fn header(
    s: &Desktop,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
    layout: &Layout,
) -> Option<El> {
    if !s.can_show_details() {
        return None;
    }
    let settings = s.workspace.details.settings.as_ref()?;
    let popup_open = !s.workspace.details.inline.get() && s.workspace.details.popup.is_some();
    let open = layout.card || popup_open;
    let trigger = el()
        .key("toggle-thread-details")
        .role("button")
        .label(&settings.labels.toggle)
        .help(&settings.labels.toggle)
        .checked(open)
        .no_window_drag()
        .focusable(true)
        .size(28., 28.)
        .center()
        .radius(6.)
        .hover(theme.hover)
        .child(icon(&DETAILS_ICON, 16.).color(theme.text_secondary))
        .on_press_tx(Desktop::toggle_details);
    if s.workspace.details.inline.get() {
        return Some(trigger);
    }
    let mut popover = Popover::new("thread-details")
        .label(&settings.labels.panel)
        .open(popup_open)
        .padding(0.)
        .placement(Placement::BottomEnd)
        .trigger(trigger)
        .on_close(|s: &mut Desktop| {
            s.workspace.details.popup = None;
            s.workspace.details.menu = None;
        });
    if popup_open {
        popover = popover.child(contents(s, settings, theme));
    }
    Some(popover.build(kit, overlays))
}

pub(super) fn card(s: &Desktop, width: f32, top: f32, theme: &tok::Theme) -> Option<El> {
    let settings = s.workspace.details.settings.as_ref()?;
    let card = col()
        .key("thread-details-inline")
        .role("region")
        .label(&settings.labels.panel)
        .w(WIDTH)
        .radius(16.)
        .fill(theme.card)
        .border(1., theme.separator)
        .child(contents(s, settings, theme));
    Some(card.absolute(width - WIDTH - GAP, top + GAP))
}
