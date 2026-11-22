use super::{ContextAction, Desktop, El, Menu, MenuItem, Request, glyphs, label};
use rune::{Tx, el, icon, row, tok};
use serde::Deserialize;

pub(super) const TRIGGER: &str = "choose-project-script-trigger";

#[derive(Deserialize)]
pub(crate) struct Script {
    key: String,
    edit: String,
    run: String,
    primary: bool,
    script: Stored,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Stored {
    name: String,
    command: String,
    icon: String,
    preview_url: String,
    run_on_worktree_create: bool,
    run_on_settle: bool,
    stop_on_settle: bool,
    r#async: Option<bool>,
    auto_open_preview: bool,
}

pub(super) struct Catalog {
    watch: u64,
    project: String,
    thread: Option<String>,
    request: u64,
    available: bool,
    rows: Vec<Script>,
    imports: Vec<Stored>,
}

impl Desktop {
    fn script_target(&self) -> Option<(u64, &str, Option<&str>)> {
        self.workspace
            .details
            .settings
            .as_ref()
            .filter(|s| s.scripts)?;
        let (draft, target) = self.details_target()?;
        let project = self.workspace.project(if draft {
            target
        } else {
            &self.workspace.visible_thread()?.project
        })?;
        (!project.scratch).then_some((
            self.workspace.feed?,
            &project.id,
            (!draft).then_some(target),
        ))
    }
    pub(crate) fn sync_project_scripts(&mut self) -> bool {
        let next = self.script_target();
        let old = self.workspace.details.scripts.as_ref();
        if next == old.map(|c| (c.watch, c.project.as_str(), c.thread.as_deref())) {
            return false;
        }
        let next = next.map(|(w, p, t)| (w, p.to_owned(), t.map(str::to_owned)));
        let previous = old.map(|c| c.watch);
        self.workspace.details.script_form = None;
        self.workspace.details.scripts = None;
        if let Some((watch, project, thread)) = next {
            let request = self.next_request_id();
            if self.write_request(
                Some(request),
                Request::WatchProjectScripts {
                    watch,
                    project: Some(&project),
                    thread: thread.as_deref(),
                },
            ) {
                self.workspace.details.scripts = Some(Catalog {
                    watch,
                    project,
                    request,
                    thread,
                    available: false,
                    imports: vec![],
                    rows: vec![],
                });
            }
        } else if let Some(watch) = previous {
            self.write_request(
                None,
                Request::WatchProjectScripts {
                    watch,
                    project: None,
                    thread: None,
                },
            );
        }
        true
    }
    pub(crate) fn receive_scripts(
        &mut self,
        watch: u64,
        project: &str,
        id: Option<u64>,
        available: bool,
        rows: Vec<Script>,
        imports: Vec<Stored>,
    ) {
        if let Some(c) = self.workspace.details.scripts.as_mut()
            && c.watch == watch
            && c.project == project
            && Some(c.request) == id
        {
            c.rows = rows;
            c.imports = imports;
            c.available = available;
        }
    }
    fn current_scripts(&self) -> Option<&[Script]> {
        let c = self.workspace.details.scripts.as_ref()?;
        (self.script_target() == Some((c.watch, c.project.as_str(), c.thread.as_deref()))
            && c.available
            && (!c.rows.is_empty() || self.can_edit_scripts()))
        .then_some(&c.rows)
    }
    fn can_edit_scripts(&self) -> bool {
        self.workspace
            .details
            .settings
            .as_ref()
            .is_some_and(|s| s.script_write)
            && !self.workspace.paused
    }

    pub(super) fn has_scripts(&self) -> bool {
        self.current_scripts().is_some()
    }

    fn can_run_project_script(&self) -> bool {
        self.new_threads.active.is_none()
            && self.terminal_write
            && self.terminal_available()
            && !self.workspace.paused
            && self
                .workspace
                .conversation
                .as_ref()
                .is_some_and(|c| c.status == super::super::LiveStatus::Live)
    }

    fn launch_project_script(&mut self, key: &str, tx: &mut Tx) {
        self.workspace.details.menu = None;
        if self.can_run_project_script()
            && self
                .current_scripts()
                .is_some_and(|rows| rows.iter().any(|s| s.key == key))
        {
            self.workspace.details.popup = None;
            self.open_thread_terminal(Some(key.to_owned()), None, tx);
        }
    }
}

pub(super) fn controls(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let scripts = s.current_scripts()?;
    let primary = scripts.iter().find(|s| s.primary);
    let labels = &s.workspace.details.settings.as_ref()?.labels;
    if scripts.is_empty() && s.workspace.details.scripts.as_ref()?.imports.is_empty() {
        return Some(
            row()
                .key(TRIGGER)
                .role("button")
                .focusable(true)
                .label(&labels.form.add)
                .w_full()
                .h(32.)
                .px(10.)
                .gap(10.)
                .center_y()
                .radius(8.)
                .hover(theme.hover)
                .child(icon(&super::super::ADD_ICON, 16.).color(theme.text_secondary))
                .child(label(&labels.form.add, 13., 500., theme.text.alpha(0.8)))
                .on_press_tx(|s: &mut Desktop, tx| s.edit_script(None, tx)),
        );
    }
    let mut run = row()
        .key("run-project-script")
        .role("button")
        .focusable(true)
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .px(10.)
        .gap(10.)
        .center_y()
        .radius(8.)
        .hover(theme.hover);
    if let Some(script) = primary {
        let key = script.key.clone();
        run = run
            .label(&script.run)
            .help(&script.run)
            .disabled(!s.can_run_project_script())
            .child(icon(script_icon(&script.script.icon), 16.).color(theme.text_secondary))
            .child(
                label(&script.script.name, 13., 500., theme.text.alpha(0.8))
                    .truncate()
                    .shrink(),
            )
            .on_press_tx(move |s: &mut Desktop, tx| s.launch_project_script(&key, tx));
    } else {
        run = run
            .label(&labels.actions)
            .child(icon(script_icon("configure"), 16.).color(theme.text_secondary))
            .child(label(&labels.actions, 13., 500., theme.text.alpha(0.8)))
            .on_press(|s: &mut Desktop| s.open_details_menu(ContextAction::Scripts));
    }
    Some(
        row()
            .key("project-script-actions")
            .w_full()
            .center_y()
            .h(32.)
            .child(run)
            .child(
                el().key(TRIGGER)
                    .role("button")
                    .focusable(true)
                    .label(&labels.scripts)
                    .help(&labels.scripts)
                    .size(32., 32.)
                    .center()
                    .radius(8.)
                    .hover(theme.hover)
                    .child(icon(&glyphs::CHEVRON_DOWN, 16.).color(theme.text_secondary))
                    .on_press(|s: &mut Desktop| s.open_details_menu(ContextAction::Scripts)),
            ),
    )
}

pub(super) fn menu(s: &Desktop) -> Option<Menu> {
    let scripts = s.current_scripts()?;
    let mut menu = Menu::new("choose-project-script")
        .min_width(super::WIDTH - 16.)
        .placement(super::Placement::BottomEnd)
        .open(true)
        .on_close(|s: &mut Desktop| s.workspace.details.menu = None);
    for script in scripts {
        let key = script.key.clone();
        menu = menu.item(
            MenuItem::new(&script.script.name)
                .icon(script_icon(&script.script.icon))
                .disabled(!s.can_run_project_script())
                .on_select_tx(move |s: &mut Desktop, tx| s.launch_project_script(&key, tx)),
        );
        if s.can_edit_scripts() {
            let key = script.key.clone();
            menu = menu.item(
                MenuItem::new(&script.edit)
                    .on_select_tx(move |s: &mut Desktop, tx| s.edit_script(Some(&key), tx)),
            );
        }
    }
    if s.can_edit_scripts() {
        let imports = &s.workspace.details.scripts.as_ref()?.imports;
        if !imports.is_empty() {
            let labels = &s.workspace.details.settings.as_ref()?.labels;
            if !scripts.is_empty() {
                menu = menu.separator();
            }
            menu = menu.group_label(&labels.import_from);
            for action in imports {
                let item = action.clone();
                menu = menu.item(
                    MenuItem::new(&action.name)
                        .icon(script_icon(&action.icon))
                        .shortcut(&labels.import)
                        .on_select_tx(move |s: &mut Desktop, tx| s.import_script(&item, tx)),
                );
            }
            menu = menu.separator();
        }
        let form = &s.workspace.details.settings.as_ref()?.labels.form;
        menu = menu.item(
            MenuItem::new(&form.add)
                .icon(&super::super::ADD_ICON)
                .on_select_tx(|s: &mut Desktop, tx| s.edit_script(None, tx)),
        );
    }
    Some(menu)
}

fn script_icon(kind: &str) -> &'static rune::Icon {
    match kind {
        "test" => &rune::Icon {
            name: "FlaskConical",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2v6a2 2 0 0 0 .245.96l5.51 10.08A2 2 0 0 1 18 22H6a2 2 0 0 1-1.755-2.96l5.51-10.08A2 2 0 0 0 10 8V2"></path><path d="M6.453 15h11.094"></path><path d="M8.5 2h7"></path></g>"#,
        },
        "lint" => &rune::Icon {
            name: "ListChecks",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 5h8"></path><path d="M13 12h8"></path><path d="M13 19h8"></path><path d="m3 17 2 2 4-4"></path><path d="m3 7 2 2 4-4"></path></g>"#,
        },
        "configure" => &rune::Icon {
            name: "Wrench",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z"></path></g>"#,
        },
        "build" => &rune::Icon {
            name: "Hammer",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 12-9.373 9.373a1 1 0 0 1-3.001-3L12 9"></path><path d="m18 15 4-4"></path><path d="m21.5 11.5-1.914-1.914A2 2 0 0 1 19 8.172v-.344a2 2 0 0 0-.586-1.414l-1.657-1.657A6 6 0 0 0 12.516 3H9l1.243 1.243A6 6 0 0 1 12 8.485V10l2 2h1.172a2 2 0 0 1 1.414.586L18.5 14.5"></path></g>"#,
        },
        "debug" => &rune::Icon {
            name: "Bug",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20v-9"></path><path d="M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z"></path><path d="M14.12 3.88 16 2"></path><path d="M21 21a4 4 0 0 0-3.81-4"></path><path d="M21 5a4 4 0 0 1-3.55 3.97"></path><path d="M22 13h-4"></path><path d="M3 21a4 4 0 0 1 3.81-4"></path><path d="M3 5a4 4 0 0 0 3.55 3.97"></path><path d="M6 13H2"></path><path d="m8 2 1.88 1.88"></path><path d="M9 7.13V6a3 3 0 1 1 6 0v1.13"></path></g>"#,
        },
        _ => &rune::Icon {
            name: "Play",
            view: 24.,
            body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"></path></g>"#,
        },
    }
}

#[path = "script_form.rs"]
mod form;
pub(crate) use form::Input;
pub(crate) use form::overlay;
pub(super) use form::{Form, Labels};
