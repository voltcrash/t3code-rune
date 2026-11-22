use super::{
    Desktop, El, Request, TextEdit, TextStyle, Thread, Tx, Workspace, icon, label, row, text_input,
    tok,
};
use rune_kit::{Kit, Menu, MenuItem, glyphs};
use uuid::Uuid;

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
    rename: Option<Rename>,
    forward: Option<Forward>,
    input: TextEdit,
}

impl Actions {
    pub(super) fn close_menu(&mut self) {
        self.menu = None;
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            menu: None,
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
    }
}

impl Desktop {
    fn workspace_edit_pending(&self) -> bool {
        self.pending || self.workspace.refresh_request.is_some()
    }

    fn thread_menu(&mut self, thread: &str, origin: Origin, point: Option<(f32, f32)>) {
        self.commit_title();
        self.workspace.actions.menu = self
            .workspace
            .action_target(thread, origin)
            .map(|target| OpenMenu { target, point });
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
        .on_close(|s: &mut Desktop| s.workspace.actions.menu = None);
    if let Some(point) = open.and_then(|m| m.point) {
        menu = menu.at(point.0, point.1);
    }
    if s.can_operate {
        let id = thread.id.clone();
        menu = menu.item(
            MenuItem::new("Rename thread")
                .disabled(s.workspace_edit_pending())
                .on_select_tx(move |s: &mut Desktop, tx| s.begin_rename(&id, origin, tx)),
        );
    }
    if s.can_operate {
        let id = thread.id.clone();
        let settled = !thread.settled;
        menu = menu.item(
            MenuItem::new(if settled {
                "Settle thread"
            } else {
                "Un-settle thread"
            })
            .icon(&glyphs::CHECK)
            .disabled(s.workspace_edit_pending())
            .on_select(move |s: &mut Desktop| s.set_settlement(&id, settled, origin)),
        );
    }
    if s.can_pin {
        let id = thread.id.clone();
        let pinned = !thread.pinned;
        menu = menu.item(
            MenuItem::new(if pinned { "Pin thread" } else { "Unpin thread" })
                .icon(if pinned { &PIN } else { &UNPIN })
                .disabled(s.workspace_edit_pending())
                .on_select(move |s: &mut Desktop| s.set_pin(&id, pinned)),
        );
    }
    let id = thread.id.clone();
    menu.item(
        MenuItem::new("Copy thread ID").on_select_tx(move |s: &mut Desktop, tx| {
            tx.copy(&id);
            s.workspace.actions.menu = None;
            if origin == Origin::Sidebar {
                tx.focus(&id, true);
            }
        }),
    )
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
