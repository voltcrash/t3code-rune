use super::{
    Desktop, El, Kit, Request, TextEdit, TextStyle, Thread, Tx, Workspace, icon, label, row,
    text_input, tok,
};
use rune_kit::{Menu, MenuItem, glyphs};
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

struct Rename {
    target: Target,
    original: String,
}

pub(super) struct Actions {
    menu: Option<OpenMenu>,
    rename: Option<Rename>,
    input: TextEdit,
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            menu: None,
            rename: None,
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
    fn thread_menu(&mut self, thread: &str, origin: Origin, point: Option<(f32, f32)>) {
        self.commit_title();
        self.workspace.actions.menu = self
            .workspace
            .action_target(thread, origin)
            .map(|target| OpenMenu { target, point });
    }

    fn begin_rename(&mut self, id: &str, origin: Origin, tx: &mut Tx) {
        self.workspace.actions.menu = None;
        let Some(target) = self.workspace.action_target(id, origin) else {
            return;
        };
        if !self.can_operate {
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

    fn commit_title(&mut self) {
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
                .on_select_tx(move |s: &mut Desktop, tx| s.begin_rename(&id, origin, tx)),
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
