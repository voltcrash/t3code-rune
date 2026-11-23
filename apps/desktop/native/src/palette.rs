use super::workspace_files::{Input as FileInput, Location};
use super::{Desktop, NEW_THREAD_ICON, SCRATCH_ICON, Workspace, add_project, label};
use rune::{
    Cx, El, GrowthSpec, Icon, Key, KeyEvent, ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon,
    row, spacer, text_input, tok,
};
use rune_kit::glyphs;
use std::borrow::Cow;

#[derive(Clone, Copy)]
enum Action {
    NewThread(usize),
    StartInProject(usize),
    PickProject,
    Scratch,
    AddProject,
    Projects,
    Issues,
    Back,
    Forward,
    ToggleSidebar,
    ToggleTerminal,
    GoToFile,
    ToggleBrowser,
    TogglePanelMaximized,
    ToggleFiles,
    FindThread,
    ToggleDetails,
    OpenEditor,
    OpenFile,
    Settings,
    CheckUpdates,
    Usage,
    Refresh,
    Project(usize),
    Thread(usize),
}

impl Action {
    fn key(self, ws: &Workspace) -> String {
        match self {
            Self::NewThread(i) | Self::StartInProject(i) => {
                format!("new-thread-{}", ws.projects[i].id)
            }
            Self::Thread(i) => format!("thread-{}", ws.threads[i].id),
            Self::Project(i) => format!("project-{}", ws.projects[i].id),
            Self::PickProject => "pick-project".into(),
            Self::Scratch => "scratch".into(),
            Self::AddProject => "add-project".into(),
            Self::Projects => "projects".into(),
            Self::Issues => "issues".into(),
            Self::CheckUpdates => "check-updates".into(),
            Self::Settings => "settings".into(),
            Self::Usage => "usage".into(),
            Self::Back => "go-back".into(),
            Self::Forward => "go-forward".into(),
            Self::ToggleSidebar => "toggle-sidebar".into(),
            Self::Refresh => "refresh".into(),
            Self::ToggleTerminal => "toggle-terminal".into(),
            Self::GoToFile => "go-to-file".into(),
            Self::ToggleBrowser => "toggle-browser".into(),
            Self::TogglePanelMaximized => "maximize-workspace-panel".into(),
            Self::ToggleFiles => "toggle-files".into(),
            Self::FindThread => "find-in-thread".into(),
            Self::ToggleDetails => "toggle-thread-details".into(),
            Self::OpenEditor => "open-editor".into(),
            Self::OpenFile => unreachable!(),
        }
    }

    fn title(self, ws: &Workspace) -> Cow<'_, str> {
        match self {
            Self::NewThread(i) => return format!("New thread in {}", ws.projects[i].title).into(),
            Self::PickProject => "New thread in...",
            Self::Scratch => "New thread without a project",
            Self::AddProject => "Add project",
            Self::Projects => "Projects",
            Self::Issues => {
                return ws
                    .issues
                    .labels
                    .as_ref()
                    .map_or("Issues", |l| l.title.as_str())
                    .into();
            }
            Self::CheckUpdates => "Check for Updates…",
            Self::Settings => "Settings",
            Self::Usage => return ws.usage.title().into(),
            Self::GoToFile => "Go to file",
            Self::TogglePanelMaximized => ws.maximize_label(),
            Self::ToggleBrowser => {
                return ws
                    .browser
                    .labels
                    .as_ref()
                    .map_or("Browser", |l| l.title.as_str())
                    .into();
            }
            Self::ToggleFiles => "Toggle right panel",
            Self::OpenEditor => {
                return ws
                    .details
                    .editor
                    .as_ref()
                    .map_or("Open in editor", |c| c.open_label())
                    .into();
            }
            Self::FindThread => {
                return ws
                    .find_labels
                    .as_ref()
                    .map_or("Find in thread", |l| &l.title)
                    .into();
            }
            Self::ToggleDetails => {
                return ws
                    .details
                    .settings
                    .as_ref()
                    .map_or("Toggle thread details panel", |settings| {
                        settings.toggle_label()
                    })
                    .into();
            }
            Self::OpenFile => unreachable!(),
            Self::Back => "Go back",
            Self::Forward => "Go forward",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::Refresh => "Refresh",
            Self::ToggleTerminal => "Toggle terminal drawer",
            Self::Project(i) | Self::StartInProject(i) => &ws.projects[i].title,
            Self::Thread(i) => &ws.threads[i].title,
        }
        .into()
    }

    fn hint(self, ws: &Workspace) -> &str {
        match self {
            Self::PickProject | Self::Project(_) => "›",
            Self::Scratch if cfg!(target_os = "macos") => "⌥⌘N",
            Self::Scratch => "Ctrl+Alt+N",
            Self::Back if cfg!(target_os = "macos") => "⌘[",
            Self::Back => "Ctrl+[",
            Self::Forward if cfg!(target_os = "macos") => "⌘]",
            Self::Forward => "Ctrl+]",
            Self::ToggleSidebar if cfg!(target_os = "macos") => "⌘B",
            Self::ToggleSidebar => "Ctrl+B",
            Self::ToggleTerminal if cfg!(target_os = "macos") => "⌘J",
            Self::ToggleTerminal => "Ctrl+J",
            Self::GoToFile if cfg!(target_os = "macos") => "⌘P",
            Self::GoToFile => "Ctrl+P",
            Self::OpenEditor if cfg!(target_os = "macos") => "⌘O",
            Self::OpenEditor => "Ctrl+O",
            Self::FindThread if cfg!(target_os = "macos") => "⌘F",
            Self::FindThread => "Ctrl+F",
            Self::ToggleDetails if cfg!(target_os = "macos") => "⇧⌘B",
            Self::ToggleDetails => "Ctrl+Shift+B",
            Self::ToggleBrowser if cfg!(target_os = "macos") => "⇧⌘J",
            Self::ToggleBrowser => "Ctrl+Shift+J",
            Self::ToggleFiles if cfg!(target_os = "macos") => "⌥⌘B",
            Self::ToggleFiles => "Ctrl+Alt+B",
            Self::Thread(i) => ws.project(&ws.threads[i].project).map_or("", |p| &p.title),
            _ => "",
        }
    }

    fn icon(&self) -> &'static Icon {
        match self {
            Self::NewThread(_) | Self::StartInProject(_) | Self::PickProject => &NEW_THREAD_ICON,
            Self::Thread(_) | Self::Scratch => &SCRATCH_ICON,
            Self::AddProject | Self::Project(_) | Self::Projects => &glyphs::FOLDER,
            Self::GoToFile | Self::OpenFile => &glyphs::SEARCH,
            Self::TogglePanelMaximized => &super::panel_maximize::MAXIMIZE,
            Self::ToggleBrowser => &crate::browser::ICON,
            Self::ToggleFiles => &PANEL_ICON,
            Self::FindThread => &glyphs::SEARCH,
            Self::ToggleDetails => &DETAILS_ICON,
            Self::OpenEditor => &super::thread_details::editor::OPEN,
            Self::Settings => &crate::appearance::GEAR,
            Self::Usage => &crate::usage::CHART,
            Self::Issues => &crate::issues::ICON,
            Self::Back => &crate::navigation::BACK,
            Self::Forward => &crate::navigation::FORWARD,
            Self::ToggleSidebar => &rune_kit::shell::SIDEBAR,
            Self::Refresh | Self::CheckUpdates => &REFRESH,
            Self::ToggleTerminal => &crate::terminal::ICON,
        }
    }
}

const DETAILS_ICON: Icon = Icon {
    name: "thread-details",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M7 8h.01M10 8h7M7 12h.01M10 12h7M7 16h.01M10 16h7"/></g>"#,
};

static PANEL_ICON: Icon = Icon {
    name: "panel-right",
    view: 24.,
    body: r#"<rect x="3" y="3" width="18" height="18" rx="2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M15 3v18" fill="none" stroke="currentColor" stroke-width="1.7"/>"#,
};

pub(crate) static REFRESH: Icon = Icon {
    name: "refresh-workspace",
    view: 24.0,
    body: r#"<path d="M20 7v5h-5 M4 17v-5h5 M5.6 8.4a7.5 7.5 0 0 1 12.6-2l1.8 2 M4 15.6l1.8 2a7.5 7.5 0 0 0 12.6-2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

enum Page {
    Root,
    PickProject,
    Project(String),
    Files {
        location: Location,
        operation: u64,
        status: Option<String>,
    },
}

struct Available {
    find: bool,
    browser: bool,
    terminal: bool,
    files: bool,
    details: bool,
    editor: bool,
}

struct Item {
    key: String,
    group: &'static str,
    action: Action,
}

pub(crate) struct Palette {
    query: TextEdit,
    page: Page,
    parent_query: String,
    items: Vec<Item>,
    selected: Option<usize>,
    pointer: Option<rune::PointerPress>,
    revision: u64,
}

impl Palette {
    pub(crate) fn reveal_selection(&self, tx: &mut Tx) {
        if let Some(i) = self.selected {
            tx.scroll_into_view(format!("palette-rows/{}", self.items[i].key));
        }
    }

    fn action_for(&self, key: &str) -> Option<Action> {
        self.items
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.action)
    }

    fn new() -> Self {
        Self {
            query: TextEdit::new(""),
            page: Page::Root,
            parent_query: String::new(),
            items: Vec::new(),
            selected: None,
            pointer: None,
            revision: 0,
        }
    }

    fn filter(
        &mut self,
        ws: &Workspace,
        active: Option<&str>,
        write: bool,
        available: Available,
        history: &crate::navigation::History,
        keep_selection: bool,
    ) {
        if matches!(self.page, Page::Files { .. }) {
            return;
        }
        let query = self.query.text.trim().to_lowercase();
        let mut items = Vec::new();
        let mut command = |action: Action| {
            if action.title(ws).to_lowercase().contains(&query) {
                items.push(Item {
                    key: action.key(ws),
                    group: "Actions",
                    action,
                });
            }
        };
        let start_here = |id: &str| {
            ws.by_project
                .get(id)
                .copied()
                .filter(|&i| !ws.projects[i].archived)
        };
        match &self.page {
            Page::Root => {
                if write {
                    if let Some(i) = active.and_then(start_here) {
                        command(Action::NewThread(i));
                    }
                    if ws.projects.iter().any(|p| !p.archived && !p.scratch) {
                        command(Action::PickProject);
                    }
                    if ws.scratch_available {
                        command(Action::Scratch);
                    }
                    command(Action::AddProject);
                }
                if history.available(false) {
                    command(Action::Back);
                }
                if history.available(true) {
                    command(Action::Forward);
                }
                command(Action::Projects);
                if ws.issues.labels.is_some() {
                    command(Action::Issues);
                }
                command(Action::Settings);
                if ws.usage.labels.is_some() {
                    command(Action::Usage);
                }
                command(Action::CheckUpdates);
                command(Action::ToggleSidebar);
                if available.editor {
                    command(Action::OpenEditor);
                }
                if available.find {
                    command(Action::FindThread);
                }
                if available.details {
                    command(Action::ToggleDetails);
                }
                if ws.panel_width.presentation.available.get() {
                    command(Action::TogglePanelMaximized);
                }
                if available.browser {
                    command(Action::ToggleBrowser);
                }
                if available.files {
                    command(Action::GoToFile);
                    command(Action::ToggleFiles);
                }
                if available.terminal {
                    command(Action::ToggleTerminal);
                }
                command(Action::Refresh);
            }
            Page::Project(id) if write => {
                if let Some(i) = start_here(id) {
                    command(Action::NewThread(i));
                }
            }
            _ => {}
        }
        if !matches!(self.page, Page::Project(_)) {
            for (i, p) in ws.projects.iter().enumerate() {
                if !p.archived && !p.scratch && p.search_title.contains(&query) {
                    let action = if matches!(self.page, Page::PickProject) {
                        Action::StartInProject(i)
                    } else {
                        Action::Project(i)
                    };
                    items.push(Item {
                        key: action.key(ws),
                        group: "Projects",
                        action,
                    });
                }
            }
        }
        if !matches!(self.page, Page::PickProject) {
            let recent = matches!(self.page, Page::Root) && query.is_empty();
            let mut threads: Vec<_> = ws
                .threads
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    if let Page::Project(id) = &self.page
                        && &t.project != id
                    {
                        return false;
                    }
                    if recent && history.on_thread(&t.id) {
                        return false;
                    }
                    t.search_title.contains(&query)
                        || ws
                            .project(&t.project)
                            .is_some_and(|p| p.search_title.contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
            if recent {
                let by_recency = |&a: &usize, &b: &usize| {
                    ws.threads[b]
                        .recent_at
                        .total_cmp(&ws.threads[a].recent_at)
                        .then(ws.threads[a].id.cmp(&ws.threads[b].id))
                };
                if threads.len() > 12 {
                    threads.select_nth_unstable_by(12, by_recency);
                    threads.truncate(12);
                }
                threads.sort_unstable_by(by_recency);
            }
            for i in threads {
                items.push(Item {
                    key: Action::Thread(i).key(ws),
                    group: if recent { "Recent threads" } else { "Threads" },
                    action: Action::Thread(i),
                });
            }
        }
        self.selected = if keep_selection {
            self.selected
                .and_then(|i| items.iter().position(|item| item.key == self.items[i].key))
        } else {
            (!items.is_empty()).then_some(0)
        };
        self.items = items;
        self.revision = self.revision.wrapping_add(1);
    }
}

impl Desktop {
    pub(crate) fn file_search_location(&self) -> Option<&Location> {
        match &self.palette.as_ref()?.page {
            Page::Files { location, .. } => Some(location),
            _ => None,
        }
    }

    fn cancel_file_search(&mut self) {
        if let Some(Page::Files {
            location,
            operation,
            ..
        }) = self.palette.as_ref().map(|p| &p.page)
        {
            self.file_request(
                location.clone(),
                *operation,
                FileInput::Search {
                    query: String::new(),
                    cancel: true,
                },
            );
        }
    }

    pub(crate) fn dismiss_palette(&mut self) {
        self.cancel_file_search();
        self.palette = None;
    }

    pub(crate) fn toggle_file_picker(&mut self, tx: &mut Tx) {
        if self.file_search_location().is_some() {
            self.dismiss_palette();
            return;
        }
        let Some(location) = self.file_location() else {
            return;
        };
        if self.palette.is_none() {
            self.toggle_palette(tx);
        }
        self.palette_page(
            Page::Files {
                location,
                operation: 0,
                status: None,
            },
            tx,
        );
    }

    fn search_files(&mut self) {
        let next = self.next_request_id();
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let Page::Files {
            location,
            operation,
            status,
        } = &mut p.page
        else {
            return;
        };
        *operation = next;
        p.items.clear();
        p.selected = None;
        p.revision += 1;
        *status = Some("Searching files…".into());
        let loc = location.clone();
        let query = p.query.text.trim().to_owned();
        if query.encode_utf16().count() > 256 {
            *status = Some("Search is limited to 256 characters.".into());
            self.cancel_file_search();
            return;
        }
        self.file_request(
            loc,
            next,
            FileInput::Search {
                query,
                cancel: false,
            },
        );
    }

    pub(crate) fn receive_file_matches(
        &mut self,
        operation: u64,
        paths: Vec<String>,
        error: Option<String>,
    ) {
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let Page::Files {
            operation: current,
            status,
            ..
        } = &mut p.page
        else {
            return;
        };
        if *current != operation {
            return;
        }
        p.items = paths
            .into_iter()
            .map(|path| Item {
                key: path,
                group: "Files",
                action: Action::OpenFile,
            })
            .collect();
        *status = error.or_else(|| p.items.is_empty().then(|| "No matching files.".into()));
        p.selected = (!p.items.is_empty()).then_some(0);
        p.revision += 1;
    }

    pub(crate) fn toggle_palette(&mut self, tx: &mut Tx) {
        if self.palette.is_some() {
            self.dismiss_palette();
            return;
        }
        if self.settings_open || self.environment.is_none() || self.pending {
            return;
        }
        self.workspace.project_scope.close();
        self.permission_menu = None;
        self.models.close();
        self.palette = Some(Palette::new());
        self.filter_palette(false);
        tx.focus("palette-query", true);
    }

    pub(crate) fn filter_palette(&mut self, keep_selection: bool) {
        let active = self.new_threads.active.as_deref().or_else(|| {
            let pane = self.workspace.conversation.as_ref()?;
            self.workspace
                .by_thread
                .get(&pane.id)
                .map(|&i| self.workspace.threads[i].project.as_str())
        });
        let available = Available {
            browser: self.browser_available(),
            terminal: self.terminal_available(),
            files: self.file_location().is_some(),
            find: self.can_find_thread(),
            details: self.can_show_details(),
            editor: self.can_open_editor(),
        };
        if let Some(palette) = self.palette.as_mut() {
            palette.filter(
                &self.workspace,
                active,
                self.can_operate,
                available,
                &self.history,
                keep_selection,
            );
        }
    }

    fn palette_page(&mut self, page: Page, tx: &mut Tx) {
        self.cancel_file_search();
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        if matches!(page, Page::Root) {
            p.query = TextEdit::new(std::mem::take(&mut p.parent_query));
        } else {
            p.parent_query = p.query.text.clone();
            p.query = TextEdit::new("");
        }
        p.page = page;
        if matches!(p.page, Page::Files { .. }) {
            self.search_files();
        } else {
            self.filter_palette(false);
        }
        tx.focus("palette-query", true);
        tx.scroll_to("palette-scroll", 0.0, 0.0);
    }

    fn choose_palette(&mut self, action: Action, key: &str, tx: &mut Tx) {
        match action {
            Action::GoToFile => return self.toggle_file_picker(tx),
            Action::OpenFile => {
                if let Some(location) = self.file_search_location().cloned() {
                    self.dismiss_palette();
                    self.open_workspace_file(location, key.to_owned());
                }
                return;
            }
            Action::PickProject => return self.palette_page(Page::PickProject, tx),
            Action::Project(i) => {
                return self.palette_page(Page::Project(self.workspace.projects[i].id.clone()), tx);
            }
            _ => {}
        }
        self.palette = None;
        match action {
            Action::NewThread(i) | Action::StartInProject(i) => {
                self.prepare_thread(&self.workspace.projects[i].id.clone())
            }
            Action::Scratch => self.prepare_scratch(),
            Action::Thread(i) => self.open_thread(&self.workspace.threads[i].id.clone()),
            Action::Back => self.navigate_history(false, tx),
            Action::Forward => self.navigate_history(true, tx),
            Action::ToggleSidebar => self.toggle_sidebar(),
            Action::ToggleTerminal => self.toggle_terminal(tx),
            Action::ToggleBrowser => self.toggle_browser(tx),
            Action::TogglePanelMaximized => self.toggle_panel_maximized(tx),
            Action::ToggleFiles => self.toggle_files_panel(tx),
            Action::FindThread => {
                let focus = if self.can_operate
                    && self
                        .workspace
                        .visible_thread()
                        .is_some_and(|t| !self.workspace.questions.contains_key(&t.id))
                {
                    "message-input"
                } else {
                    "thread-actions-trigger"
                };
                self.open_thread_find(Some(focus), tx)
            }
            Action::ToggleDetails => self.toggle_details(tx),
            Action::OpenEditor => self.open_editor(),
            Action::Refresh => self.refresh_visible(),
            Action::AddProject => {
                self.show_projects();
                add_project(self, tx);
            }
            Action::Projects => self.show_projects(),
            Action::Issues => self.show_issue_list(tx),
            Action::Settings => self.open_settings(tx),
            Action::Usage => self.open_usage(),
            Action::CheckUpdates => self.check_updates(tx),
            Action::PickProject | Action::Project(_) | Action::GoToFile | Action::OpenFile => {
                unreachable!()
            }
        }
    }
}

fn key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(p) = s.palette.as_mut() else {
        return false;
    };
    match ev.key {
        Key::Down | Key::Up => {
            if !p.items.is_empty() {
                p.selected = Some(match (ev.key == Key::Down, p.selected) {
                    (true, None) => 0,
                    (true, Some(i)) => (i + 1) % p.items.len(),
                    (false, None | Some(0)) => p.items.len() - 1,
                    (false, Some(i)) => i - 1,
                });
                p.reveal_selection(tx);
            }
        }
        Key::Enter => {
            if !tx.key_repeat()
                && let Some(i) = p.selected
            {
                let action = p.items[i].action;
                let path = p.items[i].key.clone();
                s.choose_palette(action, &path, tx);
            }
        }
        Key::Backspace if p.query.text.is_empty() && !matches!(p.page, Page::Root) => {
            s.palette_page(Page::Root, tx)
        }
        Key::Escape => s.dismiss_palette(),
        _ => return false,
    }
    true
}

pub(super) fn overlay(s: &Desktop, cx: &Cx<Desktop>, theme: &tok::Theme, overlays: &mut Vec<El>) {
    let Some(p) = &s.palette else { return };
    let colors = *theme;
    let bounds = cx.layout_size("workspace").unwrap_or([1100.0, 780.0]);
    let top = (bounds[1] * if bounds[0] < 640.0 { 0.04 } else { 0.10 }).max(48.0);
    let mut content_height: f32 = 12.0;
    let mut group = "";
    for item in p.items.iter().take(11) {
        if item.group != group {
            content_height += 30.0;
            group = item.group;
        }
        content_height += 32.0;
    }
    let height = (89.0 + content_height.max(52.0))
        .min(420.0)
        .min(bounds[1] - top - 16.0);
    let placeholder = if matches!(p.page, Page::Files { .. }) {
        "Search files"
    } else if matches!(p.page, Page::Root) {
        "Search commands, projects, and threads..."
    } else {
        "Search..."
    };
    let query = text_input(&p.query, TextStyle::new(14.0, 400.0))
        .key("palette-query")
        .label(placeholder)
        .placeholder(placeholder, theme.text_secondary)
        .single_line()
        .min_w(0.0)
        .grow(1.0)
        .h(20.0)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.palette.as_mut().unwrap().query)
        .on_input_change(|s: &mut Desktop, tx| {
            if s.file_search_location().is_some() {
                s.search_files();
            } else {
                s.filter_palette(false);
            }
            tx.scroll_to("palette-scroll", 0.0, 0.0);
        })
        .on_input_key(key);
    let mut header = row().w_full().h(48.0).px(16.0).gap(8.0).center_y();
    if !matches!(p.page, Page::Root) {
        header = header.child(
            super::action_with_tx("‹", theme, true, |s, tx| s.palette_page(Page::Root, tx))
                .label("Back"),
        );
    }
    header = header
        .child(icon(&glyphs::SEARCH, 16.0).color(theme.text_secondary))
        .child(query);
    let list = col()
        .w_full()
        .key("palette-rows")
        .growth(GrowthSpec::default())
        .virtual_rows(
            p.items.len(),
            32.0,
            |s: &Desktop, i| s.palette.as_ref().unwrap().items[i].key.clone().into(),
            move |s: &Desktop, i| {
                let p = s.palette.as_ref().unwrap();
                let item = &p.items[i];
                let (title, hint) = if matches!(item.action, Action::OpenFile) {
                    let (parent, name) = item.key.rsplit_once('/').unwrap_or(("", &item.key));
                    (Cow::Borrowed(name), parent)
                } else {
                    (
                        item.action.title(&s.workspace),
                        item.action.hint(&s.workspace),
                    )
                };
                let target = item.key.clone();
                let mut option = row()
                    .key("option")
                    .role("option")
                    .label(if matches!(item.action, Action::OpenFile) {
                        item.key.as_str()
                    } else {
                        title.as_ref()
                    })
                    .no_window_drag()
                    .w_full()
                    .h(32.0)
                    .px(8.0)
                    .gap(8.0)
                    .center_y()
                    .radius(5.0)
                    .child(
                        icon(
                            if matches!(item.action, Action::OpenFile) {
                                rune_kit::file_icon(&item.key)
                            } else {
                                item.action.icon()
                            },
                            16.0,
                        )
                        .color(colors.text_secondary),
                    )
                    .child(label(&title, 14.0, 400.0, colors.text).truncate().shrink())
                    .child(spacer());
                if !hint.is_empty() {
                    option = option.child(
                        label(hint, 12.0, 400.0, colors.text_secondary)
                            .shrink()
                            .truncate(),
                    );
                }
                if Some(i) == p.selected {
                    option = option.fill(colors.text.alpha(0.09)).focus_selected(true);
                }
                option = option
                    .on_pointer_move_local(move |s: &mut Desktop, event, tx| {
                        if let Some(p) = s.palette.as_mut()
                            && p.pointer.replace(event.window()) != Some(event.window())
                            && p.selected != Some(i)
                        {
                            p.selected = Some(i);
                        } else {
                            tx.live_only();
                        }
                    })
                    .on_press_tx(move |s: &mut Desktop, tx| {
                        if let Some(action) = s.palette.as_ref().and_then(|p| p.action_for(&target))
                        {
                            s.choose_palette(action, &target, tx);
                        }
                    });
                let mut block = col().w_full();
                if i == 0 || p.items[i - 1].group != item.group {
                    block = block.child(
                        label(item.group, 12.0, 500.0, colors.text_secondary)
                            .px(8.0)
                            .pt(10.0)
                            .pb(4.0),
                    );
                }
                block.child(option)
            },
        )
        .virtual_rows_version((p.revision, s.dark(cx)));
    let result = if p.items.is_empty() {
        label(
            match &p.page {
                Page::Files { status, .. } => status.as_deref().unwrap_or("No matching files."),
                _ => "No results found.",
            },
            14.0,
            400.0,
            theme.text_secondary,
        )
        .padding(16.0)
    } else {
        list
    };
    let footer = row()
        .px(16.0)
        .w_full()
        .h(40.0)
        .gap(5.0)
        .center_y()
        .children(
            [("↑ ↓", "Navigate"), ("Enter", "Select"), ("Esc", "Close")]
                .into_iter()
                .flat_map(|(k, name)| {
                    [
                        label(k, 12.0, 500.0, theme.text)
                            .px(4.0)
                            .py(2.0)
                            .radius(4.0)
                            .fill(theme.text.alpha(0.08)),
                        label(name, 13.0, 500.0, theme.text_secondary).pr(8.0),
                    ]
                }),
        );
    let panel = col()
        .key("command-palette")
        .role("dialog")
        .label("Command palette")
        .no_window_drag()
        .w_full()
        .max_w(576.0)
        .h(height)
        .radius(16.0)
        .border(1.0, theme.separator)
        .fill(theme.bg)
        .child(header)
        .child(
            col()
                .key("palette-scroll")
                .role("listbox")
                .label("Results")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .px(8.0)
                .py(6.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(result),
        )
        .child(el().h(1.0).w_full().fill(theme.separator))
        .child(footer)
        .modal(true, Desktop::dismiss_palette)
        .modal_click_outside(true);
    overlays.push(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .backdrop_blur(3.0)
            .fill(rune::Color::BLACK.alpha(0.4))
            .a11y_hidden(),
    );
    overlays.push(
        col()
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .items_center()
            .px(16.0)
            .pt(top)
            .child(panel),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LiveUpdate, Reply, WorkerMessage};

    #[test]
    fn recent_threads_rank_activity_before_applying_the_twelve_row_limit() {
        let mut app = Desktop::new();
        let threads = (0..14)
            .map(|i| {
                serde_json::from_value(serde_json::json!({
            "id": i.to_string(), "project": "p", "title": format!("Thread {i}"),
            "branch": null, "model": "m", "status": "idle", "settled": false,
            "recent_at": i, "sort_at": i, "order_key": if i == 13 { Some("manual") } else { None }
        })).unwrap()
            })
            .collect();
        app.workspace.replace(vec![], threads);
        app.palette = Some(Palette::new());
        app.filter_palette(false);
        let titles: Vec<_> = app
            .palette
            .as_ref()
            .unwrap()
            .items
            .iter()
            .filter(|item| item.group == "Recent threads")
            .map(|item| item.action.title(&app.workspace))
            .collect();
        assert_eq!(titles.len(), 12);
        assert_eq!(titles.first().unwrap(), "Thread 13");
        assert_eq!(titles.last().unwrap(), "Thread 2");
    }

    #[test]
    fn removing_a_thread_above_the_selection_keeps_the_same_keyboard_and_pointer_target() {
        let mut app = Desktop::new();
        let threads = ["A", "B", "C", "D"].map(|id| super::super::Thread {
            id: id.into(),
            project: "project".into(),
            title: format!("match {id}"),
            search_title: String::new(),
            branch: None,
            worktree: None,
            status: Default::default(),
            title_status: None,
            status_at: None,
            recedes: false,
            sort_at: 0,
            recent_at: 0.,
            order_key: None,
            model: String::new(),
            providers: Vec::new(),
            permission: None,
            settled: false,
            pinned: false,
            snoozed_until: None,
        });
        app.workspace.replace(vec![], threads.into());
        app.open_thread("A");
        app.workspace.conversation.as_mut().unwrap().begin_watch(1);
        let mut p = Palette::new();
        p.query = TextEdit::new("match");
        app.palette = Some(p);
        app.filter_palette(false);
        let pressed = app.palette.as_ref().unwrap().items[2].key.clone();
        let removed = app.palette.as_ref().unwrap().items[0].key.clone();
        app.palette.as_mut().unwrap().selected = Some(2);
        app.receive(WorkerMessage {
            id: None,
            reply: Reply::Live {
                watch: 1,
                thread: "A".into(),
                sequence: 1,
                update: LiveUpdate::Deleted,
            },
        });
        assert!(app.workspace.has_thread("A"));
        app.workspace.feed = Some(2);
        app.receive(WorkerMessage {
            id: None,
            reply: Reply::WorkspaceLive {
                watch: 2,
                changes: super::super::WorkspaceChanges {
                    landing_project: None,
                    sequence: 1,
                    scratch_available: false,
                    projects: vec![],
                    threads: vec![],
                    removed_projects: vec![],
                    removed_threads: vec!["A".into()],
                },
            },
        });
        let p = app.palette.as_ref().unwrap();
        assert_eq!(
            p.items[p.selected.unwrap()].action.title(&app.workspace),
            "match C"
        );
        assert_eq!(
            p.action_for(&pressed).unwrap().title(&app.workspace),
            "match C"
        );
        assert!(p.action_for(&removed).is_none());
    }
}
