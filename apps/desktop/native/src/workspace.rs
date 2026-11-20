#[path = "palette.rs"]
pub(super) mod palette;

use super::questions::{self, Forms, Prompt};
use super::{
    Desktop, Request,
    approval::{self, Approval},
    composer::{self, Draft},
};
use rune::{
    Cx, El, FileDialogOptions, GrowthSpec, ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon, row,
    spacer, text, text_input, tok,
};
use rune_kit::{Kit, Markdown, MarkdownStyle};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub(super) struct Project {
    id: String,
    title: String,
    #[serde(skip)]
    search_title: String,
    path: String,
    archived: bool,
    pub(super) scratch: bool,
}

#[derive(Deserialize)]
pub(super) struct Thread {
    pub id: String,
    pub(super) project: String,
    title: String,
    #[serde(skip)]
    search_title: String,
    branch: Option<String>,
    status: String,
    model: String,
    settled: bool,
}

#[derive(Deserialize)]
pub(super) struct MessageRow {
    key: String,
    role: String,
    body: String,
    attachments: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct ConversationWindow {
    #[serde(default)]
    questions: Vec<Prompt>,
    #[serde(default)]
    approvals: Vec<Approval>,
    active_run: Option<String>,
    rows: Vec<MessageRow>,
    cursor: Option<String>,
    sequence: u64,
    earlier_pages: usize,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum LiveStatus {
    Live,
    Reconnecting,
    Paused,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(super) enum LiveUpdate {
    Requests {
        questions: Vec<Prompt>,
        approvals: Vec<Approval>,
    },
    Splice {
        index: usize,
        remove: usize,
        rows: Vec<MessageRow>,
    },
    Append {
        index: usize,
        key: String,
        text: String,
    },
    Window {
        #[serde(flatten)]
        window: ConversationWindow,
    },
    Status {
        state: LiveStatus,
    },
    Run {
        run: Option<String>,
    },
    Deleted,
}

pub(super) struct Conversation {
    pub(super) approvals: Vec<Approval>,
    pub(super) approval_menu: Option<String>,
    pub(super) stop_operation: Option<String>,
    pub(super) stop_feedback: Option<composer::StopFeedback>,
    pub(super) active_run: Option<String>,
    pub id: String,
    pub rows: Vec<MessageRow>,
    pub cursor: Option<String>,
    pub sequence: Option<u64>,
    pub earlier_pages: usize,
    watch: Option<u64>,
    pub status: LiveStatus,
    needs_reload: bool,
    revision: u64,
}

impl Conversation {
    pub fn replace(&mut self, window: ConversationWindow, forms: &mut Forms) {
        questions::replace(forms, &self.id, window.questions);
        self.replace_approvals(window.approvals);
        self.set_active_run(window.active_run);
        self.rows = window.rows;
        self.sequence = Some(window.sequence);
        self.earlier_pages = window.earlier_pages;
        self.cursor = window.cursor;
        self.needs_reload = false;
        self.revision += 1;
    }
    pub fn begin_watch(&mut self, id: u64) {
        self.watch = Some(id);
        self.set_status(LiveStatus::Reconnecting);
    }
    fn set_status(&mut self, status: LiveStatus) {
        if status != LiveStatus::Live {
            self.approval_menu = None;
        }
        self.status = status;
    }
    fn refresh(&self) -> Request<'static> {
        Request::Thread {
            thread: self.id.clone(),
            cursor: None,
            sequence: self.sequence.filter(|_| !self.needs_reload),
            earlier_pages: self.earlier_pages,
            oldest: if self.earlier_pages > 0 {
                self.rows.first().map(|row| row.key.clone())
            } else {
                None
            },
        }
    }
}

pub(super) struct Workspace {
    pub questions: Forms,
    pub(super) scratch_available: bool,
    projects: Vec<Project>,
    by_project: HashMap<String, usize>,
    threads: Vec<Thread>,
    by_thread: HashMap<String, usize>,
    filtered: Vec<usize>,
    filter_revision: u64,
    settled_count: usize,
    pub(super) conversation: Option<Conversation>,
    pub(super) drafts: HashMap<String, Draft>,
    search: TextEdit,
    pub(super) show_projects: bool,
    sidebar: bool,
    show_settled: bool,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            scratch_available: false,
            projects: Vec::new(),
            by_project: HashMap::new(),
            threads: Vec::new(),
            by_thread: HashMap::new(),
            filtered: Vec::new(),
            filter_revision: 0,
            settled_count: 0,
            conversation: None,
            drafts: HashMap::new(),
            questions: HashMap::new(),
            search: TextEdit::new(""),
            show_projects: false,
            sidebar: true,
            show_settled: false,
        }
    }
    pub fn replace(&mut self, mut projects: Vec<Project>, mut threads: Vec<Thread>) {
        for p in &mut projects {
            p.search_title = p.title.to_lowercase();
        }
        for t in &mut threads {
            t.search_title = t.title.to_lowercase();
        }
        self.by_project = projects
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        self.by_thread = threads
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        if self
            .conversation
            .as_ref()
            .is_some_and(|p| !self.by_thread.contains_key(&p.id))
        {
            self.conversation = None;
        }
        self.questions
            .retain(|id, _| self.by_thread.contains_key(id));
        self.drafts.retain(|id, _| self.by_thread.contains_key(id));
        self.settled_count = threads.iter().filter(|t| t.settled).count();
        self.projects = projects;
        self.threads = threads;
        self.filter();
    }

    pub fn upsert_project(&mut self, mut project: Project) {
        project.search_title = project.title.to_lowercase();
        if let Some(&index) = self.by_project.get(&project.id) {
            self.projects[index] = project;
        } else {
            self.by_project
                .insert(project.id.clone(), self.projects.len());
            self.projects.push(project);
        }
        self.filter();
    }

    pub fn insert_launched(&mut self, mut thread: Thread) {
        thread.search_title = thread.title.to_lowercase();
        if let Some(&i) = self.by_thread.get(&thread.id) {
            self.threads.remove(i);
        }
        self.threads.insert(0, thread);
        self.by_thread = self
            .threads
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        self.settled_count = self.threads.iter().filter(|t| t.settled).count();
        self.filter();
    }
    pub(super) fn launch_projects(&self) -> Vec<rune_kit::ComboboxOption> {
        self.projects
            .iter()
            .filter(|p| !p.archived)
            .map(|p| rune_kit::ComboboxOption::new(&p.id, &p.title))
            .collect()
    }

    fn filter(&mut self) {
        let query = self.search.text.trim().to_lowercase();
        self.filtered = self
            .threads
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let matches = if query.is_empty() {
                    self.show_settled || !t.settled
                } else {
                    t.search_title.contains(&query)
                        || self
                            .project(&t.project)
                            .is_some_and(|p| p.search_title.contains(&query))
                };
                matches.then_some(i)
            })
            .collect();
        self.filter_revision += 1;
    }
    pub(super) fn thread_count(&self) -> usize {
        self.threads.len()
    }

    fn project(&self, id: &str) -> Option<&Project> {
        self.by_project.get(id).map(|&i| &self.projects[i])
    }
    pub fn apply_live(&mut self, watch: u64, id: &str, sequence: u64, update: LiveUpdate) {
        let Some(pane) = &mut self.conversation else {
            return;
        };
        if pane.id != id || pane.watch != Some(watch) {
            return;
        }
        match update {
            LiveUpdate::Requests {
                approvals,
                questions,
            } => {
                questions::replace(&mut self.questions, id, questions);
                pane.replace_approvals(approvals);
                pane.sequence = Some(sequence);
            }
            LiveUpdate::Run { run } => {
                pane.set_active_run(run);
                pane.sequence = Some(sequence);
            }
            LiveUpdate::Deleted => {
                self.conversation = None;
                self.drafts.remove(id);
                self.questions.remove(id);
                if let Some(index) = self.by_thread.remove(id) {
                    let removed = self.threads.remove(index);
                    self.settled_count -= usize::from(removed.settled);
                    for i in index..self.threads.len() {
                        self.by_thread.insert(self.threads[i].id.clone(), i);
                    }
                    self.filter();
                }
            }
            LiveUpdate::Window { window } => pane.replace(window, &mut self.questions),
            LiveUpdate::Status { state } if !pane.needs_reload => pane.set_status(state),
            LiveUpdate::Append { index, key, text }
                if !pane.needs_reload && pane.rows.get(index).is_some_and(|row| row.key == key) =>
            {
                pane.rows[index].body.push_str(&text);
                pane.sequence = Some(sequence);
            }
            LiveUpdate::Splice {
                index,
                remove,
                rows,
            } if !pane.needs_reload
                && index <= pane.rows.len()
                && remove <= pane.rows.len() - index =>
            {
                pane.rows.splice(index..index + remove, rows);
                pane.sequence = Some(sequence);
                pane.revision += 1;
            }
            _ => {
                pane.needs_reload = true;
                pane.set_status(LiveStatus::Paused);
            }
        }
    }
}

impl Desktop {
    pub(super) fn replace_workspace(&mut self, projects: Vec<Project>, threads: Vec<Thread>) {
        self.workspace.replace(projects, threads);
        self.new_threads
            .retain(|id| self.workspace.project(id).is_some_and(|p| !p.archived));
    }
    pub(super) fn prepare_scratch(&mut self) {
        if self.can_operate && self.workspace.scratch_available && !self.pending {
            self.send(Request::PrepareScratch);
        }
    }

    pub(super) fn new_thread_in_workspace(&mut self) {
        if self.pending || !self.can_operate {
            return;
        }
        let project = self
            .new_threads
            .active
            .clone()
            .or_else(|| {
                self.workspace
                    .conversation
                    .as_ref()
                    .and_then(|c| self.workspace.by_thread.get(&c.id))
                    .map(|i| self.workspace.threads[*i].project.clone())
            })
            .filter(|id| self.workspace.project(id).is_some_and(|p| !p.archived));
        let mut active = self.workspace.projects.iter().filter(|p| !p.archived);
        let single = active.next().filter(|_| active.next().is_none());
        if let Some(id) = project.or_else(|| single.map(|p| p.id.clone())) {
            self.prepare_thread(&id);
        } else {
            self.workspace.show_projects = true;
            self.new_threads.active = None;
        }
    }
    pub(super) fn open_thread(&mut self, id: &str) {
        if self.pending {
            return;
        }
        self.new_threads.active = None;
        self.workspace.show_projects = false;
        if self
            .workspace
            .conversation
            .as_ref()
            .is_some_and(|p| p.id == id)
        {
            return;
        }
        self.workspace.drafts.entry(id.to_owned()).or_default();
        self.workspace.conversation = Some(Conversation {
            approval_menu: None,
            approvals: vec![],
            stop_operation: None,
            stop_feedback: None,
            active_run: None,
            id: id.to_owned(),
            rows: Vec::new(),
            cursor: None,
            sequence: None,
            earlier_pages: 0,
            watch: None,
            status: LiveStatus::Reconnecting,
            needs_reload: false,
            revision: 0,
        });
        self.refresh_thread();
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.workspace.sidebar = !self.workspace.sidebar;
    }

    fn show_projects(&mut self) {
        self.new_threads.active = None;
        self.workspace.show_projects = true;
    }

    fn refresh_visible(&mut self) {
        if let Some(id) = self.new_threads.active.clone() {
            self.prepare_thread(&id);
        } else if self.workspace.conversation.is_some() && !self.workspace.show_projects {
            self.refresh_thread();
        } else {
            self.send(Request::Refresh);
        }
    }

    pub(super) fn refresh_thread(&mut self) {
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        self.send(pane.refresh());
    }

    fn older(&mut self) {
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        let Some(cursor) = pane.cursor.clone() else {
            return;
        };
        let thread = pane.id.clone();
        self.send(Request::Thread {
            thread,
            cursor: Some(cursor),
            sequence: None,
            oldest: None,
            earlier_pages: 0,
        });
    }
}

fn label(value: &str, size: f32, weight: f32, color: rune::Color) -> El {
    text(value).font(TextStyle::new(size, weight)).color(color)
}

pub(super) fn action(
    caption: &str,
    theme: &tok::Theme,
    enabled: bool,
    run: impl Fn(&mut Desktop) + 'static,
) -> El {
    action_with_tx(caption, theme, enabled, move |s, _| run(s))
}

fn action_with_tx(
    caption: &str,
    theme: &tok::Theme,
    enabled: bool,
    run: impl Fn(&mut Desktop, &mut Tx) + 'static,
) -> El {
    let base = el()
        .key(caption)
        .label(caption)
        .role("button")
        .no_window_drag()
        .h(28.0)
        .px(7.0)
        .radius(5.0)
        .center()
        .child(label(caption, 12.0, 500.0, theme.text_secondary));
    if enabled {
        base.hover(theme.separator).focusable(true).on_press_tx(run)
    } else {
        base.disabled(true).opacity(0.5)
    }
}

fn toggle_sidebar(show: bool, theme: &tok::Theme) -> El {
    let caption = if show { "Show sidebar" } else { "Hide sidebar" };
    el().key(caption)
        .role("button")
        .label(caption)
        .help(caption)
        .no_window_drag()
        .size(28.0, 28.0)
        .center()
        .radius(5.0)
        .hover(theme.separator)
        .focusable(true)
        .child(icon(&rune_kit::shell::SIDEBAR, 16.0).color(theme.text_secondary))
        .on_press(move |s: &mut Desktop| s.workspace.sidebar = show)
}

fn project_badge(title: &str, theme: &tok::Theme) -> El {
    let initials = title
        .split_whitespace()
        .take(2)
        .filter_map(|part| part.chars().next())
        .collect::<String>()
        .to_uppercase();
    let blue = tok::accent(0, true);
    el().size(16.0, 16.0)
        .radius(4.0)
        .fill(blue.alpha(0.2))
        .center()
        .child(label(&initials, 8.0, 600.0, theme.text_secondary))
}

fn thread_row(
    thread: &Thread,
    project: &str,
    selected: bool,
    pending: bool,
    theme: &tok::Theme,
) -> El {
    let id = thread.id.clone();
    let status = match thread.status.as_str() {
        "completed" => "Done",
        "running" => "Working",
        "waiting" => "Waiting",
        "failed" => "Failed",
        "queued" => "Queued",
        "preparing" => "Preparing",
        "starting" => "Starting",
        "interrupted" => "Interrupted",
        "cancelled" => "Cancelled",
        "rolled_back" => "Rolled back",
        _ => "",
    };
    let heading = row()
        .w_full()
        .center_y()
        .gap(6.0)
        .child(project_badge(project, theme))
        .child(
            label(project, 12.0, 400.0, theme.text_secondary)
                .truncate()
                .shrink(),
        )
        .child(spacer())
        .child(label(
            status,
            12.0,
            500.0,
            if status == "Done" {
                rune::oklch(0.75, 0.14, 165.0)
            } else {
                theme.text_secondary
            },
        ));
    let mut base = col()
        .key(&id)
        .label(&thread.title)
        .role("button")
        .radius(6.0)
        .w_full()
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .child(heading)
        .child(label(&thread.title, 14.0, 500.0, theme.text).truncate());
    if let Some(branch) = &thread.branch {
        base = base.child(label(branch, 12.0, 400.0, theme.text_secondary).truncate());
    }
    if selected {
        base = base.fill(theme.separator);
    }
    if pending {
        base.disabled(true)
    } else {
        base.hover(theme.separator)
            .focusable(true)
            .on_press(move |s: &mut Desktop| s.open_thread(&id))
    }
}

static NEW_THREAD_ICON: rune::Icon = rune::Icon {
    name: "new-thread",
    view: 24.0,
    body: r#"<path d="M11 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-6 M15.5 5.5l3 3 M9 15l1-4 8-8a2 2 0 0 1 3 3l-8 8-4 1Z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

fn sidebar(s: &Desktop, inset: f32, width: f32, theme: &tok::Theme) -> El {
    let search = text_input(&s.workspace.search, TextStyle::new(13.0, 400.0))
        .key("thread-search")
        .label("Search threads")
        .w_full()
        .h(34.0)
        .px(10.0)
        .color(theme.text)
        .placeholder("Search", theme.text_secondary)
        .single_line()
        .on_edit(|s: &mut Desktop| &mut s.workspace.search)
        .on_input_change(|s: &mut Desktop, _| s.workspace.filter());
    let query_empty = s.workspace.search.text.trim().is_empty();
    let count = s.workspace.filtered.len();
    let colors = *theme;
    let rows = col()
        .key("thread-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            76.0,
            move |s: &Desktop, i| {
                s.workspace.threads[s.workspace.filtered[i]]
                    .id
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let t = &s.workspace.threads[s.workspace.filtered[i]];
                let project = s
                    .workspace
                    .project(&t.project)
                    .map_or("", |p| p.title.as_str());
                let selected = s.new_threads.active.is_none()
                    && !s.workspace.show_projects
                    && s.workspace
                        .conversation
                        .as_ref()
                        .is_some_and(|p| p.id == t.id);
                thread_row(t, project, selected, s.pending, &colors)
            },
        )
        .virtual_rows_version(s.workspace.filter_revision);
    let list = col()
        .key("thread-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            if query_empty && s.workspace.projects.is_empty() && !s.pending {
                let mut empty = col().w_full().pt(12.0).gap(6.0).center().child(label(
                    "No projects yet",
                    12.0,
                    400.0,
                    theme.text_secondary,
                ));
                if s.can_operate {
                    empty = empty.child(
                        home_button("Add project", &ADD_ICON, false, theme, true, add_project)
                            .key("sidebar-add-project"),
                    );
                }
                empty
            } else if count == 0 {
                label(
                    if query_empty {
                        "No threads"
                    } else {
                        "No matching threads"
                    },
                    13.0,
                    400.0,
                    theme.text_secondary,
                )
                .padding(10.0)
            } else {
                rows
            },
        );
    let settled = s.workspace.settled_count;
    let mut sidebar = col()
        .key("sidebar")
        .w(width)
        .max_w(width)
        .h_full()
        .px(8.0)
        .pb(8.0)
        .child(
            row()
                .h(52.0)
                .w_full()
                .pl(inset)
                .center_y()
                .window_drag()
                .gap(4.0)
                .child(toggle_sidebar(false, theme))
                .child(label("T3 Code", 14.0, 600.0, theme.text)),
        )
        .child(
            row()
                .h(48.0)
                .w_full()
                .center_y()
                .px(7.0)
                .gap(4.0)
                .child(icon(&rune_kit::glyphs::SEARCH, 16.0).color(theme.text_secondary))
                .child(search.shrink())
                .children(s.can_operate.then(|| {
                    let item = el()
                        .key("new-thread")
                        .label("New thread")
                        .role("button")
                        .help("New thread")
                        .size(26.0, 26.0)
                        .radius(5.0)
                        .center()
                        .child(icon(&NEW_THREAD_ICON, 16.0).color(theme.text_secondary));
                    if s.pending {
                        item.disabled(true)
                    } else {
                        item.focusable(true)
                            .hover(theme.separator)
                            .on_press(Desktop::new_thread_in_workspace)
                    }
                })),
        )
        .child(list);
    if settled > 0 && query_empty {
        sidebar = sidebar.child(
            action(&format!("Settled  {settled}"), theme, true, |s| {
                s.workspace.show_settled = !s.workspace.show_settled;
                s.workspace.filter();
            })
            .self_start(),
        );
    }
    sidebar.child(
        row()
            .w_full()
            .child(action(
                "Projects",
                theme,
                !s.pending,
                Desktop::show_projects,
            ))
            .child(spacer())
            .child(action("Disconnect", theme, !s.pending, |s| {
                s.send(Request::Disconnect)
            })),
    )
}

fn add_project(s: &mut Desktop, tx: &mut Tx) {
    if !s.can_operate || s.pending {
        return;
    }
    if !s.local {
        s.project_path.get_or_insert_with(|| TextEdit::new(""));
        tx.focus("project-path", true);
        return;
    }
    let request_id = s.request_id;
    tx.open_folder(
        FileDialogOptions {
            title: "Add project".into(),
            ..Default::default()
        },
        move |s: &mut Desktop, result, _| {
            if !s.local || !s.can_operate || s.request_id != request_id {
                return;
            }
            match result {
                Ok(Some(path)) => match path.into_os_string().into_string() {
                    Ok(path) => s.send(Request::AddProject { path }),
                    Err(_) => {
                        s.error = Some("This folder name cannot be displayed as text.".into())
                    }
                },
                Ok(None) => {}
                Err(_) => s.error = Some("Couldn’t open the folder picker.".into()),
            }
        },
    );
}

fn submit_project_path(s: &mut Desktop) {
    if !s.can_operate || s.local {
        return;
    }
    if let Some(field) = &s.project_path
        && !field.text.trim().is_empty()
    {
        s.send(Request::AddProject {
            path: field.text.trim().to_owned(),
        });
    }
}

fn empty_home(s: &Desktop, theme: &tok::Theme, narrow: bool) -> El {
    let write = s.can_operate;
    let scratch = write && s.workspace.scratch_available;
    let mut buttons = if narrow { col() } else { row().flex_wrap() }
        .w_full()
        .max_w(450.0)
        .gap(8.0)
        .center();
    if write {
        buttons = buttons.child(home_button(
            "Add project",
            &ADD_ICON,
            true,
            theme,
            !s.pending,
            add_project,
        ));
    }
    if scratch {
        buttons = buttons.child(home_button(
            "Start without a project",
            &SCRATCH_ICON,
            false,
            theme,
            !s.pending,
            |s, _| s.prepare_scratch(),
        ));
    }
    let intro = col()
        .w_full()
        .center()
        .gap(4.0)
        .child(
            label(
                "What should we work on?",
                if narrow { 24.0 } else { 30.0 },
                600.0,
                theme.text,
            )
            .w_full()
            .wrap()
            .align(rune::Align::Center),
        )
        .child(
            label(
                if scratch {
                    "Add a project, or start without one."
                } else if write {
                    "Add a project to start your first thread."
                } else {
                    "No projects in this environment."
                },
                14.0,
                400.0,
                theme.text_secondary,
            )
            .w_full()
            .wrap()
            .align(rune::Align::Center),
        );
    col()
        .px(8.0)
        .min_w(0.0)
        .w_full()
        .grow(1.0)
        .center()
        .gap(24.0)
        .child(intro)
        .child(buttons)
}

static ADD_ICON: rune::Icon = rune::Icon {
    name: "add-project",
    view: 24.0,
    body: r#"<path d="M12 5v14m-7-7h14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};
static SCRATCH_ICON: rune::Icon = rune::Icon {
    name: "scratch-thread",
    view: 24.0,
    body: r#"<path d="M8 3H6a3 3 0 0 0-3 3v2m0 3v3m0 3v4l4-2m3 0h3m3 0h2a3 3 0 0 0 3-3m0-3v-3m0-3V6a3 3 0 0 0-3-3m-3 0h-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>"#,
};
fn home_button(
    caption: &str,
    glyph: &'static rune::Icon,
    primary: bool,
    theme: &tok::Theme,
    enabled: bool,
    press: impl Fn(&mut Desktop, &mut Tx) + 'static,
) -> El {
    let fg = if primary {
        rune::Color::WHITE
    } else {
        theme.text
    };
    let mut button = row()
        .label(caption)
        .role("button")
        .key(caption)
        .h(28.0)
        .px(9.0)
        .gap(5.0)
        .center()
        .radius(8.0)
        .child(icon(glyph, 16.0).color(fg))
        .child(label(caption, 13.0, 500.0, fg));
    button = if primary {
        button.fill(rune::oklch(0.56, 0.19, 259.0))
    } else {
        button.border(1.0, theme.separator)
    };
    if enabled {
        button.focusable(true).on_press_tx(press)
    } else {
        button.disabled(true).opacity(0.5)
    }
}

fn projects_view(s: &Desktop, theme: &tok::Theme) -> El {
    let colors = *theme;
    let list = col()
        .key("project-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.workspace.projects.len(),
            65.0,
            |s: &Desktop, i| s.workspace.projects[i].id.as_str().into(),
            move |s: &Desktop, i| {
                let p = &s.workspace.projects[i];
                let theme = &colors;
                let mut title = row()
                    .w_full()
                    .center_y()
                    .gap(8.0)
                    .child(label(&p.title, 14.0, 600.0, theme.text).truncate().shrink());
                if p.archived {
                    title = title.child(label("Archived", 12.0, 400.0, theme.text_secondary));
                }
                if s.can_operate {
                    let id = p.id.clone();
                    if !p.archived {
                        let project = id.clone();
                        title = title.child(action("New thread", theme, !s.pending, move |s| {
                            s.prepare_thread(&project)
                        }));
                    }
                    let archived = !p.archived;
                    title = title.child(spacer()).child(action(
                        if archived { "Archive" } else { "Restore" },
                        theme,
                        !s.pending,
                        move |s| {
                            s.send(Request::ArchiveProject {
                                project: id.clone(),
                                archived,
                            });
                        },
                    ));
                }
                col()
                    .key(&p.id)
                    .w_full()
                    .py(9.0)
                    .gap(3.0)
                    .child(title)
                    .child(label(&p.path, 13.0, 400.0, theme.text_secondary).truncate_middle())
            },
        );
    let mut area = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if s.workspace.projects.is_empty() && !s.pending {
        area = area.child(label("No projects", 14.0, 400.0, theme.text_secondary));
    }
    if let Some(environment) = &s.environment {
        area = area.child(label(environment, 13.0, 400.0, theme.text_secondary).py(8.0));
    }
    if let Some(field) = &s.project_path {
        let input = text_input(field, TextStyle::new(14.0, 400.0))
            .key("project-path")
            .label("Folder path on this environment")
            .single_line()
            .grow(1.0)
            .min_w(0.0)
            .h(32.0)
            .px(8.0)
            .radius(5.0)
            .border(1.0, theme.separator)
            .color(theme.text)
            .placeholder("Folder path on this environment", theme.text_secondary)
            .disabled(s.pending)
            .on_edit(|s: &mut Desktop| s.project_path.as_mut().unwrap())
            .on_submit(|s: &mut Desktop, _| submit_project_path(s));
        area = area.child(
            row()
                .w_full()
                .center_y()
                .gap(6.0)
                .py(5.0)
                .child(input)
                .child(action(
                    "Add",
                    theme,
                    !s.pending && !field.text.trim().is_empty(),
                    submit_project_path,
                ))
                .child(action("Cancel", theme, !s.pending, |s| {
                    s.project_path = None;
                    s.error = None;
                })),
        );
    }
    area.child(list)
}

fn timeline(
    s: &Desktop,
    pane: &Conversation,
    theme: &tok::Theme,
    dark: bool,
    overlays: &mut Vec<El>,
) -> El {
    let kit = Kit::neutral(dark);
    let style = MarkdownStyle::from_kit(&kit)
        .body_font(TextStyle::new(14.0, 400.0).lh(22.0))
        .text(theme.text)
        .table_full_width(true)
        .table_flush_left(true)
        .table_rules(theme.separator)
        .table_header_fill(rune::Color::TRANSPARENT)
        .table_outline(None)
        .table_cell_padding(10.0, 8.0);
    let mut content = col().w_full().max_w(736.0).self_center();
    if pane.cursor.is_some() {
        content = content.child(action(
            "Load earlier messages",
            theme,
            !s.pending,
            Desktop::older,
        ));
    }
    if pane.sequence.is_none() && !s.pending {
        content = content.child(action(
            "Retry conversation",
            theme,
            true,
            Desktop::refresh_thread,
        ));
    }
    if pane.sequence.is_some() && pane.rows.is_empty() {
        content =
            content.child(label("No messages", 14.0, 400.0, theme.text_secondary).key("empty"));
    }
    let colors = *theme;
    let message_kit = kit.clone();
    let list = col()
        .key("message-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            pane.rows.len(),
            100.0,
            |s: &Desktop, i| {
                s.workspace.conversation.as_ref().unwrap().rows[i]
                    .key
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let theme = &colors;
                let message = &s.workspace.conversation.as_ref().unwrap().rows[i];
                let mut content = col().gap(6.0);
                if message.role == "user" {
                    content = content
                        .max_w(640.0)
                        .self_end()
                        .px(14.0)
                        .py(9.0)
                        .radius(14.0)
                        .fill(theme.text.alpha(0.08))
                        .child(
                            label(&message.body, 14.0, 400.0, theme.text)
                                .wrap()
                                .selectable(true),
                        );
                } else {
                    content = content.w_full().child(
                        Markdown::new(&message.body)
                            .style(style.clone())
                            .selectable(true)
                            .build(&message_kit),
                    );
                }
                for file in &message.attachments {
                    content = content.child(label(file, 12.0, 500.0, theme.text_secondary));
                }
                col().w_full().py(10.0).child(content)
            },
        )
        .virtual_rows_version((&pane.id, pane.revision));
    content = content.child(list);
    for request in &pane.approvals {
        content = content.child(approval::view(s, request, &kit, theme, overlays));
    }
    col()
        .key(format!("timeline-{}", pane.id))
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(24.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(content)
}

pub(super) fn view(s: &Desktop, cx: &mut Cx<Desktop>, theme: &tok::Theme) -> El {
    let narrow = cx
        .layout_with("workspace", |rect| rect[2] < 640.0)
        .unwrap_or(false);
    let composing = s.new_threads.active.as_deref();
    let mut overlays = Vec::new();
    let thread = s.workspace.conversation.as_ref().and_then(|p| {
        s.workspace
            .by_thread
            .get(&p.id)
            .map(|&i| &s.workspace.threads[i])
    });
    let inset = cx.chrome_insets();
    let empty = s.workspace.projects.is_empty() && s.project_path.is_none() && !s.pending;
    let mut header = row()
        .key("thread-titlebar")
        .w_full()
        .h(52.0)
        .window_drag()
        .center_y()
        .gap(8.0)
        .px(16.0);
    if !s.workspace.sidebar {
        header = header
            .pl(inset.leading + 12.0)
            .child(action("Show sidebar", theme, true, |s| {
                s.workspace.sidebar = true
            }));
    }
    if let Some(project) = composing.and_then(|id| s.workspace.project(id)) {
        header = header
            .child(project_badge(&project.title, theme))
            .child(label(&project.title, 13.0, 400.0, theme.text_secondary))
            .child(label("/  New thread", 14.0, 500.0, theme.text));
    } else if !s.workspace.show_projects
        && let Some(thread) = thread
    {
        if let Some(project) = s.workspace.project(&thread.project) {
            header = header
                .child(project_badge(&project.title, theme))
                .child(label(&project.title, 13.0, 400.0, theme.text_secondary).truncate())
                .child(label("/", 13.0, 400.0, theme.text_secondary));
        }
        header = header.child(
            label(&thread.title, 14.0, 500.0, theme.text)
                .truncate()
                .shrink(),
        );
    } else if !empty {
        header = header.child(label("Projects", 14.0, 500.0, theme.text));
    }
    header = header.child(spacer());
    if composing.is_none()
        && (s.workspace.show_projects || s.workspace.conversation.is_none())
        && s.project_path.is_none()
        && !empty
        && s.can_operate
    {
        header = header.child(action_with_tx(
            "Add project",
            theme,
            !s.pending,
            add_project,
        ));
    }
    if composing.is_none()
        && !s.pending
        && !s.workspace.show_projects
        && let Some(pane) = &s.workspace.conversation
        && pane.watch.is_some()
    {
        let status = match pane.status {
            LiveStatus::Live => None,
            LiveStatus::Reconnecting => Some("Connecting…"),
            LiveStatus::Paused => Some("Live updates paused. Refresh to reconnect."),
        };
        if let Some(status) = status {
            header = header.child(label(status, 12.0, 400.0, theme.text_secondary));
        }
    }
    if s.pending {
        header = header.child(label("Loading…", 12.0, 400.0, theme.text_secondary));
    }
    header = header.pr(inset.trailing + 12.0).child(action(
        "Refresh",
        theme,
        !s.pending,
        Desktop::refresh_visible,
    ));
    let mut main = col().grow(1.0).min_w(0.0).h_full().child(header);
    if let Some(err) = &s.error {
        main = main.child(
            label(err, 13.0, 400.0, theme.text_secondary)
                .wrap()
                .px(16.0),
        );
    }
    main = main.child(
        if let Some(id) = composing
            && let Some(project) = s.workspace.project(id)
        {
            super::new_thread::view(
                s,
                id,
                &project.title,
                project.scratch,
                theme,
                &Kit::neutral(cx.system_dark()),
                &mut overlays,
            )
        } else if !s.workspace.show_projects
            && let Some(pane) = &s.workspace.conversation
        {
            timeline(s, pane, theme, cx.system_dark(), &mut overlays)
        } else {
            if empty {
                empty_home(s, theme, narrow)
            } else {
                projects_view(s, theme)
            }
        },
    );
    if composing.is_none()
        && !s.workspace.show_projects
        && let Some(thread) = thread
        && let Some(pane) = &s.workspace.conversation
        && pane.sequence.is_some()
        && (s.can_operate || s.workspace.questions.contains_key(&thread.id))
    {
        if s.workspace.questions.contains_key(&thread.id) {
            main = main.child(questions::banner(s, &thread.id, theme, cx.system_dark()));
        }
        main = main.child(composer::view(
            s,
            &thread.id,
            &thread.model,
            theme,
            cx.system_dark(),
        ));
    }
    let mut root = row().key("workspace").w_full().h_full().fill(theme.bg);
    if s.workspace.sidebar {
        root = root
            .child(sidebar(
                s,
                inset.leading,
                if narrow { 176.0 } else { 256.0 },
                theme,
            ))
            .child(el().w(1.0).h_full().fill(theme.separator));
    }
    palette::overlay(s, cx, theme, &mut overlays);
    let mut workspace = root.child(main);
    if s.palette.is_some() {
        workspace = workspace.blur(3.0);
    }
    rune_kit::root(workspace, overlays)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_patches_belong_to_one_watch_and_do_not_complete_a_request() {
        use super::super::{Reply, WorkerMessage};
        let mut app = Desktop::new();
        app.open_thread("thread");
        let pane = app.workspace.conversation.as_mut().unwrap();
        pane.approval_menu = Some("on-previous-watch".into());
        pane.begin_watch(7);
        assert!(pane.approval_menu.is_none());
        pane.status = LiveStatus::Live;
        pane.approval_menu = Some("current".into());
        app.request_id = 8;
        app.pending = true;
        let mut receive = |watch, update| {
            app.receive(WorkerMessage {
                id: None,
                reply: Reply::Live {
                    watch,
                    thread: "thread".into(),
                    sequence: 12,
                    update,
                },
            })
        };
        receive(
            7,
            LiveUpdate::Splice {
                index: 0,
                remove: 0,
                rows: vec![MessageRow {
                    key: "msg".into(),
                    role: "assistant".into(),
                    body: "hello".into(),
                    attachments: vec![],
                }],
            },
        );
        receive(
            7,
            LiveUpdate::Run {
                run: Some("current".into()),
            },
        );
        receive(6, LiveUpdate::Run { run: None });
        receive(6, LiveUpdate::Deleted);
        receive(
            7,
            LiveUpdate::Append {
                index: 0,
                key: "msg".into(),
                text: " world".into(),
            },
        );
        receive(
            7,
            LiveUpdate::Append {
                index: 0,
                key: "wrong".into(),
                text: "stale".into(),
            },
        );
        receive(
            7,
            LiveUpdate::Status {
                state: LiveStatus::Live,
            },
        );
        let pane = app.workspace.conversation.as_ref().unwrap();
        assert_eq!(pane.active_run.as_deref(), Some("current"));
        assert_eq!(pane.rows[0].body, "hello world");
        assert!(matches!(
            pane.refresh(),
            Request::Thread { sequence: None, .. }
        ));
        assert!(pane.status == LiveStatus::Paused);
        assert!(pane.approval_menu.is_none());
        assert!(app.pending);
    }

    #[test]
    fn refreshing_expanded_history_requests_the_oldest_loaded_row() {
        let mut desktop = Desktop::new();
        desktop.open_thread("thread");
        let workspace = &mut desktop.workspace;
        let message = |key: &str| MessageRow {
            key: key.into(),
            role: "user".into(),
            body: key.into(),
            attachments: Vec::new(),
        };
        workspace.conversation.as_mut().unwrap().replace(
            ConversationWindow {
                questions: vec![],
                approvals: vec![],
                active_run: None,
                rows: vec![message("earlier"), message("latest")],
                sequence: 10,
                earlier_pages: 1,
                cursor: None,
            },
            &mut workspace.questions,
        );
        desktop.open_thread("thread");
        let pane = desktop.workspace.conversation.as_ref().unwrap();
        let Request::Thread {
            cursor,
            sequence,
            oldest,
            earlier_pages,
            ..
        } = pane.refresh()
        else {
            panic!("expected thread refresh")
        };
        assert_eq!(sequence, Some(10));
        assert_eq!(earlier_pages, 1);
        assert_eq!(oldest.as_deref(), Some("earlier"));
        assert!(cursor.is_none());
        assert_eq!(pane.rows.len(), 2);
    }
}
