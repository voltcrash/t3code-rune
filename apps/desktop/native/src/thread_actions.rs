use super::{
    Desktop, El, Request, TextEdit, TextStyle, Thread, Tx, Workspace, icon, label, row, text_input,
    tok,
};
use rune_kit::{Kit, Menu, MenuEntry, MenuItem, glyphs};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Labels {
    pin: String,
    unpin: String,
    settle: String,
    unsettle: String,
    rename: String,
    mark_unread: String,
    copy: String,
    copy_path: String,
    copy_branch: String,
    copy_thread_id: String,
    delete: String,
    delete_title: String,
    delete_description: String,
    cancel: String,
    confirm: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Header,
    Sidebar,
}

struct Target {
    thread: String,
    origin: Origin,
    watch: u64,
}

struct OpenMenu {
    target: Target,
    point: Option<(f32, f32)>,
}

struct Forward {
    committed: Option<u64>,
    thread: String,
    project: String,
    next: Option<String>,
}

struct Rename {
    target: Target,
    original: String,
}

pub(super) struct Actions {
    menu: Option<OpenMenu>,
    copy_open: bool,
    deleting: Option<Target>,
    rename: Option<Rename>,
    forward: Option<Forward>,
    input: TextEdit,
}

impl Actions {
    pub(super) fn close_sidebar_menu(&mut self) -> bool {
        self.menu
            .take_if(|menu| menu.target.origin == Origin::Sidebar)
            .is_some()
    }

    pub(super) fn close_menu(&mut self) {
        self.menu = None;
        self.copy_open = false;
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            menu: None,
            copy_open: false,
            deleting: None,
            rename: None,
            forward: None,
            input: TextEdit::new(""),
        }
    }
}

impl Workspace {
    fn action_target(&self, thread: &str, origin: Origin) -> Option<Target> {
        self.by_thread.contains_key(thread).then_some(Target {
            thread: thread.to_owned(),
            origin,
            watch: self.feed?,
        })
    }

    pub(super) fn reconcile_actions(&mut self) {
        let valid =
            |t: &Target| self.feed == Some(t.watch) && self.by_thread.contains_key(&t.thread);
        if self
            .actions
            .menu
            .as_ref()
            .is_some_and(|m| !valid(&m.target))
        {
            self.actions.menu = None;
        }
        if self
            .actions
            .rename
            .as_ref()
            .is_some_and(|r| !valid(&r.target))
        {
            self.actions.rename = None;
        }
        if self.actions.deleting.as_ref().is_some_and(|t| !valid(t)) {
            self.actions.deleting = None;
        }
    }
}

impl Desktop {
    fn workspace_edit_pending(&self) -> bool {
        self.pending || self.workspace.refresh_request.is_some()
    }

    fn thread_menu(&mut self, thread: &str, origin: Origin, point: Option<(f32, f32)>) {
        self.commit_title();
        self.workspace.actions.copy_open = false;
        self.workspace.actions.menu = self
            .workspace
            .action_target(thread, origin)
            .map(|target| OpenMenu { target, point });
    }

    fn mark_unread(&mut self, id: &str) {
        self.workspace.actions.close_menu();
        let Some(target) = self.workspace.action_target(id, Origin::Sidebar) else {
            return;
        };
        if self.can_operate {
            self.write_request(
                None,
                Request::MarkUnread {
                    thread: target.thread,
                    watch: target.watch,
                    operation: Uuid::new_v4().to_string(),
                },
            );
        }
    }

    fn request_delete(&mut self, id: &str, origin: Origin) {
        self.workspace.actions.close_menu();
        if !self.can_operate || self.workspace_edit_pending() {
            return;
        }
        let Some(target) = self.workspace.action_target(id, origin) else {
            return;
        };
        if self.confirm_thread_delete {
            self.workspace.actions.deleting = Some(target);
        } else {
            self.delete_thread(target);
        }
    }

    fn confirm_delete(&mut self) {
        if let Some(target) = self.workspace.actions.deleting.take() {
            self.delete_thread(target);
        }
    }

    fn delete_thread(&mut self, target: Target) {
        if !self.can_operate || self.workspace.feed != Some(target.watch) {
            return;
        }
        self.write_request(
            None,
            Request::DeleteThread {
                thread: target.thread,
                watch: target.watch,
                operation: Uuid::new_v4().to_string(),
            },
        );
    }

    fn set_pin(&mut self, id: &str, pinned: bool) {
        self.workspace.actions.menu = None;
        if self.workspace_edit_pending() || !self.can_pin {
            return;
        }
        let Some(watch) = self.workspace.feed else {
            return;
        };
        let Some(&i) = self.workspace.by_thread.get(id) else {
            return;
        };
        if self.workspace.threads[i].pinned != pinned {
            self.write_request(
                None,
                Request::SetPin {
                    thread: id.into(),
                    watch,
                    pinned,
                    operation: Uuid::new_v4().to_string(),
                },
            );
        }
    }

    fn set_settlement(&mut self, id: &str, settled: bool, origin: Origin) {
        self.workspace.actions.menu = None;
        let ws = &self.workspace;
        let Some(target) = ws.action_target(id, origin) else {
            return;
        };
        if self.workspace_edit_pending() || !self.can_operate {
            return;
        }
        let thread = &ws.threads[ws.by_thread[id]];
        if thread.settled == settled {
            return;
        }
        let advance = settled
            && origin == Origin::Sidebar
            && self.new_threads.active.is_none()
            && ws.visible_thread().is_some_and(|t| t.id == id);
        self.workspace.actions.forward = advance.then(|| {
            let next = ws
                .filtered
                .iter()
                .position(|&i| ws.threads[i].id == id)
                .and_then(|pos| {
                    ws.filtered[pos + 1..]
                        .iter()
                        .chain(&ws.filtered[..pos])
                        .map(|&i| &ws.threads[i])
                        .find(|t| ws.can_advance_to(t))
                        .map(|t| t.id.clone())
                });
            Forward {
                committed: None,
                thread: id.into(),
                project: thread.project.clone(),
                next,
            }
        });
        self.send(Request::SetSettlement {
            thread: id.into(),
            watch: target.watch,
            settled,
            advance,
            operation: Uuid::new_v4().to_string(),
        });
    }

    pub(crate) fn settlement_committed(&mut self, sequence: Option<u64>) {
        if let Some(plan) = &mut self.workspace.actions.forward {
            plan.committed = sequence;
        }
        if sequence.is_none() {
            self.cancel_settle_navigation();
            self.send(Request::Refresh);
        }
    }

    pub(crate) fn cancel_settle_navigation(&mut self) {
        self.workspace.actions.forward = None;
    }

    pub(crate) fn waiting_for_settle(&self) -> bool {
        self.workspace
            .actions
            .forward
            .as_ref()
            .is_some_and(|p| p.committed.is_some())
    }

    pub(crate) fn advance_after_settle(&mut self) {
        if self.pending
            || !self
                .workspace
                .actions
                .forward
                .as_ref()
                .and_then(|p| p.committed)
                .is_some_and(|n| self.workspace.sequence >= n)
        {
            return;
        }
        let Some(plan) = self.workspace.actions.forward.take() else {
            return;
        };
        if self.new_threads.active.is_some()
            || !self
                .workspace
                .visible_thread()
                .is_some_and(|t| t.id == plan.thread && t.settled)
        {
            return;
        }
        if let Some(next) = plan.next
            && self
                .workspace
                .by_thread
                .get(&next)
                .is_some_and(|&i| self.workspace.can_advance_to(&self.workspace.threads[i]))
        {
            self.open_thread(&next);
        } else if self
            .workspace
            .project(&plan.project)
            .is_some_and(|p| !p.archived)
        {
            self.prepare_thread(&plan.project);
        }
    }

    pub(crate) fn thread_action_shortcut(&mut self, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        if !self.can_operate
            || !ev.mods.shift
            || ev.mods.alt
            || !(ev.is_command('s') || ev.is_command('p') && self.can_pin)
            || self.workspace.project_scope.is_open()
            || tx.focused() == Some("thread-terminal-grid")
            || self.new_threads.active.is_some()
        {
            return false;
        }
        let Some(t) = self.workspace.visible_thread() else {
            return false;
        };
        if !tx.key_repeat() {
            if ev.is_command('p') {
                self.set_pin(&t.id.clone(), !t.pinned);
            } else {
                self.set_settlement(&t.id.clone(), !t.settled, Origin::Sidebar);
            }
        }
        true
    }

    fn begin_rename(&mut self, id: &str, origin: Origin, tx: &mut Tx) {
        self.workspace.actions.menu = None;
        let Some(target) = self.workspace.action_target(id, origin) else {
            return;
        };
        if !self.can_operate || self.workspace_edit_pending() {
            return;
        }
        let title = self.workspace.threads[self.workspace.by_thread[id]]
            .title
            .clone();
        let mut input = TextEdit::new(&title);
        input.focused = true;
        input.set_selection(0, title.len());
        self.workspace.actions.input = input;
        self.workspace.actions.rename = Some(Rename {
            target,
            original: title,
        });
        self.workspace.filter_revision += 1;
        tx.focus("thread-title-input", true);
    }

    pub(crate) fn commit_title(&mut self) {
        let Some(rename) = self.workspace.actions.rename.take() else {
            return;
        };
        self.workspace.filter_revision += 1;
        let title = self.workspace.actions.input.text.trim().to_owned();
        if self.workspace.feed != Some(rename.target.watch)
            || !self.can_operate
            || !self.workspace.has_thread(&rename.target.thread)
        {
            return;
        }
        if title.is_empty() {
            self.error = Some("Thread title cannot be empty".into());
        } else if title != rename.original {
            self.write_request(
                None,
                Request::RenameThread {
                    thread: rename.target.thread,
                    title,
                    watch: rename.target.watch,
                    operation: Uuid::new_v4().to_string(),
                },
            );
        }
    }

    pub(crate) fn finish_title_on_blur(&mut self) -> bool {
        if self.workspace.actions.rename.is_none() || self.workspace.actions.input.focused {
            return false;
        }
        self.commit_title();
        true
    }

    pub(crate) fn cancel_title_edit(&mut self, tx: &mut Tx) -> bool {
        let Some(rename) = self.workspace.actions.rename.take() else {
            return false;
        };
        self.workspace.filter_revision += 1;
        restore_focus(&rename.target, tx);
        true
    }
}

fn restore_focus(t: &Target, tx: &mut Tx) {
    tx.focus(
        if t.origin == Origin::Header {
            "thread-actions-trigger"
        } else {
            &t.thread
        },
        true,
    );
}

pub(super) fn editor(
    s: &Desktop,
    thread: &Thread,
    origin: Origin,
    theme: &tok::Theme,
) -> Option<El> {
    let actions = &s.workspace.actions;
    let rename = actions.rename.as_ref()?;
    if rename.target.origin != origin || rename.target.thread != thread.id {
        return None;
    }
    Some(
        text_input(&actions.input, TextStyle::new(14., 500.))
            .key("thread-title-input")
            .label("Thread title")
            .single_line()
            .no_window_drag()
            .w_full()
            .max_w(360.)
            .min_w(0.)
            .shrink()
            .h(24.)
            .px(4.)
            .radius(4.)
            .border(1., theme.separator)
            .color(theme.text)
            .on_edit(|s: &mut Desktop| &mut s.workspace.actions.input)
            .on_submit(|s: &mut Desktop, tx| {
                if let Some(r) = &s.workspace.actions.rename {
                    restore_focus(&r.target, tx);
                }
                s.commit_title();
            }),
    )
}

pub(super) fn context(trigger: El, id: &str, origin: Origin) -> El {
    let id = id.to_owned();
    trigger.on_secondary_press(move |s: &mut Desktop, p, _| {
        s.thread_menu(&id, origin, Some((p.x, p.y)))
    })
}

pub(super) fn header(
    s: &Desktop,
    thread: &Thread,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
) -> El {
    if let Some(input) = editor(s, thread, Origin::Header, theme) {
        return input;
    }
    let id = thread.id.clone();
    let direct = id.clone();
    let trigger = row()
        .role("button")
        .label(format!("Thread actions for {}", thread.title))
        .no_window_drag()
        .focusable(true)
        .min_w(0.)
        .shrink()
        .h(26.)
        .px(4.)
        .gap(4.)
        .center_y()
        .radius(4.)
        .hover(theme.separator)
        .child(
            label(&thread.title, 14., 500., theme.text)
                .truncate()
                .shrink(),
        )
        .child(icon(&glyphs::CHEVRON_DOWN, 12.).color(theme.text_secondary))
        .on_press(move |s: &mut Desktop| s.thread_menu(&id, Origin::Header, None));
    let trigger = if s.can_operate {
        trigger.on_double_press_tx(move |s: &mut Desktop, tx| {
            s.begin_rename(&direct, Origin::Header, tx)
        })
    } else {
        trigger
    };
    let menu = menu(s, thread, Origin::Header, "thread-actions");
    menu.trigger(context(trigger, &thread.id, Origin::Header))
        .build(kit, overlays)
}

const CIRCLE_CHECK: rune::Icon = rune::Icon {
    name: "circle-check",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/></g>"#,
};
const PENCIL: rune::Icon = rune::Icon {
    name: "pencil",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/></g>"#,
};
const MAIL_OPEN: rune::Icon = rune::Icon {
    name: "mail-open",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z"/><path d="m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10"/></g>"#,
};
const FOLDER: rune::Icon = rune::Icon {
    name: "folder",
    view: 24.,
    body: r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const GIT_BRANCH: rune::Icon = rune::Icon {
    name: "git-branch",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/></g>"#,
};
const HASH: rune::Icon = rune::Icon {
    name: "hash",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="4" x2="20" y1="9" y2="9"/><line x1="4" x2="20" y1="15" y2="15"/><line x1="10" x2="8" y1="3" y2="21"/><line x1="16" x2="14" y1="3" y2="21"/></g>"#,
};
const TRASH: rune::Icon = rune::Icon {
    name: "trash",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2"/></g>"#,
};

const PIN: rune::Icon = rune::Icon {
    name: "pin",
    view: 24.,
    body: r#"<path d="M12 17v5 M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const UNPIN: rune::Icon = rune::Icon {
    name: "pin-off",
    view: 24.,
    body: r#"<path d="M12 17v5 M15 9.34V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H7.89 M2 2l20 20 M9 9v1.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) fn pin_indicator(s: &Desktop, thread: &Thread, theme: &tok::Theme) -> El {
    let pin = icon(&PIN, 12.).color(theme.text_secondary.alpha(0.65));
    if !s.can_pin {
        return pin.role("img").label("Pinned");
    }
    let id = thread.id.clone();
    rune::el()
        .size(12., 12.)
        .key(format!("unpin-{id}"))
        .role("button")
        .label("Unpin thread")
        .help("Unpin thread")
        .focusable(!s.workspace_edit_pending())
        .disabled(s.workspace_edit_pending())
        .hover_group()
        .child(pin.group_opacity(1., 0.).a11y_hidden())
        .child(
            icon(&UNPIN, 12.)
                .absolute(0., 0.)
                .color(theme.text)
                .group_opacity(0., 1.)
                .a11y_hidden(),
        )
        .on_press(move |s: &mut Desktop| s.set_pin(&id, false))
}

fn menu(s: &Desktop, thread: &Thread, origin: Origin, key: &str) -> Menu {
    let open = s
        .workspace
        .actions
        .menu
        .as_ref()
        .filter(|m| m.target.origin == origin && m.target.thread == thread.id);
    let mut menu = Menu::new(key)
        .open(open.is_some())
        .on_close(|s: &mut Desktop| s.workspace.actions.close_menu());
    if let Some(point) = open.and_then(|m| m.point) {
        menu = menu.at(point.0, point.1);
    }
    let Some(labels) = &s.thread_labels else {
        return menu;
    };
    if origin == Origin::Header
        && s.can_find_thread()
        && s.workspace
            .visible_thread()
            .is_some_and(|t| t.id == thread.id)
        && let Some(find) = &s.workspace.find_labels
    {
        menu = menu.item(
            MenuItem::new(&find.title)
                .icon(&glyphs::SEARCH)
                .on_select_tx(|s: &mut Desktop, tx| {
                    s.workspace.actions.close_menu();
                    s.open_thread_find(Some("thread-actions-trigger"), tx);
                }),
        );
    }
    let locked = !s.can_operate || s.workspace_edit_pending();
    if s.can_pin {
        let id = thread.id.clone();
        let pinned = !thread.pinned;
        menu = menu.item(
            MenuItem::new(if pinned { &labels.pin } else { &labels.unpin })
                .icon(if pinned { &PIN } else { &UNPIN })
                .disabled(s.workspace_edit_pending())
                .on_select(move |s: &mut Desktop| s.set_pin(&id, pinned)),
        );
    }
    let id = thread.id.clone();
    let settled = !thread.settled;
    menu = menu
        .item(
            MenuItem::new(if settled {
                &labels.settle
            } else {
                &labels.unsettle
            })
            .icon(&CIRCLE_CHECK)
            .disabled(locked)
            .on_select(move |s: &mut Desktop| s.set_settlement(&id, settled, origin)),
        )
        .separator();
    let id = thread.id.clone();
    menu = menu.item(
        MenuItem::new(&labels.rename)
            .icon(&PENCIL)
            .disabled(locked)
            .on_select_tx(move |s: &mut Desktop, tx| s.begin_rename(&id, origin, tx)),
    );
    let id = thread.id.clone();
    menu = menu
        .item(
            MenuItem::new(&labels.mark_unread)
                .icon(&MAIL_OPEN)
                .disabled(!s.can_operate)
                .on_select(move |s: &mut Desktop| s.mark_unread(&id)),
        )
        .separator();
    let copy = |text: String| {
        move |s: &mut Desktop, tx: &mut Tx| {
            tx.copy(&text);
            s.workspace.actions.close_menu();
        }
    };
    let path = thread.worktree.clone().or_else(|| {
        s.workspace
            .project(&thread.project)
            .map(|p| p.path.clone())
            .filter(|p| !p.is_empty())
    });
    let mut copies = Vec::new();
    if let Some(path) = path {
        copies.push(MenuEntry::item(
            MenuItem::new(&labels.copy_path)
                .icon(&FOLDER)
                .on_select_tx(copy(path)),
        ));
    }
    if let Some(branch) = thread.branch.clone() {
        copies.push(MenuEntry::item(
            MenuItem::new(&labels.copy_branch)
                .icon(&GIT_BRANCH)
                .on_select_tx(copy(branch)),
        ));
    }
    copies.push(MenuEntry::item(
        MenuItem::new(&labels.copy_thread_id)
            .icon(&HASH)
            .on_select_tx(copy(thread.id.clone())),
    ));
    menu = menu
        .item(
            MenuItem::new(&labels.copy)
                .icon(&glyphs::COPY)
                .submenu(copies, s.workspace.actions.copy_open)
                .on_select(|s: &mut Desktop| {
                    s.workspace.actions.copy_open = !s.workspace.actions.copy_open
                }),
        )
        .separator();
    let id = thread.id.clone();
    menu.item(
        MenuItem::new(&labels.delete)
            .icon(&TRASH)
            .danger(true)
            .disabled(locked)
            .on_select(move |s: &mut Desktop| s.request_delete(&id, origin)),
    )
}

pub(super) fn delete_dialog(s: &Desktop, theme: &tok::Theme, overlays: &mut Vec<El>) {
    let (Some(target), Some(labels)) = (&s.workspace.actions.deleting, &s.thread_labels) else {
        return;
    };
    let Some(&i) = s.workspace.by_thread.get(&target.thread) else {
        return;
    };
    let title = labels
        .delete_title
        .replace("{title}", &s.workspace.threads[i].title);
    crate::confirm_dialog::overlay(
        crate::confirm_dialog::Confirm {
            key: "delete-thread",
            title: &title,
            description: Some(&labels.delete_description),
            cancel: &labels.cancel,
            confirm: &labels.confirm,
            destructive: true,
        },
        theme,
        |s| s.workspace.actions.deleting = None,
        Desktop::confirm_delete,
        overlays,
    );
}

pub(super) fn sidebar_menu(s: &Desktop, kit: &Kit, overlays: &mut Vec<El>) {
    let Some(m) = &s.workspace.actions.menu else {
        return;
    };
    if m.target.origin == Origin::Sidebar
        && let Some(&i) = s.workspace.by_thread.get(&m.target.thread)
    {
        menu(
            s,
            &s.workspace.threads[i],
            Origin::Sidebar,
            "sidebar-thread-actions",
        )
        .build(kit, overlays);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkerMessage;
    use serde_json::json;

    #[test]
    fn delete_waits_for_confirmation_only_when_the_setting_asks_for_it() {
        use std::io::{Read, Seek};
        let mut wire = tempfile::tempfile().unwrap();
        #[cfg(unix)]
        let output = std::os::fd::OwnedFd::from(wire.try_clone().unwrap());
        #[cfg(windows)]
        let output = std::os::windows::io::OwnedHandle::from(wire.try_clone().unwrap());
        let mut app = Desktop::new();
        app.output = Some(output.into());
        app.can_operate = true;
        app.workspace.feed = Some(1);
        app.workspace.replace(
            vec![],
            serde_json::from_value(json!([
                {"id":"a","project":"p","title":"a","model":"m","status":"idle","settled":false,"snoozed_until":null},
                {"id":"b","project":"p","title":"b","model":"m","status":"idle","settled":false,"snoozed_until":null}
            ]))
            .unwrap(),
        );
        app.request_delete("a", Origin::Sidebar);
        assert!(app.workspace.actions.deleting.is_some());
        app.workspace.actions.deleting = None;
        app.request_delete("a", Origin::Header);
        app.confirm_delete();
        app.confirm_thread_delete = false;
        app.request_delete("b", Origin::Sidebar);
        app.mark_unread("b");
        wire.rewind().unwrap();
        let mut text = String::new();
        wire.read_to_string(&mut text).unwrap();
        let sent: Vec<(String, String)> = text
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .map(|v| {
                (
                    v["type"].as_str().unwrap().into(),
                    v["thread"].as_str().unwrap().into(),
                )
            })
            .collect();
        let expected = [
            ("deletethread", "a"),
            ("deletethread", "b"),
            ("markunread", "b"),
        ];
        assert_eq!(sent, expected.map(|(t, id)| (t.to_owned(), id.to_owned())));
    }

    #[test]
    fn a_settle_receipt_waits_for_the_destination_in_the_same_shell_batch() {
        let thread = |id: &str, settled: bool, snoozed: Option<i64>| {
            json!({
                "id": id, "project": "p", "title": id, "model": "m", "status": "idle",
                "settled": settled, "snoozed_until": snoozed
            })
        };
        for (snoozed, expected) in [(Some(i64::MAX), "a"), (None, "b")] {
            let mut app = Desktop::new();
            app.can_operate = true;
            app.workspace.feed = Some(1);
            app.workspace.replace(
                vec![],
                serde_json::from_value(json!([thread("a", false, None), thread("b", false, None)]))
                    .unwrap(),
            );
            app.open_thread("a");
            app.set_settlement("a", true, Origin::Sidebar);
            app.pending = true;
            app.receive(
                serde_json::from_value::<WorkerMessage>(json!({
                    "type":"settlementchanged","id":app.request_id,"watch":1,"sequence":9
                }))
                .unwrap(),
            );
            assert_eq!(app.workspace.visible_thread().unwrap().id, "a");
            app.receive(
                serde_json::from_value(json!({
                    "type":"workspacelive","watch":1,"sequence":9,"scratch_available":false,
                    "threads":[thread("a",true,None),thread("b",false,snoozed)],
                    "removed_threads":[],"projects":[],"removed_projects":[]
                }))
                .unwrap(),
            );
            assert_eq!(app.workspace.visible_thread().unwrap().id, expected);
            assert!(!app.waiting_for_settle());
        }
    }
}
