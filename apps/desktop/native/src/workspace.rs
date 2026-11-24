#[path = "activity.rs"]
mod activity;
#[path = "command.rs"]
mod command;
#[path = "images.rs"]
pub(crate) mod images;
#[path = "message.rs"]
mod message;
#[path = "project_favicon.rs"]
mod project_favicon;
#[path = "tool.rs"]
pub(crate) mod tool;
#[path = "turn_folds.rs"]
mod turn_folds;
use turn_folds::VisibleRow;
#[path = "file_diff.rs"]
pub(crate) mod file_diff;
#[path = "palette.rs"]
pub(super) mod palette;
#[path = "panel_maximize.rs"]
mod panel_maximize;
#[path = "workspace_panel_width.rs"]
pub(crate) mod panel_width;
#[path = "plan.rs"]
mod plan;
#[path = "sidebar_header.rs"]
mod sidebar_header;
#[path = "thread_actions.rs"]
pub(crate) mod thread_actions;
#[path = "thread_details.rs"]
pub(crate) mod thread_details;
#[path = "thread_find.rs"]
pub(crate) mod thread_find;
#[path = "workspace_files.rs"]
pub(crate) mod workspace_files;
#[path = "workspace_tree.rs"]
pub(crate) mod workspace_tree;

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
use rune_kit::MarkdownStyle;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
pub(super) struct Project {
    pub(super) id: String,
    icon: Option<crate::project_icon::ProjectIcon>,
    favicon: Option<project_favicon::Target>,
    pub(super) title: String,
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
    worktree: Option<String>,
    status: sidebar_status::RowStatus,
    #[serde(default = "can_archive")]
    can_archive: bool,
    title_status: Option<String>,
    status_at: Option<i64>,
    #[serde(default)]
    recedes: bool,
    #[serde(default)]
    pub(super) sort_at: i64,
    order_key: Option<String>,
    #[serde(default)]
    recent_at: f64,
    #[serde(default)]
    providers: Vec<String>,
    pub(super) model: String,
    pub(super) permission: Option<crate::permissions::Selection>,
    settled: bool,
    #[serde(default)]
    pinned: bool,
    snoozed_until: Option<i64>,
}

fn can_archive() -> bool {
    true
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
struct AssistantFileLink {
    href: String,
    copy: String,
    prefix: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct MessageRow {
    entry_id: Option<String>,
    #[serde(skip)]
    preview: std::cell::OnceCell<String>,
    key: String,
    role: String,
    body: String,
    copy_body: Option<String>,
    file_links: Option<std::sync::Arc<std::collections::HashMap<String, AssistantFileLink>>>,
    attachments: Vec<images::Attachment>,
    caption: Option<String>,
    file: Option<file_diff::Row>,
    plan: Option<plan::Plan>,
    tool: Option<tool::Tool>,
    exit_code: Option<i32>,
    #[serde(default)]
    failed: bool,
    language: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ConversationWindow {
    #[serde(default)]
    replies: HashSet<String>,
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
    Replies {
        replies: HashSet<String>,
    },
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
    replies: HashSet<String>,
    copied: Option<(String, std::time::Instant)>,
    find: Option<thread_find::Search>,
    folds: turn_folds::Layout,
    project_root: Option<String>,
    expanded_activities: HashSet<String>,
    plan_actions: Option<plan::Actions>,
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
        self.replies = window.replies;
        self.rows = window.rows;
        self.folds.update(self.rows.len(), Some(window.folds));
        self.sequence = Some(window.sequence);
        self.earlier_pages = window.earlier_pages;
        self.cursor = window.cursor;
        self.needs_reload = false;
        self.revision += 1;
    }
    pub fn begin_watch(&mut self, id: u64) {
        self.plan_actions = None;
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
    pub(super) drawer: std::cell::Cell<Option<bool>>,
    sidebar_relocated: std::cell::Cell<bool>,
    pub(super) sequence: u64,
    environment_id: Option<String>,
    pub(super) feed: Option<u64>,
    pub(super) refresh_request: Option<u64>,
    pub(super) refresh_after_edit: Option<u64>,
    pub(crate) details: thread_details::Details,
    actions: thread_actions::Actions,
    pub(crate) browser: crate::browser::Browser,
    pub(crate) issues: crate::issues::Issues,
    pub(crate) pull_requests: crate::pull_requests::PullRequests,
    pub(crate) usage: crate::usage::Panel,
    pub(crate) find_labels: Option<Box<thread_find::Labels>>,
    row_clock: sidebar_status::Clock,
    pub(super) paused: bool,
    pub(crate) diff: file_diff::Panel,
    pub(crate) resolving_file_link: Option<(u64, workspace_files::Location)>,
    pub(crate) file_preview: Option<workspace_files::Preview>,
    pub(crate) attachment_labels: crate::attachments::Labels,
    pub(crate) attachment_viewport: rune::Live<[f32; 4]>,
    pub(crate) draft_image: std::cell::RefCell<Option<crate::attachments::Preview>>,
    pub(crate) explorer: Option<workspace_tree::Explorer>,
    pub questions: Forms,
    pub(super) scratch_available: bool,
    projects: Vec<Project>,
    favicon_cache: project_favicon::Cache,
    pub(super) providers: crate::sidebar_providers::Catalog,
    by_project: HashMap<String, usize>,
    threads: Vec<Thread>,
    by_thread: HashMap<String, usize>,
    filtered: Vec<usize>,
    filter_revision: u64,
    settled: Vec<usize>,
    snoozed: Vec<usize>,
    next_wake: Option<i64>,
    pub(super) conversation: Option<Conversation>,
    pub(super) drafts: HashMap<String, Draft>,
    pub(crate) panel_width: panel_width::Widths,
    pub(crate) terminal_heights: HashMap<String, f32>,
    search: TextEdit,
    search_target: Option<String>,
    project_scope: sidebar_header::ProjectScope,
    pub(super) show_projects: bool,
    show_settled: bool,
    show_snoozed: bool,
}

impl Workspace {
    pub(super) fn set_feed(&mut self, environment_id: String, watch: u64) {
        if self.environment_id.as_ref() != Some(&environment_id) {
            self.providers = Default::default();
            self.usage.clear_results();
            self.environment_id = Some(environment_id);
        }
        self.providers.set_watch(watch);
        self.feed = Some(watch);
    }

    pub(super) fn update_providers(
        &mut self,
        watch: u64,
        instances: Vec<crate::sidebar_providers::Instance>,
        art: HashMap<String, crate::sidebar_providers::Art>,
    ) {
        self.providers.update(watch, instances, art);
        self.refresh_provider_rows();
    }

    pub(super) fn refresh_provider_rows(&mut self) {
        self.filter_revision += 1;
    }

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
            drawer: Default::default(),
            sidebar_relocated: Default::default(),
            environment_id: None,
            feed: None,
            refresh_request: None,
            refresh_after_edit: None,
            details: Default::default(),
            actions: Default::default(),
            usage: Default::default(),
            browser: Default::default(),
            issues: Default::default(),
            pull_requests: Default::default(),
            row_clock: Default::default(),
            paused: false,
            diff: Default::default(),
            resolving_file_link: None,
            file_preview: None,
            attachment_labels: Default::default(),
            attachment_viewport: rune::Live::new([0.; 4]),
            draft_image: Default::default(),
            explorer: None,
            scratch_available: false,
            projects: Vec::new(),
            favicon_cache: Default::default(),
            providers: Default::default(),
            by_project: HashMap::new(),
            threads: Vec::new(),
            by_thread: HashMap::new(),
            filtered: Vec::new(),
            filter_revision: 0,
            settled: Vec::new(),
            snoozed: Vec::new(),
            next_wake: None,
            conversation: None,
            find_labels: None,
            drafts: HashMap::new(),
            panel_width: Default::default(),
            terminal_heights: HashMap::new(),
            questions: HashMap::new(),
            search: TextEdit::new(""),
            search_target: None,
            project_scope: Default::default(),
            show_projects: false,
            show_settled: false,
            show_snoozed: false,
        }
    }
    pub fn replace(&mut self, mut projects: Vec<Project>, mut threads: Vec<Thread>) {
        for p in &mut projects {
            p.search_title = p.title.to_lowercase();
        }
        for t in &mut threads {
            t.search_title = t.title.to_lowercase();
        }
        self.favicon_cache.clear();
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
            let previous = &mut self.projects[index];
            if previous.path == project.path
                && previous.archived == project.archived
                && let (Some(old), Some(next)) = (&previous.favicon, &project.favicon)
                && old.path == next.path
            {
                project.favicon = previous.favicon.take();
            }
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
        self.snoozed.clear();
        self.next_wake = None;
        let now = sidebar_status::unix_millis().saturating_add(self.row_clock.offset);
        for (index, thread) in self.threads.iter().enumerate() {
            if !self.project_scope.includes(&thread.project) {
                continue;
            }
            let wake = thread.snoozed_until.filter(|&until| until > now);
            // Like the web sidebar, a snooze outranks settling until it wakes.
            if query.is_empty()
                && let Some(wake) = wake
            {
                self.snoozed.push(index);
                self.next_wake = Some(self.next_wake.map_or(wake, |next| next.min(wake)));
            } else if query.is_empty() && thread.settled {
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
        // Like the web Snoozed section, the soonest wake comes first.
        let threads = &self.threads;
        self.snoozed
            .sort_by_key(|&i| (threads[i].snoozed_until, threads[i].id.clone()));
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
    pub(super) fn is_snoozed(&self, thread: &Thread) -> bool {
        let now = sidebar_status::unix_millis().saturating_add(self.row_clock.offset);
        thread.snoozed_until.is_some_and(|until| until > now)
    }

    /// A shelf's rows; a closed shelf still shows the open thread, like the web sidebar.
    fn shelf_rows(&self, shelf: Shelf, draft_open: bool) -> &[usize] {
        let (rows, open) = match shelf {
            Shelf::Settled => (&self.settled, self.show_settled),
            Shelf::Snoozed => (&self.snoozed, self.show_snoozed),
        };
        if rows.is_empty() || open {
            return rows;
        }
        if !draft_open
            && let Some(t) = self.visible_thread()
            && let Some(i) = rows.iter().find(|&&i| self.threads[i].id == t.id)
        {
            return std::slice::from_ref(i);
        }
        &[]
    }

    pub(super) fn has_thread(&self, id: &str) -> bool {
        self.by_thread.contains_key(id)
    }

    pub(crate) fn project_root(&self, id: &str) -> Option<&str> {
        self.project(id)
            .filter(|project| !project.archived)
            .map(|project| project.path.as_str())
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
            LiveUpdate::Replies { replies } if !pane.needs_reload => {
                pane.replies = replies;
                pane.sequence = Some(sequence);
                pane.revision += 1;
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
    pub(super) fn sync_window_title(&mut self, tx: &mut Tx) {
        let thread = self
            .workspace
            .visible_thread()
            .filter(|_| self.new_threads.active.is_none());
        let name = thread.map_or("", |t| t.title.trim());
        let project = thread
            .and_then(|t| self.workspace.project(&t.project))
            .map_or("", |p| p.title.trim());
        let status = thread
            .and_then(|t| t.title_status.as_deref())
            .filter(|_| self.reader_title)
            .unwrap_or("");
        let prefix = if status.is_empty() { "" } else { ": " };
        let parts = if name.is_empty() {
            [status, prefix, super::product_name(), "", ""]
        } else if project.is_empty() {
            [status, prefix, name, "", ""]
        } else if self.reader_title {
            [status, prefix, name, " - ", project]
        } else {
            ["", "", project, " / ", name]
        };
        if self
            .window_title
            .as_ref()
            .is_none_or(|previous| *previous != parts)
        {
            tx.set_title(parts.concat().replace('\0', "�"));
            self.window_title = Some(parts.map(str::to_owned));
        }
    }

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
        self.reconcile_file_location();
        if self.palette.is_some() {
            self.filter_palette(true);
        }
    }
    pub(super) fn prepare_scratch(&mut self) {
        if self.can_operate && self.workspace.scratch_available && !self.pending {
            self.close_sidebar_drawer();
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
        self.close_sidebar_drawer();
        self.close_browser();
        let opened = self.workspace.show_projects
            || self.new_threads.active.is_some()
            || !self.history.on_thread(id)
            || self
                .workspace
                .conversation
                .as_ref()
                .is_none_or(|pane| pane.id != id);
        if opened {
            self.workspace.draft_image.get_mut().take();
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
            find: None,
            folds: Default::default(),
            project_root: self
                .workspace
                .by_thread
                .get(id)
                .and_then(|i| self.workspace.project(&self.workspace.threads[*i].project))
                .map(|p| p.path.clone()),
            expanded_activities: HashSet::new(),
            plan_actions: None,
            tools: HashMap::new(),
            images: Default::default(),
            command: None,
            approval_menu: None,
            approvals: vec![],
            stop_operation: None,
            stop_feedback: None,
            replies: HashSet::new(),
            copied: None,
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

    pub(super) fn reconcile_sidebar_menus(&mut self) -> bool {
        if !self.workspace.sidebar_relocated.replace(false)
            && self.workspace.row_clock.mounted.get()
        {
            return false;
        }
        let scope = self.workspace.project_scope.is_open();
        if scope {
            self.workspace.project_scope.close();
        }
        self.workspace.actions.close_sidebar_menu() || scope
    }

    pub(super) fn close_sidebar_drawer(&mut self) {
        if self.workspace.drawer.get() == Some(true) {
            self.workspace.drawer.set(Some(false));
            self.workspace.project_scope.close();
            self.workspace.actions.close_sidebar_menu();
        }
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.workspace.project_scope.close();
        self.sidebar_layout.end_drag();
        if let Some(open) = self.workspace.drawer.get() {
            if open {
                self.close_sidebar_drawer();
            } else {
                self.workspace.drawer.set(Some(true));
            }
            return;
        }
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
        self.open_index_page(Page::Home);
    }

    pub(super) fn open_index_page(&mut self, page: Page) {
        self.close_sidebar_drawer();
        self.leave_landing();
        self.leave_visible_thread();
        self.close_conversation_inspectors();
        self.history.visit(page);
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

pub(crate) fn panel_control(
    key: &'static str,
    name: &str,
    glyph: &'static rune::Icon,
    open: bool,
    theme: &tok::Theme,
) -> El {
    let button = el()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .checked(open)
        .focusable(true)
        .no_window_drag()
        .size(28., 28.)
        .radius(8.)
        .center()
        .hover(theme.hover)
        .child(icon(glyph, 16.).color(theme.text.alpha(0.8)));
    if open {
        button.fill(theme.hover)
    } else {
        button
    }
}

/// A sidebar footer icon button, sized like the web's `SidebarMenuButton size="icon"`.
pub(crate) fn footer_button(
    key: &'static str,
    caption: &str,
    glyph: &'static rune::Icon,
    theme: &tok::Theme,
    enabled: bool,
) -> El {
    let button = el()
        .key(key)
        .role("button")
        .label(caption)
        .help(caption)
        .no_window_drag()
        .size(32., 32.)
        .radius(8.)
        .center()
        .child(icon(glyph, 16.).color(theme.text_secondary));
    if enabled {
        button.hover(theme.separator).focusable(true)
    } else {
        button.disabled(true).opacity(0.64)
    }
}

pub(super) fn action(
    caption: &str,
    theme: &tok::Theme,
    enabled: bool,
    run: impl Fn(&mut Desktop) + 'static,
) -> El {
    action_with_tx(caption, theme, enabled, move |s, _| run(s))
}

pub(super) fn action_with_tx(
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

const OPEN_SIDEBAR: rune::Icon = rune::Icon {
    name: "panel-left-open",
    view: 24.,
    body: r#"<g stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18m4-12 3 3-3 3"/></g>"#,
};

const WORKTREE_ICON: rune::Icon = rune::Icon {
    name: "folder-git-2",
    view: 24.,
    body: r#"<g stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 19a5 5 0 0 1-5-5v8M9 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v5"/><circle cx="13" cy="12" r="2"/><circle cx="20" cy="19" r="2"/></g>"#,
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

fn card_heading(
    project_name: &str,
    badge: Option<El>,
    pin: Option<El>,
    status: El,
    theme: &tok::Theme,
) -> El {
    row()
        .w_full()
        .h(20.)
        .center_y()
        .gap(6.)
        .children(badge)
        .children(pin)
        .child(
            label(project_name, 12., 400., theme.text_secondary)
                .truncate()
                .shrink(),
        )
        .child(spacer())
        .child(status)
}

fn multiline_thread_card(
    thread: &Thread,
    heading: El,
    title: El,
    providers: Option<El>,
    theme: &tok::Theme,
) -> El {
    let card = col().py(8.).child(heading);
    if let Some(branch) = thread.branch.as_deref().filter(|branch| !branch.is_empty()) {
        let mut meta = row().mt(2.).h(16.).w_full().center_y().gap(6.);
        if let Some(path) = thread
            .worktree
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let description = format!("Worktree: {path} ({branch})");
            meta = meta.child(
                icon(&WORKTREE_ICON, 14.)
                    .color(theme.text_secondary)
                    .role("img")
                    .label(&description)
                    .help(description),
            );
        }
        card.child(title.mt(4.)).child(
            meta.child(
                label(branch, 12., 400., theme.text_secondary)
                    .min_w(0.)
                    .grow(1.)
                    .truncate_middle(),
            )
            .children(providers),
        )
    } else {
        card.child(
            row()
                .w_full()
                .mt(4.)
                .center_y()
                .gap(4.)
                .child(title.min_w(0.).grow(1.).shrink())
                .children(providers),
        )
    }
}

fn sidebar_border(s: &Desktop, dark: bool) -> rune::Color {
    let (border, foreground) = if dark {
        (
            rune::Color::WHITE.alpha(0.08),
            rune::color::hex("#f1f3f7").unwrap(),
        )
    } else {
        (
            rune::color::hex("#e4e4e7").unwrap(),
            rune::color::hex("#18181b").unwrap(),
        )
    };
    s.contrast.border(border, foreground)
}

fn thread_row(
    s: &Desktop,
    thread: &Thread,
    selected: bool,
    section: Option<Shelf>,
    theme: &tok::Theme,
    dark: bool,
) -> El {
    let shelf = section.is_some();
    let project = s.workspace.project(&thread.project);
    let id = thread.id.clone();
    let recedes = thread.recedes && !selected;
    let indicator = thread.indicator(shelf);
    let (status, name) = sidebar_status::view(indicator, dark, theme, &s.workspace.row_clock);
    // Like the web row, hovering swaps the time for the row's quick actions.
    let status = match thread_actions::row_actions(s, thread, section, theme) {
        // An open preset menu keeps its Snooze button showing, as the web row does.
        Some(actions) if s.workspace.snooze_menu_open(&thread.id) => row()
            .center_y()
            .justify_end()
            .child(status.opacity(0.))
            .child(actions.pin(None, Some(0.), Some(-4.), Some(0.))),
        Some(actions) => row()
            .center_y()
            .justify_end()
            .child(status.group_opacity(1., 0.))
            .child(
                actions
                    .pin(None, Some(0.), Some(-4.), Some(0.))
                    .group_opacity(0., 1.),
            ),
        None => status,
    };
    let project_name = project.map_or("", |p| p.title.as_str());
    let accessible = [Some(thread.title.as_str()), name, Some(project_name)]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let badge = project.and_then(|p| p.badge(s.workspace.feed, dark));
    let pin = thread
        .pinned
        .then(|| thread_actions::pin_indicator(s, thread, theme));
    let editor = thread_actions::editor(s, thread, thread_actions::Origin::Sidebar, theme);
    let editing = editor.is_some();
    let title = editor.unwrap_or_else(|| {
        let color = if shelf && !selected {
            theme.text_secondary
        } else if recedes {
            theme.text.alpha(0.75)
        } else {
            theme.text
        };
        row().h(20.).w_full().center_y().child(
            label(&thread.title, 14., if recedes { 400. } else { 500. }, color)
                .min_w(0.)
                .shrink()
                .truncate()
                .group_color(theme.text),
        )
    });
    let mut base = if shelf {
        row()
            .h(36.)
            .center_y()
            .gap(10.)
            .children(badge.map(|b| b.group_opacity(0.4, 1.)))
            .child(title.min_w(0.).grow(1.).shrink())
            .children(pin)
            .child(row().min_w(32.).justify_end().center_y().child(status))
    } else {
        let branch = thread
            .branch
            .as_deref()
            .is_some_and(|branch| !branch.is_empty());
        let ink = if branch {
            theme.text_secondary
        } else {
            theme.text
        };
        let colors = crate::sidebar_providers::Colors {
            rest: if recedes && !branch {
                theme.text_secondary.alpha(0.75)
            } else {
                ink
            },
            hovered: ink,
        };
        multiline_thread_card(
            thread,
            card_heading(project_name, badge, pin, status, theme),
            title,
            s.workspace
                .providers
                .stack(&thread.providers, dark, colors, theme),
            theme,
        )
    }
    .hover_group()
    .key(&id)
    .label(accessible)
    .role("button")
    .radius(6.)
    .w_full()
    .px(10.);
    if selected {
        base = if dark {
            let opacity = if s.workspace.search.text.trim().is_empty() {
                0.11
            } else {
                0.07
            };
            base.fill(theme.text.alpha(opacity))
        } else {
            let border = sidebar_border(s, false);
            base.fill(rune::Color::WHITE)
                .border(1., border.alpha(border.a * 0.7))
        };
    }
    let base = s.workspace.row_clock.track_row(thread, indicator, base);
    if editing {
        base
    } else {
        let base = thread_actions::context(base, &id, thread_actions::Origin::Sidebar);
        if s.pending {
            base.disabled(true)
        } else {
            let base = if selected {
                base
            } else {
                base.hover(if dark {
                    theme.text.alpha(0.08)
                } else {
                    rune::oklch(0.992, 0., 0.)
                })
            };
            base.focusable(true)
                .on_press(move |s: &mut Desktop| s.select_sidebar_thread(&id))
        }
    }
}

static NEW_THREAD_ICON: rune::Icon = rune::Icon {
    name: "new-thread",
    view: 24.0,
    body: r#"<path d="M11 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-6 M15.5 5.5l3 3 M9 15l1-4 8-8a2 2 0 0 1 3 3l-8 8-4 1Z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

impl Desktop {
    fn sidebar_rows(&self, shelf: Option<Shelf>) -> &[usize] {
        match shelf {
            Some(shelf) => self
                .workspace
                .shelf_rows(shelf, self.new_threads.active.is_some()),
            None => &self.workspace.filtered,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Shelf {
    Snoozed,
    Settled,
}

fn sidebar_rows(s: &Desktop, shelf: Option<Shelf>, theme: &tok::Theme, dark: bool) -> El {
    let count = s.sidebar_rows(shelf).len();
    let query_empty = s.workspace.search.text.trim().is_empty();
    let colors = *theme;
    let open = match shelf {
        Some(Shelf::Settled) => s.workspace.show_settled,
        Some(Shelf::Snoozed) => s.workspace.show_snoozed,
        None => true,
    };
    col()
        .key(match shelf {
            Some(Shelf::Settled) => "settled-rows",
            Some(Shelf::Snoozed) => "snoozed-rows",
            None => "thread-rows",
        })
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            if shelf.is_some() { 37. } else { 76. },
            move |s: &Desktop, i| {
                s.workspace.threads[s.sidebar_rows(shelf)[i]]
                    .id
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let t = &s.workspace.threads[s.sidebar_rows(shelf)[i]];
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
                let card = thread_row(s, t, selected, shelf, &colors, dark);
                let gap = if i + 1 < count { 1. } else { 0. };
                let inset = if shelf.is_some() { 0. } else { 2. };
                col().w_full().pt(inset).pb(inset + gap).child(card)
            },
        )
        .virtual_rows_version((
            dark,
            s.workspace.filter_revision,
            if open {
                None
            } else {
                s.sidebar_rows(shelf).first().copied()
            },
        ))
}

fn shelf_header(ws: &Workspace, shelf: Shelf, theme: &tok::Theme) -> El {
    let (key, name, rows, open) = match shelf {
        Shelf::Settled => ("settled-section", "Settled", &ws.settled, ws.show_settled),
        Shelf::Snoozed => ("snoozed-section", "Snoozed", &ws.snoozed, ws.show_snoozed),
    };
    let count = rows.len().to_string();
    let chevron = crate::disclosure_chevron(open, 12.).color(theme.text_secondary);
    row()
        .key(key)
        .role("button")
        .focusable(true)
        .label(format!("{name}, {count}"))
        .expanded(open)
        .w_full()
        .h(32.)
        .px(8.)
        .center_y()
        .gap(8.)
        .radius(5.)
        .hover(theme.separator)
        .child(label(name, 12., 500., theme.text_secondary))
        .child(spacer())
        .child(label(&count, 12., 400., theme.text_secondary))
        .child(chevron)
        .on_press(move |s: &mut Desktop| {
            let ws = &mut s.workspace;
            match shelf {
                Shelf::Settled => ws.show_settled = !ws.show_settled,
                Shelf::Snoozed => ws.show_snoozed = !ws.show_snoozed,
            }
        })
}

fn sidebar(
    s: &Desktop,
    bar: Topbar,
    size: [f32; 2],
    theme: &tok::Theme,
    dark: bool,
    overlays: &mut Vec<El>,
) -> El {
    let [width, height] = size;
    let theme = &tok::Theme {
        text: if dark {
            s.contrast.foreground(
                rune::color::hex("#f1f3f7").unwrap(),
                rune::Color::BLACK,
                rune::Color::WHITE,
            )
        } else {
            theme.text
        },
        ..*theme
    };
    let inset = bar.leading;
    let compact = width - inset < 174.;
    let mut on_art = *theme;
    if s.stage.has_art() {
        on_art.text = rune::Color::WHITE;
        on_art.text_secondary = rune::Color::WHITE.alpha(0.76);
        on_art.separator = rune::Color::WHITE.alpha(0.14);
    }
    let query_empty = s.workspace.search.text.trim().is_empty();
    let count = s.workspace.filtered.len();
    let shelved = !s.workspace.settled.is_empty() || !s.workspace.snoozed.is_empty();
    let show_shelf = query_empty && (count > 0 || shelved);
    let mut list = s
        .workspace
        .sidebar_viewport(col().key("thread-list"))
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
            } else if count == 0 && !shelved {
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
                sidebar_rows(s, None, theme, dark)
            },
        );
    if show_shelf {
        list = list.pb(8.).child(spacer());
        for shelf in [Shelf::Snoozed, Shelf::Settled] {
            if shelf == Shelf::Snoozed && s.workspace.snoozed.is_empty() {
                continue;
            }
            let gap = if s.sidebar_rows(Some(shelf)).is_empty() {
                0.
            } else {
                1.
            };
            list = list
                .child(col().w_full().px(2.).pb(gap).child(shelf_header(
                    &s.workspace,
                    shelf,
                    theme,
                )))
                .child(sidebar_rows(s, Some(shelf), theme, dark));
        }
    }
    let mut sidebar = col()
        .key("sidebar")
        .w(width - 1.)
        .max_w(width - 1.)
        .h_full()
        .px(8.0)
        .pb(8.0)
        .child(
            row()
                .h(bar.height)
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
    let mut footer = row().w_full().h(36.).pt(4.).gap(4.).center_y();
    let back = if s.history.on_issues() {
        s.workspace.issues.labels.as_ref().map(|l| &l.back)
    } else if s.history.on_pull_requests() {
        s.workspace.pull_requests.labels.as_ref().map(|l| &l.back)
    } else {
        None
    };
    if let Some(back) = back {
        let enabled = !s.pending && s.history.available(false);
        let button = row()
            .key("sidebar-back")
            .role("button")
            .label(back)
            .no_window_drag()
            .grow(1.)
            .min_w(0.)
            .h(32.)
            .px(10.)
            .gap(8.)
            .center_y()
            .radius(8.)
            .child(icon(&crate::navigation::BACK, 16.).color(theme.text_secondary))
            .child(label(back, 14., 500., theme.text_secondary).truncate());
        footer = footer.child(if enabled {
            button
                .hover(theme.separator)
                .focusable(true)
                .on_press_tx(|s: &mut Desktop, tx| s.navigate_history(false, tx))
        } else {
            button.disabled(true).opacity(0.64)
        });
    } else {
        footer = footer
            .child(crate::appearance::trigger(theme))
            .children(crate::pull_requests::trigger(s, theme))
            .children(crate::issues::trigger(s, theme))
            .children(crate::usage::trigger(s, theme))
            .child(spacer());
    }
    let content = sidebar.child(footer.children(crate::updates::sidebar(s, theme)));
    col()
        .w(width)
        .h_full()
        .clip_rounded(0.)
        .fill(if dark {
            rune::Color::BLACK
        } else {
            rune::oklch(0.985, 0., 0.)
        })
        .child(crate::stage::sidebar_grain(width, height))
        .children(s.stage.backdrop(dark, width))
        .child(content)
        .child(
            el().absolute(width - 1., 0.)
                .w(1.)
                .h_full()
                .fill(sidebar_border(s, dark)),
        )
}

fn add_project(s: &mut Desktop, tx: &mut Tx) {
    if !s.can_operate || s.pending {
        return;
    }
    s.close_sidebar_drawer();
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

pub(crate) static DISCONNECT_ICON: rune::Icon = rune::Icon {
    name: "disconnect",
    view: 24.,
    body: r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4m7 14 5-5-5-5m5 5H9" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
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

fn projects_view(s: &Desktop, theme: &tok::Theme, dark: bool) -> El {
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
                    .children(p.badge(s.workspace.feed, dark))
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

pub(super) fn chat_lane_inset(viewport_width: f32) -> f32 {
    if viewport_width >= 640. { 48. } else { 12. }
}

fn timeline(
    s: &Desktop,
    pane: &Conversation,
    theme: &tok::Theme,
    dark: bool,
    lane: thread_details::Lane,
    overlays: &mut Vec<El>,
) -> El {
    let kit = s.contrast.kit(dark);
    let style = MarkdownStyle::from_kit(&kit)
        .body_font(TextStyle::new(14.0, 400.0).lh(22.75))
        .text(theme.text)
        .table_full_width(true)
        .table_flush_left(true)
        .table_rules(theme.separator)
        .table_header_fill(rune::Color::TRANSPARENT)
        .table_outline(None)
        .table_cell_padding(10.0, 8.0);
    let mut content = s.chat_width.lane(col()).pt(16.);
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
    plan::overlay(s, theme, &kit, overlays);
    let message_kit = kit.clone();
    let bubble = rune::color::hex(if dark { "#1e1e1e" } else { "#ececee" }).unwrap();
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
                if let Some(plan) = &message.plan {
                    return plan.card(s, message, theme, &message_kit, &style);
                }
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
                if message.role == "user" {
                    message::prompt(s, pane, message, theme, bubble)
                } else {
                    message::reply(s, pane, message, theme, &message_kit, &style)
                }
            },
        )
        .virtual_rows_version((&pane.id, pane.revision, dark));
    content = content.child(list);
    for request in &pane.approvals {
        content = content.child(approval::view(s, request, &kit, theme, overlays));
    }
    lane.pad(
        col()
            .key(format!("timeline-{}", pane.id))
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: true }),
    )
    .child(content)
}

#[derive(Clone, Copy)]
pub(crate) struct Topbar {
    pub(crate) height: f32,
    pub(crate) leading: f32,
    pub(crate) trailing: f32,
}

pub(super) fn view(s: &Desktop, cx: &mut Cx<Desktop>, theme: &tok::Theme) -> El {
    let dark = s.dark(cx);
    let mut draft_preview = None;
    let viewport = cx.layout_size("workspace").unwrap_or([1100., 780.]);
    let narrow = viewport[0] < 640.;
    let drawer = (viewport[0] < 768.).then(|| s.workspace.drawer.get().unwrap_or(false));
    if s.workspace.drawer.replace(drawer).is_some() != drawer.is_some() {
        s.workspace.sidebar_relocated.set(true);
    }
    let composing = s.new_threads.active.as_deref();
    let mut overlays = Vec::new();
    let thread = s.workspace.visible_thread();
    let inset = cx.chrome_insets();
    #[cfg(target_os = "linux")]
    let (overlay_height, trailing) = (
        s.frame
            .visible()
            .then(|| crate::CAPTION_HEIGHT / s.zoom.factor as f32),
        inset.trailing.max(s.frame.trailing(s.zoom.factor)),
    );
    #[cfg(not(target_os = "linux"))]
    let (overlay_height, trailing) = (
        (cfg!(target_os = "windows") && (inset.leading > 0. || inset.trailing > 0.))
            .then_some(inset.height),
        inset.trailing,
    );
    let bar = Topbar {
        height: overlay_height.map_or(crate::WORKSPACE_TOPBAR_HEIGHT, |h| {
            h.ceil().max(crate::CAPTION_HEIGHT)
        }),
        leading: inset.leading,
        trailing,
    };
    let landing = s.landing.project().is_some();
    let empty = (s.workspace.projects.is_empty()
        || (matches!(s.landing, Landing::Index)
            && s.can_operate
            && !s.workspace.projects.iter().any(|p| !p.archived)))
        && s.project_path.is_none()
        && !s.pending;
    let diff_open = s.workspace.diff.visible(s);
    let panel_open = s.right_panel_open();
    let mut bounds = crate::sidebar_layout::Bounds::new(viewport[0], bar.leading);
    let sidebar_cap = panel_width::sidebar_cap(viewport[0]);
    let inline_panel = panel_open
        && (!s.sidebar_layout.open || sidebar_cap >= bounds.min)
        && cx
            .layout_with("workspace", |rect| rect[2] >= 980.)
            .unwrap_or(false);
    s.workspace
        .panel_width
        .presentation
        .available
        .set(inline_panel && thread.is_some());
    let split_panel = inline_panel && !s.workspace.panel_maximized();
    let [measured_width, toolbar_height] = cx
        .layout_size("thread-titlebar")
        .unwrap_or([0., bar.height]);
    let find_height = if s
        .workspace
        .conversation
        .as_ref()
        .is_some_and(|p| p.find.is_some())
        && composing.is_none()
        && !s.workspace.show_projects
        && !s.history.on_utility_page()
        && (!panel_open || split_panel)
    {
        thread_find::BAR_HEIGHT
    } else {
        0.
    };
    let overlays_top = toolbar_height + find_height;
    let layout = thread_details::layout(
        s,
        measured_width,
        viewport[1] - overlays_top,
        chat_lane_inset(viewport[0]),
        !panel_open || split_panel,
    );
    let kit = s.contrast.kit(dark);
    let mut header = row()
        .key("workspace-breadcrumb-bar")
        .w(0.)
        .grow(1.)
        .min_w(0.)
        .h(bar.height)
        .center_y()
        .gap(8.0)
        .pl(16.0);
    if drawer.is_some() || !s.sidebar_layout.open {
        let caption = "Show sidebar";
        header = header.pl(bar.leading + 12.).child(
            el().key(caption)
                .role("button")
                .label(caption)
                .help(caption)
                .focusable(true)
                .no_window_drag()
                .size(28., 28.)
                .center()
                .radius(5.)
                .hover(theme.separator)
                .child(icon(&OPEN_SIDEBAR, 16.).color(theme.text))
                .on_press(Desktop::toggle_sidebar),
        );
    }
    let mut breadcrumbs = row()
        .key("thread-breadcrumbs")
        .grow(1.)
        .min_w(0.)
        .center_y()
        .gap(8.);
    let show_project = cx
        .layout_size("thread-breadcrumbs")
        .is_none_or(|[width, _]| width >= 112.);
    if let Some(project) = composing.and_then(|id| s.workspace.project(id)) {
        if show_project {
            breadcrumbs = breadcrumbs
                .children(project.badge(s.workspace.feed, dark))
                .child(
                    label(&project.title, 13., 400., theme.text_secondary)
                        .truncate_middle()
                        .shrink(),
                )
                .child(label("/", 13., 400., theme.text_secondary));
        }
        breadcrumbs = breadcrumbs.child(
            label("New thread", 14., 500., theme.text)
                .truncate()
                .shrink(),
        );
    } else if !s.workspace.show_projects
        && let Some(thread) = thread
    {
        if show_project && let Some(project) = s.workspace.project(&thread.project) {
            breadcrumbs = breadcrumbs
                .children(project.badge(s.workspace.feed, dark))
                .child(
                    label(&project.title, 13., 400., theme.text_secondary)
                        .truncate_middle()
                        .shrink(),
                )
                .child(label("/", 13., 400., theme.text_secondary));
        }
        breadcrumbs = breadcrumbs.child(
            thread_actions::header(s, thread, theme, &kit, &mut overlays)
                .shrink()
                .min_w(if show_project { 48. } else { 0. }),
        );
    } else if s.history.on_issues()
        && let Some(labels) = &s.workspace.issues.labels
    {
        breadcrumbs = breadcrumbs.child(label(&labels.title, 14., 500., theme.text).truncate());
    } else if s.history.on_pull_requests()
        && let Some(labels) = &s.workspace.pull_requests.labels
    {
        breadcrumbs = breadcrumbs.child(label(&labels.title, 14., 500., theme.text).truncate());
    } else if !empty && !landing {
        breadcrumbs =
            breadcrumbs.child(label("Projects", 14., 500., theme.text).truncate().shrink());
    }
    header = header.child(breadcrumbs);
    let mut actions = Vec::new();
    if composing.is_none()
        && (s.workspace.show_projects || s.workspace.conversation.is_none())
        && s.project_path.is_none()
        && !empty
        && !landing
        && s.can_operate
    {
        actions.push(action_with_tx(
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
            actions.push(
                label(status, 12.0, 400.0, theme.text_secondary)
                    .wrap()
                    .shrink(),
            );
        }
    }
    let mut controls = Vec::from_iter(thread_details::header(
        s,
        theme,
        &kit,
        &mut overlays,
        &layout,
    ));
    if s.browser_available()
        && let Some(labels) = &s.workspace.browser.labels
    {
        controls.push(
            panel_control(
                "toggle-browser",
                &labels.title,
                &crate::browser::ICON,
                s.workspace.browser.panel.is_some(),
                theme,
            )
            .on_press_tx(Desktop::toggle_browser),
        );
    }
    if s.terminal_available() {
        controls.push(crate::terminal::button(s.terminal.is_some(), theme));
    }
    if !s.pending
        && s.can_read_files
        && composing.is_none()
        && s.workspace.terminal_watch().is_some()
        && s.thread_workspace_root().is_some()
    {
        controls.push(workspace_tree::toggle_button(
            panel_open && s.workspace.browser.panel.is_none(),
            theme,
        ));
    }
    let refreshing_projects = s.workspace.refresh_request.is_some()
        && composing.is_none()
        && (s.workspace.show_projects || s.workspace.conversation.is_none());
    if s.pending || refreshing_projects {
        actions.push(label("Loading…", 12.0, 400.0, theme.text_secondary));
    }
    if (composing.is_none() && (s.workspace.show_projects || s.workspace.conversation.is_none()))
        || s.error.is_some()
        || s.workspace.paused
        || s.workspace
            .conversation
            .as_ref()
            .is_some_and(|pane| pane.status != LiveStatus::Live || pane.watch.is_none())
    {
        actions.push(action(
            "Refresh",
            theme,
            !s.pending && !refreshing_projects,
            Desktop::refresh_visible,
        ));
    }
    let trailing = if split_panel { 0. } else { bar.trailing };
    let leading = if drawer.is_some() || !s.sidebar_layout.open {
        bar.leading
    } else {
        0.
    };
    let available = measured_width - leading - trailing;
    let panels_inline = !controls.is_empty() && available >= 184.;
    let actions_inline = !actions.is_empty() && available >= 540.;
    if !panels_inline && !actions_inline {
        header = header.w_full().pr(trailing + 12.);
    }
    let mut toolbar = row()
        .key("thread-titlebar")
        .window_drag()
        .w_full()
        .flex_wrap()
        .center_y()
        .child(header);
    if !controls.is_empty() && !s.history.on_utility_page() {
        let panels = row()
            .key("workspace-panel-buttons")
            .center_y()
            .gap(4.)
            .children(controls);
        toolbar = toolbar.child(if panels_inline {
            panels
                .ml(8.)
                .h(bar.height)
                .mr(if actions_inline { 0. } else { trailing + 12. })
        } else {
            panels.w_full().flex_wrap().px(12.).py(4.)
        });
    }
    if !actions.is_empty() && !s.history.on_utility_page() {
        let actions = row()
            .key("workspace-header-actions")
            .center_y()
            .gap(8.)
            .children(actions);
        toolbar = toolbar.child(if actions_inline {
            actions.ml(8.).h(bar.height).mr(trailing + 12.)
        } else {
            actions.w_full().flex_wrap().px(12.).py(4.)
        });
    }
    let mut main = col()
        .key("workspace-main")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(toolbar);
    if let Some(err) = &s.error {
        main = main.child(
            label(err, 13.0, 400.0, theme.text_secondary)
                .wrap()
                .px(16.0),
        );
    }
    main = main.child(if s.history.on_issues() {
        crate::issues::view(s, theme, dark, measured_width, &mut overlays)
    } else if s.history.on_pull_requests() {
        crate::pull_requests::view(s, theme, dark, measured_width, &mut overlays)
    } else if let Some(id) = composing
        && let Some(project) = s.workspace.project(id)
    {
        super::new_thread::view(
            s,
            project,
            theme,
            cx,
            layout.lane,
            &mut overlays,
            &mut draft_preview,
        )
    } else if !s.workspace.show_projects
        && let Some(pane) = &s.workspace.conversation
    {
        col()
            .grow(1.)
            .min_h(0.)
            .w_full()
            .children(thread_find::bar(s, pane, theme, dark))
            .child(timeline(s, pane, theme, dark, layout.lane, &mut overlays))
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
        projects_view(s, theme, dark)
    });
    if composing.is_none()
        && !s.workspace.show_projects
        && let Some(thread) = thread
        && let Some(pane) = &s.workspace.conversation
        && pane.sequence.is_some()
        && (s.can_operate || s.workspace.questions.contains_key(&thread.id))
    {
        if s.workspace.questions.contains_key(&thread.id) {
            main = main.child(questions::banner(s, &thread.id, theme, dark));
        }
        main = main.child(composer::view(
            s,
            thread,
            theme,
            cx,
            &mut overlays,
            layout.lane,
            &mut draft_preview,
        ));
    }
    if let Some(drawer) = crate::terminal::view(s, theme, dark, viewport[1]) {
        main = main.child(drawer);
    }
    if layout.card {
        main = main.children(thread_details::card(s, measured_width, overlays_top, theme));
    }
    if measured_width > 0.
        && let Some(banner) =
            crate::provider_status::view(s, layout.banner_width, overlays_top, theme, dark)
    {
        main = main.child(banner);
    }
    let mut root = row().key("workspace").w_full().h_full().fill(theme.bg);
    let show_sidebar = drawer.is_none() && s.sidebar_layout.open && (!panel_open || inline_panel);
    if split_panel {
        bounds.max = bounds.max.min(sidebar_cap).max(bounds.min);
    }
    let width = if drawer.is_some() {
        (viewport[0] - 12.).max(1.)
    } else {
        bounds.shown(s.sidebar_layout)
    };
    s.workspace
        .row_clock
        .mounted
        .set(show_sidebar || drawer == Some(true));
    if show_sidebar || drawer == Some(true) {
        let mut sidebar_overlays = Vec::new();
        let side = sidebar(
            s,
            bar,
            [width, viewport[1]],
            theme,
            dark,
            &mut sidebar_overlays,
        );
        if show_sidebar {
            root = root.child(side);
        } else {
            let sheet = side
                .absolute(0., 0.)
                .key("sidebar-drawer")
                .role("dialog")
                .label("Sidebar")
                .focus_return("Show sidebar")
                .modal(true, |s: &mut Desktop| s.close_sidebar_drawer());
            let layer = el()
                .absolute(0., 0.)
                .w_full()
                .h_full()
                .fill(rune::Color::BLACK.alpha(0.3))
                .child(sheet);
            overlays.insert(0, layer);
        }
        if !s.workspace.sidebar_relocated.get() {
            overlays.extend(sidebar_overlays);
            thread_actions::sidebar_menu(s, &kit, &mut overlays);
        }
    }
    thread_details::menu(s, &kit, &mut overlays);
    thread_actions::delete_dialog(s, theme, &mut overlays);
    thread_actions::archive_dialog(s, theme, &mut overlays);
    thread_actions::unpin_dialog(s, theme, &mut overlays);
    palette::overlay(s, cx, theme, &mut overlays);
    let available = viewport[0] - if show_sidebar { width } else { 0. };
    let max_panel_width = panel_width::max_width(viewport[0], available);
    let panel_width = if split_panel {
        s.workspace.panel_width.shown(s, max_panel_width)
    } else {
        available
    };
    let workspace = if panel_open {
        let bar = Topbar {
            leading: if split_panel || show_sidebar {
                0.
            } else {
                bar.leading
            },
            ..bar
        };
        let panel = if s.workspace.browser.panel.is_some() {
            crate::browser::view(s, theme, bar, &kit, &mut overlays)
        } else if diff_open {
            file_diff::view(s, theme, &kit, bar)
        } else if s.workspace.explorer.is_some() {
            workspace_tree::panel(s, theme, &kit, bar, panel_width)
        } else {
            workspace_files::view(s, theme, &kit, bar, None)
        };
        if split_panel {
            root.child(main)
                .child(el().w(1.).h_full().fill(theme.separator))
                .child(panel.grow(0.).min_w(panel_width))
                .children(panel_width::rail(
                    s,
                    panel_width,
                    max_panel_width,
                    viewport[0] - panel_width,
                ))
        } else {
            root.child(panel)
        }
    } else {
        root.child(main)
    };
    let workspace = if show_sidebar {
        workspace.child(crate::sidebar_layout::rail(width, bounds))
    } else {
        workspace
    };
    thread_details::scripts::overlay(s, theme, &kit, viewport[1], &mut overlays);
    if let Some(dialog) = draft_preview.filter(|_| !panel_open || split_panel) {
        overlays.push(dialog);
    } else {
        s.workspace.draft_image.borrow_mut().take();
        images::overlay(s, theme, &mut overlays);
    }
    crate::appearance::overlay(s, cx, theme, &mut overlays);
    crate::usage::overlay(s, cx, theme, &mut overlays);
    crate::updates::overlay(s, theme, dark, &mut overlays);
    crate::pairing_link::overlay(s, theme, dark, &mut overlays);
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
                    copy_body: None,
                    file_links: None,
                    entry_id: None,
                    preview: Default::default(),
                    key: "msg".into(),
                    role: "assistant".into(),
                    body: "hello".into(),
                    attachments: vec![],
                    tool: None,
                    file: None,
                    plan: None,
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
    fn snoozed_threads_leave_the_inbox_until_they_wake() {
        let thread = |id: &str, snoozed_until: Option<i64>| {
            serde_json::from_value(serde_json::json!({
                "id": id, "project": "p", "title": id, "model": "m", "status": "idle",
                "settled": false, "snoozed_until": snoozed_until
            }))
            .unwrap()
        };
        let now = sidebar_status::unix_millis();
        let mut ws = Workspace::new();
        ws.replace(
            vec![],
            vec![
                thread("late", Some(now + 7_200_000)),
                thread("awake", Some(now - 1)),
                thread("soon", Some(now + 60_000)),
            ],
        );
        let ids = |rows: &[usize]| {
            rows.iter()
                .map(|&i| ws.threads[i].id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&ws.filtered), ["awake"]);
        assert_eq!(ids(&ws.snoozed), ["soon", "late"]);
        assert_eq!(ws.next_wake, Some(now + 60_000));
        ws.search = rune::TextEdit::new("o");
        ws.filter();
        assert!(
            ws.snoozed.is_empty(),
            "search lists snoozed threads with the rest"
        );
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
            worktree: None,
            status: sidebar_status::RowStatus::Ready,
            can_archive: true,
            title_status: None,
            status_at: None,
            recedes: false,
            sort_at,
            recent_at: sort_at as f64,
            order_key: None,
            model: String::new(),
            providers: Vec::new(),
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
            copy_body: None,
            file_links: None,
            entry_id: None,
            preview: Default::default(),
            key: key.into(),
            role: "user".into(),
            body: key.into(),
            attachments: Vec::new(),
            tool: None,
            file: None,
            plan: None,
            caption: None,
            exit_code: None,
            failed: false,
            language: None,
        };
        workspace.conversation.as_mut().unwrap().replace(
            ConversationWindow {
                replies: HashSet::new(),
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
