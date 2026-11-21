#[path = "activity.rs"]
mod activity;
#[path = "command.rs"]
mod command;
#[path = "images.rs"]
mod images;
#[path = "tool.rs"]
pub(crate) mod tool;
#[path = "turn_folds.rs"]
mod turn_folds;
use turn_folds::VisibleRow;
#[path = "file_diff.rs"]
pub(crate) mod file_diff;
#[path = "palette.rs"]
pub(super) mod palette;
#[path = "sidebar_header.rs"]
mod sidebar_header;
#[path = "thread_actions.rs"]
mod thread_actions;

#[path = "sidebar_status.rs"]
pub(super) mod sidebar_status;
use super::navigation::{self, Landing, Page};
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
use std::collections::{HashMap, HashSet};

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
    status: sidebar_status::RowStatus,
    status_at: Option<i64>,
    #[serde(default)]
    recedes: bool,
    #[serde(default)]
    pub(super) sort_at: i64,
    order_key: Option<String>,
    #[serde(default)]
    recent_at: f64,
    pub(super) model: String,
    pub(super) permission: Option<crate::permissions::Selection>,
    settled: bool,
    #[serde(default)]
    pinned: bool,
    snoozed_until: Option<i64>,
}

impl Thread {
    fn sidebar_order(&self, other: &Self) -> std::cmp::Ordering {
        if self.settled || other.settled {
            return self
                .settled
                .cmp(&other.settled)
                .then_with(|| other.sort_at.cmp(&self.sort_at))
                .then_with(|| self.id.cmp(&other.id));
        }
        if self.pinned != other.pinned {
            return other.pinned.cmp(&self.pinned);
        }
        let order = match (&self.order_key, &other.order_key) {
            (None, None) => other.sort_at.cmp(&self.sort_at),
            (Some(left), Some(right)) => left.cmp(right),
            // Keyless active rows precede manual order, while legacy keyless pins follow it.
            _ if self.pinned => other.order_key.cmp(&self.order_key),
            _ => self.order_key.cmp(&other.order_key),
        };
        order.then_with(|| self.id.cmp(&other.id))
    }
}

#[derive(Deserialize)]
pub(super) struct MessageRow {
    #[serde(skip)]
    preview: std::cell::OnceCell<String>,
    key: String,
    role: String,
    body: String,
    attachments: Vec<images::Attachment>,
    caption: Option<String>,
    file: Option<file_diff::Row>,
    tool: Option<tool::Tool>,
    exit_code: Option<i32>,
    #[serde(default)]
    failed: bool,
    language: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ConversationWindow {
    #[serde(default)]
    folds: Vec<turn_folds::Fold>,
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
    Folds {
        folds: Vec<turn_folds::Fold>,
    },
    Requests {
        questions: Vec<Prompt>,
        approvals: Vec<Approval>,
    },
    Splice {
        folds: Option<Vec<turn_folds::Fold>>,
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
    folds: turn_folds::Layout,
    project_root: Option<String>,
    expanded_activities: HashSet<String>,
    command: Option<command::Expanded>,
    tools: HashMap<String, tool::Open>,
    images: images::Images,
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
        self.folds.update(self.rows.len(), Some(window.folds));
        self.sequence = Some(window.sequence);
        self.earlier_pages = window.earlier_pages;
        self.cursor = window.cursor;
        self.needs_reload = false;
        self.revision += 1;
    }
    pub fn begin_watch(&mut self, id: u64) {
        self.images = Default::default();
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
            project_root: self.project_root.clone(),
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

#[derive(Deserialize)]
pub(super) struct WorkspaceChanges {
    landing_project: Option<String>,
    #[serde(default)]
    sequence: u64,
    scratch_available: bool,
    projects: Vec<Project>,
    threads: Vec<Thread>,
    removed_projects: Vec<String>,
    removed_threads: Vec<String>,
}

fn row_index<T>(rows: &[T], id: impl Fn(&T) -> &String) -> HashMap<String, usize> {
    rows.iter()
        .enumerate()
        .map(|(i, row)| (id(row).clone(), i))
        .collect()
}

pub(super) struct Workspace {
    pub(super) sequence: u64,
    pub(super) feed: Option<u64>,
    pub(super) refresh_request: Option<u64>,
    pub(super) refresh_after_edit: Option<u64>,
    actions: thread_actions::Actions,
    row_clock: sidebar_status::Clock,
    pub(super) paused: bool,
    pub(crate) diff: file_diff::Panel,
    pub questions: Forms,
    pub(super) scratch_available: bool,
    projects: Vec<Project>,
    by_project: HashMap<String, usize>,
    threads: Vec<Thread>,
    by_thread: HashMap<String, usize>,
    filtered: Vec<usize>,
    filter_revision: u64,
    settled: Vec<usize>,
    pub(super) conversation: Option<Conversation>,
    pub(super) drafts: HashMap<String, Draft>,
    pub(crate) terminal_heights: HashMap<String, f32>,
    search: TextEdit,
    search_target: Option<String>,
    project_scope: sidebar_header::ProjectScope,
    pub(super) show_projects: bool,
    show_settled: bool,
}

impl Workspace {
    pub(super) fn visible_thread(&self) -> Option<&Thread> {
        if self.show_projects {
            return None;
        }
        let id = &self.conversation.as_ref()?.id;
        self.by_thread.get(id).map(|&i| &self.threads[i])
    }
    pub(crate) fn terminal_watch(&self) -> Option<u64> {
        self.conversation.as_ref()?.watch
    }
    pub fn new() -> Self {
        Self {
            sequence: 0,
            feed: None,
            refresh_request: None,
            refresh_after_edit: None,
            actions: Default::default(),
            row_clock: Default::default(),
            paused: false,
            diff: Default::default(),
            scratch_available: false,
            projects: Vec::new(),
            by_project: HashMap::new(),
            threads: Vec::new(),
            by_thread: HashMap::new(),
            filtered: Vec::new(),
            filter_revision: 0,
            settled: Vec::new(),
            conversation: None,
            drafts: HashMap::new(),
            terminal_heights: HashMap::new(),
            questions: HashMap::new(),
            search: TextEdit::new(""),
            search_target: None,
            project_scope: Default::default(),
            show_projects: false,
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
        self.by_project = row_index(&projects, |p| &p.id);
        threads.sort_by(Thread::sidebar_order);
        self.by_thread = row_index(&threads, |t| &t.id);
        self.projects = projects;
        self.threads = threads;
        self.reconcile();
    }

    fn apply_changes(&mut self, changes: WorkspaceChanges) {
        self.sequence = changes.sequence;
        self.scratch_available = changes.scratch_available;
        for project in changes.projects {
            self.put_project(project);
        }
        if !changes.removed_projects.is_empty() {
            let removed: HashSet<_> = changes.removed_projects.into_iter().collect();
            self.projects.retain(|p| !removed.contains(&p.id));
            self.by_project = row_index(&self.projects, |p| &p.id);
        }
        let mut reorder = !changes.removed_threads.is_empty();
        for mut thread in changes.threads {
            thread.search_title = thread.title.to_lowercase();
            if let Some(&index) = self.by_thread.get(&thread.id) {
                reorder |= !thread.sidebar_order(&self.threads[index]).is_eq();
                self.threads[index] = thread;
            } else {
                self.by_thread.insert(thread.id.clone(), self.threads.len());
                self.threads.push(thread);
                reorder = true;
            }
        }
        if !changes.removed_threads.is_empty() {
            let removed: HashSet<_> = changes.removed_threads.into_iter().collect();
            self.threads.retain(|t| !removed.contains(&t.id));
        }
        if reorder {
            self.threads.sort_by(Thread::sidebar_order);
            self.by_thread = row_index(&self.threads, |t| &t.id);
        }
        self.reconcile();
    }

    fn reconcile(&mut self) {
        self.reconcile_actions();
        if self
            .conversation
            .as_ref()
            .is_some_and(|p| !self.by_thread.contains_key(&p.id))
        {
            self.conversation = None;
            self.diff = Default::default();
        }
        self.questions
            .retain(|id, _| self.by_thread.contains_key(id));
        self.terminal_heights
            .retain(|id, _| self.by_thread.contains_key(id));
        self.drafts.retain(|id, _| self.by_thread.contains_key(id));
        self.project_scope.update(&self.projects);
        self.filter();
    }

    pub fn upsert_project(&mut self, project: Project) {
        self.put_project(project);
        self.filter();
    }

    fn put_project(&mut self, mut project: Project) {
        project.search_title = project.title.to_lowercase();
        if let Some(&index) = self.by_project.get(&project.id) {
            self.projects[index] = project;
        } else {
            self.by_project
                .insert(project.id.clone(), self.projects.len());
            self.projects.push(project);
        }
    }

    pub fn insert_launched(&mut self, mut thread: Thread) {
        if self.by_thread.contains_key(&thread.id) {
            return;
        }
        thread.search_title = thread.title.to_lowercase();
        self.threads.push(thread);
        self.threads.sort_by(Thread::sidebar_order);
        self.by_thread = row_index(&self.threads, |t| &t.id);
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
        self.filtered.clear();
        self.settled.clear();
        for (index, thread) in self.threads.iter().enumerate() {
            if !self.project_scope.includes(&thread.project) {
                continue;
            }
            if query.is_empty() && thread.settled {
                self.settled.push(index);
            } else if query.is_empty()
                || thread.search_title.contains(&query)
                || self
                    .project(&thread.project)
                    .is_some_and(|p| p.search_title.contains(&query))
            {
                self.filtered.push(index);
            }
        }
        if let Some(id) = &self.search_target
            && !self
                .by_thread
                .get(id)
                .is_some_and(|i| self.filtered.contains(i))
        {
            self.search_target = None;
        }
        self.filter_revision += 1;
    }
    fn settled_rows(&self, draft_open: bool) -> &[usize] {
        if self.settled.is_empty() || self.show_settled {
            return &self.settled;
        }
        if !draft_open
            && let Some(t) = self.visible_thread()
            && t.settled
            && self.project_scope.includes(&t.project)
        {
            return std::slice::from_ref(&self.by_thread[&t.id]);
        }
        &[]
    }

    pub(super) fn has_thread(&self, id: &str) -> bool {
        self.by_thread.contains_key(id)
    }

    fn project(&self, id: &str) -> Option<&Project> {
        self.by_project.get(id).map(|&i| &self.projects[i])
    }
    pub fn close_unavailable_thread(&mut self, id: &str) {
        if self.conversation.as_ref().is_some_and(|pane| pane.id == id) {
            self.conversation = None;
            self.diff = Default::default();
        }
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
            LiveUpdate::Deleted => self.close_unavailable_thread(id),
            LiveUpdate::Window { window } => pane.replace(window, &mut self.questions),
            LiveUpdate::Status { state } if !pane.needs_reload => pane.set_status(state),
            LiveUpdate::Append { index, key, text }
                if !pane.needs_reload && pane.rows.get(index).is_some_and(|row| row.key == key) =>
            {
                let row = &mut pane.rows[index];
                if row.body.len() < activity::PREVIEW_SOURCE_BYTES {
                    row.preview.take();
                }
                row.body.push_str(&text);
                pane.sequence = Some(sequence);
            }
            LiveUpdate::Folds { folds } if !pane.needs_reload => {
                pane.folds.update(pane.rows.len(), Some(folds));
                pane.sequence = Some(sequence);
                pane.revision += 1;
            }
            LiveUpdate::Splice {
                folds,
                index,
                remove,
                rows,
            } if !pane.needs_reload
                && index <= pane.rows.len()
                && remove <= pane.rows.len() - index =>
            {
                let old_len = pane.rows.len();
                pane.rows.splice(index..index + remove, rows);
                pane.folds.resize(old_len, pane.rows.len(), folds);
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
    pub(super) fn begin_landing(&mut self, project: String) {
        if self.pending
            || !matches!(self.landing, Landing::Index)
            || !self.can_operate
            || self.project_path.is_some()
            || !self
                .workspace
                .project(&project)
                .is_some_and(|p| !p.archived)
        {
            return;
        }
        self.request_id = self.next_request_id();
        self.landing = Landing::Loading {
            project: project.clone(),
            request: self.request_id,
        };
        self.write_request(
            Some(self.request_id),
            Request::PrepareLanding {
                project: Some(project),
            },
        );
    }

    pub(super) fn leave_landing(&mut self) {
        if matches!(self.landing, Landing::Loading { .. }) {
            self.write_request(None, Request::PrepareLanding { project: None });
        }
        self.landing = Landing::Left;
    }

    pub(super) fn retry_landing(&mut self) {
        if self.landing.project().is_some() {
            self.leave_landing();
            self.landing = Landing::Index;
        }
    }

    pub(super) fn replace_workspace(&mut self, projects: Vec<Project>, threads: Vec<Thread>) {
        self.workspace.replace(projects, threads);
        self.reconcile_workspace();
    }
    pub(super) fn apply_workspace(&mut self, mut changes: WorkspaceChanges) {
        let candidate = changes.landing_project.take();
        self.workspace.apply_changes(changes);
        self.reconcile_workspace();
        if self
            .landing
            .project()
            .is_some_and(|id| !self.workspace.project(id).is_some_and(|p| !p.archived))
        {
            self.retry_landing();
        }
        if let Some(project) = candidate {
            self.begin_landing(project);
        }
    }
    fn reconcile_workspace(&mut self) {
        self.new_threads
            .retain(|id| self.workspace.project(id).is_some_and(|p| !p.archived));
        self.history.retain(&self.workspace, &self.new_threads);
        self.close_stale_terminal();
        if self.palette.is_some() {
            self.filter_palette(true);
        }
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
            .filter(|id| {
                self.workspace.project(id).is_some_and(|p| !p.archived)
                    && self.workspace.project_scope.includes(id)
            });
        let mut active = self.workspace.projects.iter().filter(|p| !p.archived);
        let single = active.next().filter(|_| active.next().is_none());
        let scoped = self.workspace.project_scope.single().map(str::to_owned);
        if let Some(id) = scoped.or(project).or_else(|| single.map(|p| p.id.clone())) {
            self.prepare_thread(&id);
        } else {
            self.show_projects();
        }
    }
    pub(super) fn open_thread(&mut self, id: &str) {
        if self.pending {
            return;
        }
        let opened = self.workspace.show_projects
            || self.new_threads.active.is_some()
            || !self.history.on_thread(id)
            || self
                .workspace
                .conversation
                .as_ref()
                .is_none_or(|pane| pane.id != id);
        if opened {
            self.cancel_settle_navigation();
        }
        self.history.visit(Page::Thread(id.into()));
        self.new_threads.active = None;
        self.workspace.show_projects = false;
        if self
            .workspace
            .conversation
            .as_ref()
            .is_some_and(|p| p.id == id)
        {
            if opened {
                self.write_request(None, Request::VisitThread { thread: Some(id) });
            }
            return;
        }
        self.workspace.drafts.entry(id.to_owned()).or_default();
        self.workspace.conversation = Some(Conversation {
            folds: Default::default(),
            project_root: self
                .workspace
                .by_thread
                .get(id)
                .and_then(|i| self.workspace.project(&self.workspace.threads[*i].project))
                .map(|p| p.path.clone()),
            expanded_activities: HashSet::new(),
            tools: HashMap::new(),
            images: Default::default(),
            command: None,
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
        if opened {
            self.write_request(None, Request::VisitThread { thread: Some(id) });
        }
    }

    pub(crate) fn close_sidebar_menus(&mut self) {
        self.workspace.project_scope.close();
        self.workspace.actions.close_menu();
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.workspace.project_scope.close();
        self.sidebar_layout.end_drag();
        self.sidebar_layout.open = !self.sidebar_layout.open;
        self.write_request(
            None,
            Request::SetSidebarOpen {
                open: self.sidebar_layout.open,
            },
        );
    }

    pub(super) fn leave_visible_thread(&mut self) {
        self.cancel_settle_navigation();
        if self.workspace.conversation.is_some() {
            self.write_request(None, Request::VisitThread { thread: None });
        }
    }

    pub(super) fn show_projects(&mut self) {
        self.leave_landing();
        self.leave_visible_thread();
        self.close_conversation_inspectors();
        self.history.visit(Page::Home);
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
            project_root: pane.project_root.clone(),
            cursor: Some(cursor),
            sequence: None,
            oldest: None,
            earlier_pages: 0,
        });
    }
}

pub(super) fn label(value: &str, size: f32, weight: f32, color: rune::Color) -> El {
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

const CLOSE_SIDEBAR: rune::Icon = rune::Icon {
    name: "panel-left-close",
    view: 24.,
    body: r#"<g stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18m7-6-3-3 3-3"/></g>"#,
};

fn hide_sidebar(theme: &tok::Theme) -> El {
    let caption = "Hide sidebar";
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
        .child(icon(&CLOSE_SIDEBAR, 16.0).color(theme.text))
        .on_press(Desktop::toggle_sidebar)
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
    s: &Desktop,
    thread: &Thread,
    project: &str,
    selected: bool,
    theme: &tok::Theme,
    dark: bool,
) -> El {
    let id = thread.id.clone();
    let recedes = thread.recedes && !selected;
    let (status, name) = sidebar_status::view(thread, dark, theme, &s.workspace.row_clock);
    let accessible = [Some(thread.title.as_str()), name, Some(project)]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let mut heading = row()
        .w_full()
        .center_y()
        .gap(6.)
        .child(project_badge(project, theme));
    if thread.pinned {
        heading = heading.child(thread_actions::pin_indicator(s, thread, theme));
    }
    let heading = heading
        .child(
            label(project, 12.0, 400.0, theme.text_secondary)
                .truncate()
                .shrink(),
        )
        .child(spacer())
        .child(status);
    let editor = thread_actions::editor(s, thread, thread_actions::Origin::Sidebar, theme);
    let editing = editor.is_some();
    let mut base = col()
        .key(&id)
        .label(&accessible)
        .role("button")
        .radius(6.0)
        .w_full()
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .child(heading)
        .child(editor.unwrap_or_else(|| {
            label(
                &thread.title,
                14.0,
                if recedes { 400. } else { 500. },
                if recedes {
                    theme.text.alpha(0.75)
                } else {
                    theme.text
                },
            )
            .truncate()
        }));
    if let Some(branch) = &thread.branch {
        base = base.child(label(branch, 12.0, 400.0, theme.text_secondary).truncate());
    }
    if selected {
        base = base.fill(theme.separator);
    }
    let base = s.workspace.row_clock.track_row(thread, base);
    if editing {
        return base;
    }
    let base = thread_actions::context(base, &id, thread_actions::Origin::Sidebar);
    if s.pending {
        base.disabled(true)
    } else {
        base.hover(theme.separator)
            .focusable(true)
            .on_press(move |s: &mut Desktop| s.select_sidebar_thread(&id))
    }
}

static NEW_THREAD_ICON: rune::Icon = rune::Icon {
    name: "new-thread",
    view: 24.0,
    body: r#"<path d="M11 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-6 M15.5 5.5l3 3 M9 15l1-4 8-8a2 2 0 0 1 3 3l-8 8-4 1Z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

impl Desktop {
    fn sidebar_rows(&self, settled: bool) -> &[usize] {
        if settled {
            self.workspace
                .settled_rows(self.new_threads.active.is_some())
        } else {
            &self.workspace.filtered
        }
    }
}

fn sidebar_rows(s: &Desktop, settled: bool, theme: &tok::Theme, dark: bool) -> El {
    let count = s.sidebar_rows(settled).len();
    let query_empty = s.workspace.search.text.trim().is_empty();
    let colors = *theme;
    col()
        .key(if settled {
            "settled-rows"
        } else {
            "thread-rows"
        })
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            76.0,
            move |s: &Desktop, i| {
                s.workspace.threads[s.sidebar_rows(settled)[i]]
                    .id
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let t = &s.workspace.threads[s.sidebar_rows(settled)[i]];
                let project = s
                    .workspace
                    .project(&t.project)
                    .map_or("", |p| p.title.as_str());
                let selected = if query_empty {
                    s.new_threads.active.is_none()
                        && !s.workspace.show_projects
                        && s.workspace
                            .conversation
                            .as_ref()
                            .is_some_and(|p| p.id == t.id)
                } else {
                    s.workspace.search_target.as_ref() == Some(&t.id)
                };
                thread_row(s, t, project, selected, &colors, dark)
            },
        )
        .virtual_rows_version((
            dark,
            s.workspace.filter_revision,
            if settled && !s.workspace.show_settled {
                s.sidebar_rows(true).first().copied()
            } else {
                None
            },
        ))
}

fn settled_header(ws: &Workspace, theme: &tok::Theme) -> El {
    let count = ws.settled.len().to_string();
    let chevron = icon(&rune_kit::glyphs::CHEVRON_DOWN, 12.)
        .rotate(if ws.show_settled {
            0.
        } else {
            -std::f32::consts::FRAC_PI_2
        })
        .color(theme.text_secondary);
    row()
        .key("settled-section")
        .role("button")
        .focusable(true)
        .label(format!("Settled, {count}"))
        .expanded(ws.show_settled)
        .w_full()
        .h(32.)
        .px(10.)
        .center_y()
        .gap(6.)
        .radius(5.)
        .hover(theme.separator)
        .child(label("Settled", 12., 500., theme.text_secondary))
        .child(spacer())
        .child(label(&count, 12., 400., theme.text_secondary))
        .child(chevron)
        .on_press(|s: &mut Desktop| s.workspace.show_settled = !s.workspace.show_settled)
}

fn sidebar(
    s: &Desktop,
    inset: f32,
    width: f32,
    theme: &tok::Theme,
    dark: bool,
    overlays: &mut Vec<El>,
) -> El {
    let compact = width - inset < 174.;
    let mut on_art = *theme;
    if s.stage.has_art() {
        on_art.text = rune::Color::WHITE;
        on_art.text_secondary = rune::Color::WHITE.alpha(0.76);
        on_art.separator = rune::Color::WHITE.alpha(0.14);
    }
    let query_empty = s.workspace.search.text.trim().is_empty();
    let count = s.workspace.filtered.len();
    let mut list = col()
        .key("thread-list")
        .drawn_rect_into(&s.workspace.row_clock.viewport)
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
            } else if count == 0 && s.workspace.settled.is_empty() {
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
                sidebar_rows(s, false, theme, dark)
            },
        );
    if !s.workspace.settled.is_empty() {
        list = list
            .child(spacer())
            .child(settled_header(&s.workspace, theme))
            .child(sidebar_rows(s, true, theme, dark));
    }
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
                .pl(if compact { inset } else { inset + 7. })
                .center_y()
                .window_drag()
                .gap(if compact { 2. } else { 9. })
                .child(hide_sidebar(&on_art))
                .child(navigation::header(s, &on_art, compact)),
        )
        .child(sidebar_header::view(s, theme, dark, overlays))
        .child(list);
    if s.workspace.paused {
        sidebar = sidebar.child(action(
            "Sidebar updates paused. Reconnect",
            theme,
            !s.pending,
            |s| s.send(Request::Refresh),
        ));
    }
    let content = sidebar.child(
        row()
            .w_full()
            .child(crate::appearance::trigger(theme))
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
    );
    col()
        .w(width)
        .h_full()
        .clip_rounded(0.)
        .children(s.stage.backdrop(dark, width))
        .child(content)
}

fn add_project(s: &mut Desktop, tx: &mut Tx) {
    if !s.can_operate || s.pending {
        return;
    }
    s.leave_landing();
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
            pane.folds.len(pane.rows.len()),
            100.0,
            |s: &Desktop, i| {
                let pane = s.workspace.conversation.as_ref().unwrap();
                match pane.folds.row(i) {
                    VisibleRow::Header(group) => pane.folds.key(group).into(),
                    VisibleRow::Message(index) => pane.rows[index].key.as_str().into(),
                }
            },
            move |s: &Desktop, i| {
                let theme = &colors;
                let pane = s.workspace.conversation.as_ref().unwrap();
                let message = match pane.folds.row(i) {
                    VisibleRow::Header(group) => return pane.folds.header(group, &pane.id, theme),
                    VisibleRow::Message(index) => &pane.rows[index],
                };
                if let Some(caption) = &message.caption {
                    if let Some(tool) = &message.tool {
                        return tool::row_view(s, message, tool, caption, theme, &message_kit);
                    }
                    if message.role != "command" {
                        return activity::row_view(
                            s,
                            message,
                            caption,
                            theme,
                            &message_kit,
                            &style,
                        );
                    }
                    return command::row_view(s, message, caption, theme, &message_kit);
                }
                let user = message.role == "user";
                let mut content = col().gap(4.);
                let mut body = col().gap(6.);
                if user {
                    content = content.max_w(640.).self_end();
                    body = body
                        .self_end()
                        .px(14.)
                        .py(9.)
                        .radius(14.)
                        .fill(theme.text.alpha(0.08));
                    if !message.body.is_empty() {
                        body = body.child(
                            label(&message.body, 14., 400., theme.text)
                                .wrap()
                                .selectable(true),
                        );
                    }
                } else {
                    content = content.w_full();
                    body = body.w_full().child(
                        Markdown::new(&message.body)
                            .style(style.clone())
                            .selectable(true)
                            .build(&message_kit),
                    );
                }
                for file in &message.attachments {
                    if let Some(key) = &file.image {
                        let thumb = images::thumbnail(s, key, &file.name, theme, 192.);
                        if user {
                            content = content.child(thumb.self_end());
                        } else {
                            body = body.child(thumb);
                        }
                    } else {
                        body = body.child(label(&file.name, 12., 500., theme.text_secondary));
                    }
                }
                if !user
                    || !message.body.is_empty()
                    || message.attachments.iter().any(|a| a.image.is_none())
                {
                    content = content.child(body);
                }
                col().w_full().py(10.0).child(content)
            },
        )
        .virtual_rows_version((&pane.id, pane.revision, dark));
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
    let viewport = cx.layout_size("workspace").unwrap_or([1100., 780.]);
    let narrow = viewport[0] < 640.;
    let composing = s.new_threads.active.as_deref();
    let mut overlays = Vec::new();
    let thread = s.workspace.visible_thread();
    let inset = cx.chrome_insets();
    let landing = s.landing.project().is_some();
    let empty = (s.workspace.projects.is_empty()
        || (matches!(s.landing, Landing::Index)
            && s.can_operate
            && !s.workspace.projects.iter().any(|p| !p.archived)))
        && s.project_path.is_none()
        && !s.pending;
    let mut header = row()
        .key("thread-titlebar")
        .w_full()
        .h(52.0)
        .window_drag()
        .center_y()
        .gap(8.0)
        .px(16.0);
    if !s.sidebar_layout.open {
        header = header.pl(inset.leading + 12.0).child(action(
            "Show sidebar",
            theme,
            true,
            Desktop::toggle_sidebar,
        ));
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
        header = header.child(thread_actions::header(
            s,
            thread,
            theme,
            &Kit::neutral(s.dark(cx)),
            &mut overlays,
        ));
    } else if !empty && !landing {
        header = header.child(label("Projects", 14.0, 500.0, theme.text));
    }
    header = header.child(spacer());
    if composing.is_none()
        && (s.workspace.show_projects || s.workspace.conversation.is_none())
        && s.project_path.is_none()
        && !empty
        && !landing
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
    if s.terminal_available() {
        header = header.child(crate::terminal::button(theme));
    }
    let refreshing_projects = s.workspace.refresh_request.is_some()
        && composing.is_none()
        && (s.workspace.show_projects || s.workspace.conversation.is_none());
    if s.pending || refreshing_projects {
        header = header.child(label("Loading…", 12.0, 400.0, theme.text_secondary));
    }
    header = header.pr(inset.trailing + 12.0).child(action(
        "Refresh",
        theme,
        !s.pending && !refreshing_projects,
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
                cx,
                &mut overlays,
            )
        } else if !s.workspace.show_projects
            && let Some(pane) = &s.workspace.conversation
        {
            timeline(s, pane, theme, s.dark(cx), &mut overlays)
        } else if matches!(s.landing, Landing::Failed(_)) {
            col()
                .grow(1.)
                .center()
                .gap(10.)
                .child(label("Couldn’t start a new thread", 18., 500., theme.text))
                .child(label(
                    "The project is still available. Try opening the draft again.",
                    14.,
                    400.,
                    theme.text_secondary,
                ))
                .child(action("Try again", theme, !s.pending, |s| {
                    s.send(Request::Refresh)
                }))
        } else if landing {
            spacer()
        } else if empty {
            empty_home(s, theme, narrow)
        } else {
            projects_view(s, theme)
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
            main = main.child(questions::banner(s, &thread.id, theme, s.dark(cx)));
        }
        main = main.child(composer::view(s, thread, theme, cx, &mut overlays));
    }
    if let Some(drawer) = crate::terminal::view(s, theme, s.dark(cx), viewport[1]) {
        main = main.child(drawer);
    }
    let diff_open = s.workspace.diff.visible(s);
    let split_panel = diff_open
        && cx
            .layout_with("workspace", |rect| rect[2] >= 980.)
            .unwrap_or(false);
    let mut root = row().key("workspace").w_full().h_full().fill(theme.bg);
    let show_sidebar = s.sidebar_layout.open && (!diff_open || split_panel);
    let bounds = crate::sidebar_layout::Bounds::new(viewport[0], inset.leading);
    let width = if narrow {
        176.
    } else {
        bounds.shown(s.sidebar_layout)
    };
    s.workspace.row_clock.mounted.set(show_sidebar);
    if show_sidebar {
        root = root
            .child(sidebar(
                s,
                inset.leading,
                width,
                theme,
                s.dark(cx),
                &mut overlays,
            ))
            .child(if narrow {
                el().w(1.).h_full().fill(theme.separator)
            } else {
                el().w(1.)
            });
    }
    thread_actions::sidebar_menu(s, &Kit::neutral(s.dark(cx)), &mut overlays);
    palette::overlay(s, cx, theme, &mut overlays);
    let workspace = if diff_open {
        let leading = if split_panel { 0. } else { inset.leading };
        let panel = file_diff::view(s, theme, &Kit::neutral(s.dark(cx)), leading, inset.trailing);
        if split_panel {
            root.child(main)
                .child(el().w(1.).h_full().fill(theme.separator))
                .child(panel)
        } else {
            root.child(panel)
        }
    } else {
        root.child(main)
    };
    let workspace = if show_sidebar && !narrow {
        workspace.child(crate::sidebar_layout::rail(width, bounds, theme))
    } else {
        workspace
    };
    images::overlay(s, theme, &mut overlays);
    crate::appearance::overlay(s, cx, theme, &mut overlays);
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
        app.terminal_read = true;
        assert!(app.terminal_available());
        app.request_id = 8;
        app.pending = true;
        assert!(!app.terminal_available());
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
                folds: None,
                index: 0,
                remove: 0,
                rows: vec![MessageRow {
                    preview: Default::default(),
                    key: "msg".into(),
                    role: "assistant".into(),
                    body: "hello".into(),
                    attachments: vec![],
                    tool: None,
                    file: None,
                    caption: None,
                    exit_code: None,
                    failed: false,
                    language: None,
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
    fn sidebar_updates_keep_pending_requests_and_ignore_replaced_subscriptions() {
        use super::super::{Reply, WorkerMessage};
        let mut app = Desktop::new();
        let thread = |id: &str, title: &str, sort_at| Thread {
            id: id.into(),
            project: "p".into(),
            title: title.into(),
            search_title: String::new(),
            branch: None,
            status: sidebar_status::RowStatus::Ready,
            status_at: None,
            recedes: false,
            sort_at,
            recent_at: sort_at as f64,
            order_key: None,
            model: String::new(),
            permission: None,
            settled: false,
            pinned: false,
            snoozed_until: None,
        };
        app.workspace
            .replace(vec![], vec![thread("a", "Original", 1)]);
        app.workspace.feed = Some(21);
        app.open_thread("a");
        app.pending = true;
        let update = |watch, threads, removed_threads| WorkerMessage {
            id: None,
            reply: Reply::WorkspaceLive {
                watch,
                changes: WorkspaceChanges {
                    landing_project: None,
                    sequence: 1,
                    scratch_available: false,
                    threads,
                    projects: vec![],
                    removed_projects: vec![],
                    removed_threads,
                },
            },
        };
        app.receive(update(
            21,
            vec![thread("a", "Renamed", 1), thread("b", "New", 2)],
            vec![],
        ));
        app.receive(update(20, vec![], vec!["a".into()]));
        assert_eq!(app.workspace.by_thread.get("a"), Some(&1));
        assert_eq!(app.workspace.threads[1].search_title, "renamed");
        assert_eq!(app.workspace.conversation.as_ref().unwrap().id, "a");
        app.workspace
            .insert_launched(thread("a", "Late launch receipt", 0));
        assert_eq!(app.workspace.threads[1].search_title, "renamed");
        let pin = |id, key: Option<&str>, created| {
            serde_json::from_value(serde_json::json!({
                "id": id, "project":"p", "title":id, "model":"m", "status":"ready",
                "settled":false, "pinned":true, "order_key":key, "sort_at":created
            }))
            .unwrap()
        };
        app.receive(update(
            21,
            vec![
                pin("a", Some("C"), 1),
                pin("c", Some("A"), 2),
                pin("d", None, 3),
            ],
            vec![],
        ));
        let ids = app
            .workspace
            .threads
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["c", "a", "d", "b"]);
        app.receive(update(21, vec![thread("a", "Unpinned", 1)], vec![]));
        assert_eq!(app.workspace.by_thread["a"], 3);
        app.workspace.conversation.as_mut().unwrap().begin_watch(22);
        app.receive(WorkerMessage {
            id: None,
            reply: Reply::Live {
                watch: 22,
                thread: "a".into(),
                sequence: 10,
                update: LiveUpdate::Deleted,
            },
        });
        assert!(app.workspace.conversation.is_none());
        assert!(
            app.workspace.has_thread("a"),
            "archive and restore may coalesce in the shell"
        );
        app.receive(update(21, vec![], vec!["a".into()]));
        assert!(app.workspace.conversation.is_none());
        assert!(app.pending);
    }

    #[test]
    fn refreshing_expanded_history_requests_the_oldest_loaded_row() {
        let mut desktop = Desktop::new();
        desktop.open_thread("thread");
        let workspace = &mut desktop.workspace;
        let message = |key: &str| MessageRow {
            preview: Default::default(),
            key: key.into(),
            role: "user".into(),
            body: key.into(),
            attachments: Vec::new(),
            tool: None,
            file: None,
            caption: None,
            exit_code: None,
            failed: false,
            language: None,
        };
        workspace.conversation.as_mut().unwrap().replace(
            ConversationWindow {
                folds: vec![],
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
