#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use rune::{
    App, AppSpec, Cx, El, TextEdit, TextStyle, Titlebar, col, el, row, text, text_input, tok,
};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};

mod appearance;
mod approval;
mod attachments;
mod composer;
mod image_decode;
mod launch_check;
#[cfg(target_os = "linux")]
mod linux_frame;
#[cfg(target_os = "linux")]
mod linux_menu;
#[cfg(target_os = "macos")]
mod mac_update;
mod models;
mod navigation;
mod new_thread;
mod pairing_link;
mod permissions;
mod project_icon;
mod provider_status;
mod questions;
mod sidebar_layout;
mod stage;
mod terminal;
mod updates;
mod window_zoom;
mod workspace;
use workspace::{ConversationWindow, LiveStatus, LiveUpdate, Project, Thread, Workspace};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Reply {
    #[cfg(target_os = "linux")]
    MenuAction {
        action: linux_menu::Action,
    },
    RestartUpdated {
        executable: std::path::PathBuf,
    },
    PairingOffer {
        #[serde(flatten)]
        offer: pairing_link::Offer,
    },
    ReleaseCheck {
        #[serde(flatten)]
        result: updates::Result,
    },
    SystemAppearance {
        dark: bool,
    },
    WindowZoom {
        factor: f64,
    },
    SidebarLayout {
        width: Option<u32>,
        open: Option<bool>,
        error: Option<String>,
    },
    ClientAppearance {
        sans: String,
        code: String,
        smoothing: bool,
        chat_width: appearance::ChatWidth,
        contrast: u8,
        error: Option<String>,
    },
    Appearance {
        mode: appearance::Mode,
        error: Option<String>,
    },
    PlanSaved {
        watch: u64,
        error: Option<String>,
    },
    Downloaded {
        error: Option<String>,
    },
    FilteredFiles {
        operation: u64,
        #[serde(default)]
        entries: Vec<workspace::workspace_tree::Entry>,
        #[serde(default)]
        truncated: bool,
        error: Option<String>,
    },
    FileEntries {
        operation: u64,
        directory: String,
        #[serde(default)]
        entries: Vec<workspace::workspace_tree::Entry>,
        #[serde(default)]
        truncated: bool,
        error: Option<String>,
    },
    FileMatches {
        operation: u64,
        #[serde(default)]
        paths: Vec<String>,
        error: Option<String>,
    },
    FileSource {
        operation: u64,
        contents: Option<String>,
        #[serde(default)]
        truncated: bool,
        error: Option<String>,
    },
    WorkspaceEditors {
        watch: u64,
        catalog: workspace::thread_details::editor::Catalog,
    },
    EditorOpened {
        watch: u64,
        message: String,
    },
    EditorError {
        watch: u64,
        error: String,
    },
    DetailsSaveError {
        watch: u64,
        error: String,
    },
    Connected {
        label: String,
        details: Option<workspace::thread_details::Settings>,
        #[serde(default)]
        stage: stage::Stage,
        local: bool,
        can_operate: bool,
        #[serde(default)]
        can_read_files: bool,
        #[serde(default)]
        can_write_files: bool,
        #[serde(default)]
        can_pin: bool,
        #[serde(default)]
        terminal_read: bool,
        #[serde(default)]
        terminal_write: bool,
    },
    WorkspaceLive {
        watch: u64,
        #[serde(flatten)]
        changes: workspace::WorkspaceChanges,
    },
    WorkspaceClock {
        watch: u64,
        offset: i64,
    },
    RefreshFavicons {
        watch: u64,
        projects: Option<Vec<String>>,
    },
    WorkspacePaused {
        watch: u64,
    },
    Workspace {
        watch: u64,
        editors: Option<workspace::thread_details::editor::Catalog>,
        #[serde(default)]
        sequence: u64,
        #[serde(default)]
        clock_offset: Option<i64>,
        landing_project: Option<String>,
        scratch_available: bool,
        projects: Vec<Project>,
        threads: Vec<Thread>,
    },
    Thread {
        thread: String,
        #[serde(flatten)]
        window: ConversationWindow,
    },
    ThreadUnavailable {
        thread: String,
    },
    Live {
        watch: u64,
        thread: String,
        sequence: u64,
        update: LiveUpdate,
    },
    Error {
        message: String,
    },
    Image {
        #[serde(default)]
        favicon: bool,
        #[serde(default)]
        retryable: bool,
        watch: u64,
        key: String,
        operation: String,
        data: Option<String>,
        #[serde(default)]
        full: bool,
    },
    ToolDetail {
        key: String,
        operation: String,
        #[serde(flatten)]
        detail: workspace::tool::Detail,
    },
    FileDiff {
        operation: String,
        #[serde(flatten)]
        recorded: workspace::file_diff::Recorded,
    },
    TerminalCopy {
        watch: u64,
        operation: String,
        request: u64,
        text: String,
    },
    Terminal {
        watch: u64,
        operation: String,
        #[serde(flatten)]
        frame: terminal::Frame,
    },
    CommandOutput {
        operation: String,
        text: String,
    },
    StopResult {
        operation: String,
        thread: String,
        run: String,
        error: Option<String>,
    },
    RequestResult {
        thread: String,
        version: String,
        error: Option<String>,
    },
    ThreadModels {
        watch: u64,
        operation: String,
        instance: String,
        #[serde(default)]
        options: Vec<models::Choice>,
        error: Option<String>,
    },
    SettlementChanged {
        watch: u64,
        sequence: Option<u64>,
    },
    ThreadEdited {
        watch: u64,
        error: Option<String>,
    },
    ModelSaved {
        thread: String,
        instance: String,
        model: String,
    },
    ThreadPermissions {
        watch: u64,
        operation: String,
        instance: String,
        #[serde(default)]
        options: Vec<permissions::OptionRow>,
        error: Option<String>,
    },
    MessageSent {
        thread: String,
        operation: String,
    },
    ProviderStatus {
        watch: u64,
        banner: Option<provider_status::Banner>,
    },
    ProviderRefreshed {
        watch: u64,
        error: Option<String>,
    },
    DraftModels {
        watch: u64,
        project: String,
        choices: Vec<new_thread::Choice>,
        permissions: new_thread::ProviderPermissions,
    },
    LaunchOptions {
        record: Project,
        project: String,
        choices: Vec<new_thread::Choice>,
        permissions: new_thread::ProviderPermissions,
        base: String,
        worktree: bool,
    },
    ThreadLaunched {
        project: String,
        thread: Option<Thread>,
        error: Option<String>,
    },
    LaunchFailed {
        project: String,
        uncertain: bool,
        message: String,
    },
    ProjectSaved {
        added: bool,
    },
    Unchanged,
    Disconnected,
    LocalClosed,
}

#[derive(Deserialize)]
struct WorkerMessage {
    id: Option<u64>,
    #[serde(flatten)]
    reply: Reply,
}

#[derive(Serialize)]
struct ClientMessage<'a> {
    id: Option<u64>,
    #[serde(flatten)]
    request: Request<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Request<'a> {
    OpenUrl {
        url: &'a str,
    },
    CheckUpdates,
    DownloadUpdate,
    InstallUpdate,
    ShowUpdate,
    #[cfg(target_os = "linux")]
    X11Menu {
        window: u32,
    },
    WorkspaceFiles {
        #[serde(flatten)]
        location: workspace::workspace_files::Location,
        operation: u64,
        #[serde(flatten)]
        input: workspace::workspace_files::Input,
    },
    SetSidebarWidth {
        width: Option<u32>,
    },
    OpenEditor {
        watch: u64,
        editor: &'a str,
        cwd: &'a str,
    },
    ChooseEditor {
        watch: u64,
        editor: &'a str,
    },
    SetDetailsVisible {
        watch: u64,
        draft: bool,
        target: &'a str,
        visible: bool,
    },
    SetSidebarOpen {
        open: bool,
    },
    SetWindowZoom {
        factor: f64,
    },
    SetAppearance {
        mode: appearance::Mode,
    },
    SetContrast {
        value: u8,
    },
    SetChatWidth {
        width: appearance::ChatWidth,
    },
    SetFontFamily {
        slot: appearance::FontSlot,
        family: &'a str,
    },
    SetFontSmoothing {
        smoothing: bool,
    },
    SetSettlement {
        thread: String,
        watch: u64,
        operation: String,
        settled: bool,
        advance: bool,
    },
    SetPin {
        thread: String,
        pinned: bool,
        watch: u64,
        operation: String,
    },
    RenameThread {
        thread: String,
        title: String,
        watch: u64,
        operation: String,
    },
    VisitThread {
        thread: Option<&'a str>,
    },
    WorkspaceHandled {
        watch: u64,
    },
    Terminal {
        watch: u64,
        operation: String,
        #[serde(flatten)]
        input: terminal::Input,
    },
    Local,
    Pair {
        link: &'a str,
    },
    PrepareScratch,
    PrepareLanding {
        project: Option<String>,
    },
    WatchProviderStatus {
        watch: u64,
        project: Option<&'a str>,
        instance: Option<&'a str>,
    },
    RefreshProvider {
        watch: u64,
        key: &'a str,
    },
    WatchDraftModels {
        watch: u64,
        project: Option<&'a str>,
    },
    PrepareThread {
        project: String,
    },
    LaunchThread {
        project: String,
        runtime: String,
        runtime_override: Option<String>,
        model: String,
        text: String,
        files: Vec<String>,
        operation: String,
        base: Option<String>,
    },
    Question {
        thread: String,
        request: String,
        version: String,
        operation: String,
        drafts: Option<std::collections::HashMap<String, questions::Submission>>,
    },
    Approval {
        thread: String,
        request: String,
        version: String,
        decision: String,
        operation: String,
    },
    ImageHandled {
        watch: u64,
        operation: String,
    },
    ProjectFavicon {
        watch: u64,
        project: String,
        cwd: String,
        path: Option<String>,
        operation: String,
    },
    InspectImage {
        full: bool,
        watch: u64,
        key: String,
        operation: String,
        open: bool,
    },
    InspectTool {
        watch: u64,
        key: String,
        operation: String,
        open: bool,
    },
    DownloadPlan {
        path: std::path::PathBuf,
        contents: String,
    },
    SavePlan {
        watch: u64,
        key: String,
        root: String,
        path: String,
    },
    InspectFile {
        watch: u64,
        key: Option<String>,
        operation: String,
    },
    InspectCommand {
        watch: u64,
        key: Option<String>,
        operation: String,
    },
    StopRun {
        thread: String,
        run: String,
        operation: String,
    },
    ThreadModels {
        watch: u64,
        operation: String,
    },
    SetThreadModel {
        watch: u64,
        thread: String,
        instance: String,
        model: String,
        operation: String,
    },
    ThreadPermissions {
        instance: String,
        watch: u64,
        operation: String,
    },
    SendMessage {
        model: Option<models::Handoff>,
        permission: Option<permissions::Selection>,
        thread: String,
        text: String,
        files: Vec<String>,
        operation: String,
    },
    AddProject {
        path: String,
    },
    ArchiveProject {
        project: String,
        archived: bool,
    },
    Refresh,
    Thread {
        thread: String,
        project_root: Option<String>,
        cursor: Option<String>,
        sequence: Option<u64>,
        oldest: Option<String>,
        earlier_pages: usize,
    },
    Disconnect,
}

struct Desktop {
    #[cfg(target_os = "linux")]
    frame: linux_frame::Frame,
    zoom: window_zoom::WindowZoom,
    sidebar_layout: sidebar_layout::Layout,
    appearance: appearance::Mode,
    system_dark: Option<bool>,
    font_smoothing: Option<bool>,
    saving_client_appearance: bool,
    chat_width: appearance::ChatWidth,
    chat_width_menu: bool,
    contrast: appearance::Contrast,
    font_families: appearance::FontFamilies,
    typography_advanced: bool,
    settings_open: bool,
    update: updates::Panel,
    appearance_error: Option<String>,
    output: Option<std::process::ChildStdin>,
    answer_advance: Option<questions::Advance>,
    link: TextEdit,
    pairing_offers: std::collections::VecDeque<pairing_link::Offer>,
    pairing: bool,
    environment: Option<String>,
    stage: stage::Stage,
    local: bool,
    can_operate: bool,
    can_write_files: bool,
    can_read_files: bool,
    can_pin: bool,
    terminal_read: bool,
    terminal_write: bool,
    terminal: Option<terminal::Panel>,
    project_path: Option<TextEdit>,
    workspace: Workspace,
    history: navigation::History,
    landing: navigation::Landing,
    new_threads: new_thread::NewThreads,
    permission_menu: Option<permissions::Menu>,
    models: models::Picker,
    provider_status: provider_status::Status,
    palette: Option<workspace::palette::Palette>,
    last_id: u64,
    request_id: u64,
    pending: bool,
    starting_local: bool,
    error: Option<String>,
}

impl Desktop {
    fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            frame: Default::default(),
            output: None,
            zoom: Default::default(),
            sidebar_layout: Default::default(),
            answer_advance: None,
            appearance: Default::default(),
            system_dark: cfg!(target_os = "linux").then_some(false),
            font_smoothing: None,
            saving_client_appearance: false,
            chat_width: Default::default(),
            chat_width_menu: false,
            contrast: Default::default(),
            font_families: Default::default(),
            typography_advanced: false,
            settings_open: false,
            update: Default::default(),
            appearance_error: None,
            link: TextEdit::new("").secure(),
            pairing_offers: Default::default(),
            pairing: false,
            environment: None,
            stage: Default::default(),
            local: false,
            can_operate: false,
            can_write_files: false,
            can_read_files: false,
            can_pin: false,
            terminal_read: false,
            terminal_write: false,
            terminal: None,
            project_path: None,
            workspace: Workspace::new(),
            history: Default::default(),
            landing: Default::default(),
            new_threads: Default::default(),
            permission_menu: None,
            models: Default::default(),
            provider_status: Default::default(),
            palette: None,
            last_id: 0,
            request_id: 0,
            pending: false,
            starting_local: false,
            error: None,
        }
    }

    fn clear_environment(&mut self) {
        self.sidebar_layout.end_drag();
        *self = Self {
            #[cfg(target_os = "linux")]
            frame: self.frame,
            provider_status: self.provider_status.for_new_connection(),
            output: self.output.take(),
            last_id: self.last_id,
            request_id: self.request_id,
            sidebar_layout: self.sidebar_layout,
            appearance: self.appearance,
            system_dark: self.system_dark,
            font_smoothing: self.font_smoothing,
            saving_client_appearance: self.saving_client_appearance,
            chat_width: self.chat_width,
            contrast: self.contrast,
            font_families: std::mem::take(&mut self.font_families),
            pairing_offers: std::mem::take(&mut self.pairing_offers),
            update: std::mem::take(&mut self.update),
            zoom: self.zoom,
            ..Self::new()
        };
    }

    fn worker_closed(&mut self) {
        *self = Self {
            #[cfg(target_os = "linux")]
            frame: self.frame,
            system_dark: self.system_dark,
            appearance: self.appearance,
            ..Self::new()
        };
    }

    fn write_request(&mut self, id: Option<u64>, request: Request<'_>) -> bool {
        let Some(mut output) = self.output.as_mut() else {
            return false;
        };
        let message = ClientMessage { id, request };
        let sent = serde_json::to_writer(&mut output, &message).is_ok()
            && output
                .write_all(b"\n")
                .and_then(|()| output.flush())
                .is_ok();
        if !sent {
            self.worker_closed();
        }
        sent
    }

    fn next_request_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    fn send(&mut self, request: Request<'_>) {
        if self.pending || self.output.is_none() {
            return;
        }
        let pairing = matches!(request, Request::Pair { .. });
        if matches!(request, Request::Refresh) {
            if self.workspace.refresh_request.is_some() {
                return;
            }
            self.retry_landing();
            if self.workspace.feed.is_some() {
                self.error = None;
                self.commit_title();
                let id = self.next_request_id();
                if self.write_request(Some(id), request) {
                    self.workspace.refresh_request = Some(id);
                }
                return;
            }
        } else if !pairing
            && (self.landing.project().is_some()
                || matches!(
                    request,
                    Request::Thread { .. }
                        | Request::PrepareThread { .. }
                        | Request::PrepareScratch
                        | Request::Disconnect
                ))
        {
            self.leave_landing();
        }
        if !matches!(request, Request::Refresh | Request::SetSettlement { .. }) {
            self.cancel_settle_navigation();
        }
        self.permission_menu = None;
        self.models.close();
        if matches!(request, Request::Thread { .. }) {
            self.close_conversation_inspectors();
        }
        self.answer_advance = None;
        self.request_id = self.next_request_id();
        let starting_local = matches!(request, Request::Local);
        if self.write_request(Some(self.request_id), request) {
            self.starting_local = starting_local;
            self.pairing = pairing;
            self.pending = true;
            self.error = None;
        }
    }

    fn pair(&mut self) {
        if self.link.text.trim().is_empty() || self.pending {
            return;
        }
        let link = std::mem::replace(&mut self.link, TextEdit::new("").secure());
        self.send(Request::Pair {
            link: link.text.trim(),
        });
    }

    fn refresh_after_edit(&mut self) {
        if self.pending || self.workspace.refresh_request.is_some() {
            return;
        }
        if let Some(watch) = self.workspace.refresh_after_edit.take()
            && self.workspace.paused
            && self.workspace.feed == Some(watch)
        {
            self.send(Request::Refresh);
        }
    }

    fn receive(&mut self, message: WorkerMessage) {
        if let Reply::WorkspaceEditors { watch, catalog } = message.reply {
            self.update_editors(watch, catalog);
            return;
        }
        if let Reply::DetailsSaveError { watch, error } | Reply::EditorError { watch, error } =
            message.reply
        {
            if self.workspace.feed == Some(watch) {
                self.error = Some(error);
            }
            return;
        }
        if let Reply::ProviderStatus { watch, banner } = message.reply {
            self.receive_provider_status(message.id, watch, banner);
            return;
        }
        if let Reply::ProviderRefreshed { watch, error } = message.reply {
            self.refreshed_provider(message.id, watch, error);
            return;
        }
        if let Reply::DraftModels {
            watch,
            project,
            choices,
            permissions,
        } = message.reply
        {
            self.receive_draft_models(watch, &project, message.id, choices, permissions);
            return;
        }
        if let Reply::FilteredFiles {
            operation,
            entries,
            truncated,
            error,
        } = message.reply
        {
            self.tree_filtered(operation, entries, truncated, error);
            return;
        }
        if let Reply::FileEntries {
            operation,
            directory,
            entries,
            truncated,
            error,
        } = message.reply
        {
            self.directory_loaded(operation, directory, entries, truncated, error);
            return;
        }
        if let Reply::FileMatches {
            operation,
            paths,
            error,
        } = message.reply
        {
            self.receive_file_matches(operation, paths, error);
            return;
        }
        if let Reply::FileSource {
            operation,
            contents,
            truncated,
            error,
        } = message.reply
        {
            self.receive_file_source(operation, contents, truncated, error);
            return;
        }
        if let Reply::PlanSaved { watch, error } = message.reply {
            self.finish_plan_save(watch, error);
            return;
        }
        if let Reply::Downloaded { error } = message.reply {
            if let Some(error) = error {
                self.error = Some(error);
            }
            return;
        }
        if let Reply::SidebarLayout { width, open, error } = message.reply {
            if let Some(error) = error {
                self.error = Some(error);
            } else {
                self.sidebar_layout.width = width;
                self.sidebar_layout.open = open.unwrap_or(true);
            }
            return;
        }
        if let Reply::PairingOffer { offer } = message.reply {
            if !self.pairing_offers.contains(&offer) {
                self.pairing_offers.push_back(offer);
            }
            return;
        }
        if let Reply::ReleaseCheck { result } = message.reply {
            self.update.receive(result);
            return;
        }
        if let Reply::WindowZoom { factor } = message.reply {
            self.zoom.restore(factor);
            return;
        }
        if let Reply::SystemAppearance { dark } = message.reply {
            self.system_dark = Some(dark);
            return;
        }
        if let Reply::Appearance { mode, error } = message.reply {
            self.appearance = mode;
            self.appearance_error = error;
            return;
        }
        if let Reply::ThreadEdited { watch, error } = message.reply {
            if self.workspace.feed == Some(watch) {
                if let Some(message) = error {
                    self.error = Some(message);
                } else if self.workspace.paused {
                    self.workspace.refresh_after_edit = Some(watch);
                    self.refresh_after_edit();
                }
            }
            return;
        }
        if let Reply::WorkspaceLive { watch, changes } = message.reply {
            if self.workspace.feed == Some(watch) {
                self.apply_workspace(changes);
                self.write_request(None, Request::WorkspaceHandled { watch });
                self.advance_after_settle();
            }
            return;
        }
        if let Reply::WorkspaceClock { watch, offset } = message.reply {
            if self.workspace.feed == Some(watch) {
                self.workspace.set_clock_offset(offset);
            }
            return;
        }
        if let Reply::RefreshFavicons { watch, projects } = message.reply {
            if self.workspace.feed == Some(watch) {
                self.workspace.refresh_favicons(projects);
            }
            return;
        }
        if let Reply::WorkspacePaused { watch } = message.reply {
            if self.workspace.feed == Some(watch) {
                self.workspace.paused = true;
                if self.waiting_for_settle() {
                    self.workspace.refresh_after_edit = Some(watch);
                    self.refresh_after_edit();
                }
            }
            return;
        }
        if let Reply::ToolDetail {
            key,
            operation,
            detail,
        } = message.reply
        {
            self.update_tool(&key, &operation, detail);
            return;
        }
        if let Reply::FileDiff {
            operation,
            recorded,
        } = message.reply
        {
            self.workspace.diff.receive(&operation, recorded);
            return;
        }
        if let Reply::Terminal {
            watch,
            operation,
            frame,
        } = message.reply
        {
            self.terminal_frame(watch, &operation, frame);
            return;
        }
        if let Reply::CommandOutput { operation, text } = message.reply {
            if let Some(pane) = &mut self.workspace.conversation {
                pane.update_output(&operation, text);
            }
            return;
        }
        if let Reply::StopResult {
            thread,
            run,
            operation,
            error,
        } = message.reply
        {
            if let Some(pane) = self.workspace.conversation.as_mut()
                && pane.id == thread
                && pane.active_run.as_deref() == Some(run.as_str())
                && pane.stop_operation.as_deref() == Some(operation.as_str())
            {
                pane.stop_operation = None;
                if let Some(message) = error {
                    pane.stop_feedback = Some(composer::StopFeedback::Retry);
                    self.error = Some(message);
                } else if pane.status == LiveStatus::Paused && !self.pending {
                    self.refresh_thread();
                }
            }
            return;
        }
        if let Reply::Live {
            watch,
            thread,
            sequence,
            update,
        } = message.reply
        {
            let was_open = self.workspace.conversation.is_some();
            self.workspace.apply_live(watch, &thread, sequence, update);
            self.close_stale_terminal();
            self.reconcile_file_location();
            if was_open && self.workspace.conversation.is_none() {
                self.history.close_thread(&thread);
                self.history.retain(&self.workspace, &self.new_threads);
                if self.palette.is_some() {
                    self.filter_palette(true);
                }
            }
            return;
        }
        let refresh = message.id.is_some() && message.id == self.workspace.refresh_request;
        let landing = self.landing.matches(message.id);
        if landing && matches!(message.reply, Reply::Error { .. }) {
            self.landing.fail();
            return;
        }
        if matches!(message.reply, Reply::LocalClosed) {
            if self.environment.is_none() {
                return;
            }
        } else if !landing && !refresh && (!self.pending || message.id != Some(self.request_id)) {
            return;
        }
        if refresh {
            self.workspace.refresh_request = None;
        } else if !landing {
            self.pending = false;
            self.starting_local = false;
            self.error = None;
        }
        match message.reply {
            Reply::RequestResult {
                thread,
                version,
                error,
            } => {
                if self.workspace.conversation.as_ref().is_some_and(|p| {
                    p.id == thread
                        && (p.approvals.iter().any(|a| a.version == version)
                            || self
                                .workspace
                                .questions
                                .get(&thread)
                                .is_some_and(|f| f.prompt.version == version))
                }) {
                    if let Some(message) = error {
                        self.error = Some(message);
                    } else {
                        self.refresh_thread();
                    }
                }
            }
            Reply::SettlementChanged { watch, sequence } => {
                if self.workspace.feed == Some(watch) {
                    if self.workspace.paused {
                        self.workspace.refresh_after_edit = Some(watch);
                    }
                    self.settlement_committed(sequence);
                }
            }
            Reply::ModelSaved {
                thread,
                instance,
                model,
            } => {
                if let Some(draft) = self.workspace.drafts.get_mut(&thread) {
                    draft.set_model(None, &instance, &model);
                }

                if self.workspace.paused {
                    self.send(Request::Refresh);
                }
            }
            Reply::MessageSent { thread, operation } => {
                if self.workspace.finish_message(&thread, &operation)
                    && self
                        .workspace
                        .conversation
                        .as_ref()
                        .is_some_and(|p| p.status == LiveStatus::Paused)
                {
                    self.refresh_thread();
                }
            }
            Reply::ProjectSaved { added } => {
                if added {
                    self.project_path = None;
                }
                self.pending = true;
            }
            Reply::Connected {
                label,
                details,
                stage,
                local,
                can_operate,
                can_write_files,
                can_read_files,
                can_pin,
                terminal_read,
                terminal_write,
            } => {
                if self.pairing && self.environment.is_some() {
                    self.clear_environment();
                }
                self.pairing = false;
                self.landing = Default::default();
                self.workspace.set_clock_offset(0);
                self.stage = stage;
                self.local = local;
                self.can_operate = can_operate;
                self.can_write_files = can_write_files;
                self.can_read_files = can_read_files;
                self.can_pin = can_operate && can_pin;
                self.terminal_read = terminal_read;
                self.terminal_write = terminal_write;
                self.workspace.details.settings = details;
                self.environment = Some(label);
                self.pending = true;
            }
            Reply::Workspace {
                watch,
                editors,
                sequence,
                projects,
                threads,
                scratch_available,
                clock_offset,
                landing_project,
            } => {
                if let Some(offset) = clock_offset {
                    self.workspace.set_clock_offset(offset);
                }
                self.workspace.sequence = sequence;
                self.workspace.feed = Some(watch);
                self.workspace.details.editor = editors;
                self.workspace.paused = false;
                self.workspace.scratch_available = scratch_available;
                self.replace_workspace(projects, threads);
                if let Some(project) = landing_project {
                    self.begin_landing(project);
                }
            }
            Reply::LaunchOptions {
                record,
                project,
                choices,
                permissions,
                base,
                worktree,
            } => {
                self.leave_visible_thread();
                self.close_conversation_inspectors();
                let page = navigation::Page::NewThread(project.clone());
                if landing {
                    self.landing = navigation::Landing::Left;
                    self.history.replace(page);
                } else {
                    self.history.visit(page);
                    self.workspace.upsert_project(record);
                }
                self.new_threads
                    .configure(project, choices, permissions, worktree, &base);
                self.workspace.show_projects = false;
            }
            Reply::ThreadLaunched {
                project,
                thread,
                error,
            } => {
                self.new_threads.forget(&project);
                if let Some(thread) = thread {
                    let id = thread.id.clone();
                    self.history.launched(&project, &id);
                    self.workspace.insert_launched(thread);
                    self.open_thread(&id);
                } else {
                    self.show_projects();
                    self.error = error.or_else(|| {
                        Some("The thread started and is no longer in this workspace.".into())
                    });
                }
                self.history.retain(&self.workspace, &self.new_threads);
            }
            Reply::LaunchFailed {
                project,
                uncertain,
                message,
            } => {
                self.new_threads.mark_unconfirmed(&project, uncertain);
                self.error = Some(message);
            }
            Reply::Thread { thread, window } => {
                if let Some(pane) = &mut self.workspace.conversation
                    && pane.id == thread
                {
                    pane.replace(window, &mut self.workspace.questions);
                    pane.begin_watch(self.request_id);
                }
            }
            Reply::ThreadUnavailable { thread } => {
                self.workspace.close_unavailable_thread(&thread);
                self.history.close_thread(&thread);
                self.close_stale_terminal();
                self.reconcile_file_location();
                self.history.retain(&self.workspace, &self.new_threads);
                if self.palette.is_some() {
                    self.filter_palette(true);
                }
            }
            Reply::Unchanged => {
                if let Some(pane) = &mut self.workspace.conversation {
                    pane.begin_watch(self.request_id);
                }
            }
            #[cfg(target_os = "linux")]
            Reply::MenuAction { .. } => unreachable!(),
            Reply::Live { .. }
            | Reply::PlanSaved { .. }
            | Reply::Downloaded { .. }
            | Reply::FileMatches { .. }
            | Reply::FileEntries { .. }
            | Reply::FilteredFiles { .. }
            | Reply::FileSource { .. }
            | Reply::StopResult { .. }
            | Reply::WorkspaceClock { .. }
            | Reply::ProviderStatus { .. }
            | Reply::ProviderRefreshed { .. }
            | Reply::DraftModels { .. }
            | Reply::WorkspaceLive { .. }
            | Reply::ThreadEdited { .. }
            | Reply::WorkspaceEditors { .. }
            | Reply::EditorError { .. }
            | Reply::EditorOpened { .. }
            | Reply::DetailsSaveError { .. }
            | Reply::SidebarLayout { .. }
            | Reply::ClientAppearance { .. }
            | Reply::RestartUpdated { .. }
            | Reply::PairingOffer { .. }
            | Reply::ReleaseCheck { .. }
            | Reply::WindowZoom { .. }
            | Reply::SystemAppearance { .. }
            | Reply::Appearance { .. }
            | Reply::RefreshFavicons { .. }
            | Reply::WorkspacePaused { .. }
            | Reply::ThreadModels { .. }
            | Reply::ThreadPermissions { .. }
            | Reply::TerminalCopy { .. }
            | Reply::Terminal { .. }
            | Reply::CommandOutput { .. }
            | Reply::Image { .. }
            | Reply::ToolDetail { .. }
            | Reply::FileDiff { .. } => {
                unreachable!()
            }
            Reply::Error { message } => {
                if !refresh && !landing {
                    self.pairing = false;
                }
                if !refresh || self.waiting_for_settle() {
                    self.cancel_settle_navigation();
                }
                self.error = Some(message);
            }
            Reply::Disconnected | Reply::LocalClosed => {
                let lost = matches!(message.reply, Reply::LocalClosed);
                let pairing = lost && self.pairing;
                self.clear_environment();
                self.pending = pairing;
                self.pairing = pairing;
                if lost {
                    self.error = Some(
                        "The local environment stopped. Reconnect to start it again.".to_owned(),
                    );
                }
            }
        }
        self.close_stale_terminal();
        self.reconcile_file_location();
        self.advance_after_settle();
        self.refresh_after_edit();
    }
}

fn button(label: &str, enabled: bool, theme: &tok::Theme, action: fn(&mut Desktop)) -> El {
    let button = el()
        .key(label)
        .role("button")
        .label(label)
        .h(30.0)
        .px(12.0)
        .center()
        .radius(6.0)
        .fill(theme.text)
        .child(
            text(label)
                .font(TextStyle::new(13.0, 500.0))
                .color(theme.bg),
        );
    if enabled {
        button.focusable(true).on_press(action)
    } else {
        button.disabled(true).opacity(0.5)
    }
}

const CAPTION_HEIGHT: f32 = 40.;
const WORKSPACE_TOPBAR_HEIGHT: f32 = 52.;

fn native_titlebar(zoom: f32) -> Titlebar {
    let center = WORKSPACE_TOPBAR_HEIGHT * zoom / 2.;
    let light_top = (center - 7.).round().max(0.);
    Titlebar::new(CAPTION_HEIGHT).traffic_lights(16., light_top)
}

fn view(s: &Desktop, cx: &mut Cx<Desktop>) -> El {
    cx.zoom(s.zoom.factor as f32);
    #[cfg(target_os = "macos")]
    cx.set_titlebar(native_titlebar(s.zoom.factor as f32));
    let dark = s.dark(cx);
    let mut theme = tok::theme(dark);
    theme.bg = if dark {
        rune::Color::BLACK
    } else {
        rune::oklch(0.992, 0.0, 0.0)
    };
    theme.text = rune::color::hex(if dark { "#f5f5f5" } else { "#18181b" }).unwrap();
    theme.text_secondary = rune::color::hex(if dark { "#a1a1a1" } else { "#71717b" }).unwrap();
    theme.separator = theme.text.alpha(0.12);
    s.contrast.apply(&mut theme, dark);
    let content = if s.output.is_some() && s.environment.is_some() {
        workspace::view(s, cx, &theme)
    } else {
        connection(s, cx, theme)
    };
    #[cfg(target_os = "linux")]
    let content = linux_frame::view(s, content, dark);
    content
}

fn connection(s: &Desktop, cx: &mut Cx<Desktop>, theme: tok::Theme) -> El {
    let title = |label: &str| {
        text(label)
            .font(TextStyle::new(20.0, 600.0))
            .color(theme.text)
    };
    let mut panel = col().w_full().max_w(576.0).padding(24.0).gap(12.0);
    if s.output.is_none() {
        panel = panel
            .child(title("The desktop connection closed"))
            .child(text("Relaunch T3 Code to reconnect.").color(theme.text_secondary));
    } else if s.starting_local {
        panel = panel.child(title("T3 Code"));
    } else {
        let field = text_input(&s.link, TextStyle::new(14.0, 400.0))
            .key("pairing-link")
            .label("Pairing link")
            .single_line()
            .secure()
            .w_full()
            .h(36.0)
            .px(10.0)
            .radius(6.0)
            .border(1.0, theme.separator)
            .color(theme.text)
            .placeholder("Paste a pairing link", theme.text_secondary)
            .disabled(s.pending)
            .on_edit(|s: &mut Desktop| &mut s.link)
            .on_submit(|s: &mut Desktop, _| s.pair());
        panel = panel
            .radius(16.0)
            .border(1.0, theme.separator)
            .child(
                text("T3 Code")
                    .font(TextStyle::new(18.0, 500.0))
                    .color(theme.text),
            )
            .child(
                row().child(button("Use this computer", !s.pending, &theme, |s| {
                    s.send(Request::Local)
                })),
            )
            .child(title("Pair with this environment"))
            .child(text("Pairing link").color(theme.text))
            .child(field)
            .child(row().child(button(
                "Continue",
                !s.pending && !s.link.text.trim().is_empty(),
                &theme,
                Desktop::pair,
            )));
    }
    if s.pending {
        panel = panel.child(
            text(if s.starting_local {
                "Starting local environment…"
            } else if s.environment.is_some() {
                "Loading projects…"
            } else {
                "Pairing…"
            })
            .color(theme.text_secondary),
        );
    }
    if let Some(error) = &s.error {
        panel = panel.child(text(error).wrap().color(theme.text));
    }
    let mut root = col()
        .key("connection")
        .w_full()
        .h_full()
        .center()
        .fill(theme.bg)
        .child(panel);
    #[cfg(target_os = "linux")]
    if s.frame.visible() {
        root = root.child(
            el().absolute(0., 0.)
                .w_full()
                .h(CAPTION_HEIGHT / s.zoom.factor as f32)
                .window_drag(),
        );
    }
    if s.output.is_some() {
        root = root.child(appearance::trigger(&theme).absolute(12., 50.));
    }
    let mut overlays = Vec::new();
    appearance::overlay(s, cx, &theme, &mut overlays);
    updates::overlay(s, &theme, s.dark(cx), &mut overlays);
    pairing_link::overlay(s, &theme, s.dark(cx), &mut overlays);
    rune_kit::root(root, overlays)
}

fn disclosure_chevron(expanded: bool, size: f32) -> El {
    // Rotating an element requires a GPU layer, even for a static icon on Windows.
    const RIGHT: rune::Icon = rune::Icon {
        name: "chevron-right",
        view: 16.,
        body: r#"<path d="M6.5 5L9.5 8L6.5 11" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>"#,
    };
    rune::icon(
        if expanded {
            &rune_kit::glyphs::CHEVRON_DOWN
        } else {
            &RIGHT
        },
        size,
    )
}

fn product_name() -> &'static str {
    option_env!("T3CODE_RUNE_PRODUCT_NAME").unwrap_or("T3 Code")
}

#[cfg(target_os = "linux")]
fn claim_installed_instance() -> io::Result<bool> {
    let mut id = format!("com.t3tools.T3Code.{}", product_name());
    let home = std::env::var("T3CODE_HOME").unwrap_or_default();
    if !home.trim().is_empty() {
        // Match the worker's path.resolve, including .. through a nonexistent directory.
        let mut path = std::path::PathBuf::new();
        for component in std::path::absolute(home.trim())?.components() {
            if component == std::path::Component::ParentDir {
                path.pop();
            } else {
                path.push(component);
            }
        }
        let hash = blake3::hash(path.as_os_str().as_encoded_bytes()).to_hex();
        id.push('-');
        id.push_str(&hash[..16]);
    }
    Ok(rune::single_instance(&id))
}

#[cfg(windows)]
fn claim_installed_instance() -> io::Result<bool> {
    // Browser activations inherit Explorer's environment, even when T3 started with a custom home.
    Ok(rune::single_instance("com.t3tools.T3Code"))
}

fn packaged_runtime() -> io::Result<Option<(std::path::PathBuf, std::path::PathBuf)>> {
    #[cfg(target_os = "macos")]
    let package = {
        let exe = std::env::current_exe()?;
        exe.parent()
            .filter(|dir| dir.ends_with("Contents/MacOS"))
            .and_then(|dir| dir.parent())
            .map(|dir| (dir.join("Helpers/node"), dir.join("Resources")))
    };
    #[cfg(target_os = "linux")]
    let package = {
        let exe = std::env::current_exe()?;
        let installed = exe.file_name().is_some_and(|name| name == "t3code")
            && exe
                .parent()
                .and_then(std::path::Path::parent)
                .is_some_and(|parent| parent.ends_with("opt"));
        let resources = if installed {
            exe.parent().map(|dir| dir.join("resources"))
        } else {
            exe.parent()
                .filter(|dir| dir.ends_with("usr/bin"))
                .and_then(|dir| dir.parent())
                .map(|dir| dir.join("lib/t3-code"))
        };
        rune::set_app_id(if installed {
            "com.t3tools.T3Code"
        } else if resources.is_some() {
            "com.t3tools.T3Code.Rune"
        } else {
            "com.t3tools.T3Code.Rune.Development"
        });
        resources.map(|dir| (dir.join("node"), dir))
    };
    #[cfg(target_os = "windows")]
    let package = {
        let exe = std::env::current_exe()?;
        exe.parent()
            .map(|dir| dir.join("resources"))
            .filter(|dir| dir.is_dir() || exe.ends_with(format!("{}.exe", product_name())))
            .map(|dir| (dir.join("node.exe"), dir))
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let package = None;
    Ok(package)
}

fn worker_command(
    mode: Option<&str>,
    package: Option<(std::path::PathBuf, std::path::PathBuf)>,
) -> io::Result<std::process::Command> {
    use std::process::Command;
    if let Some((node, resources)) = package {
        let mut command = Command::new(node);
        command
            .arg(resources.join("rune-projects.mjs"))
            .arg("--packaged");
        return Ok(command);
    }

    let node = std::env::var_os("T3CODE_RUNE_NODE").ok_or_else(|| {
        io::Error::other("Open the Rune desktop app or run vp run dev:desktop:rune.")
    })?;
    let worker = std::env::var_os("T3CODE_RUNE_WORKER")
        .ok_or_else(|| io::Error::other("Missing desktop worker path."))?;
    let mut command = Command::new(node);
    command.arg(worker);
    command.args(mode);
    Ok(command)
}

#[cfg(windows)]
fn run_windows_cli(mut cli: std::process::Command) -> io::Result<()> {
    use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use winsafe::{AttachConsole, PidParent, co};

    // Attaching a GUI executable to cmd.exe can replace redirected standard handles.
    fn inherited(stream: impl AsHandle + AsRawHandle) -> Option<OwnedHandle> {
        if stream.as_raw_handle().is_null() {
            return None;
        }
        stream.as_handle().try_clone_to_owned().ok()
    }
    let stdin = inherited(io::stdin());
    let stdout = inherited(io::stdout());
    let stderr = inherited(io::stderr());
    match AttachConsole(PidParent::Parent) {
        Ok(()) | Err(co::ERROR::ACCESS_DENIED) => {
            // The child handles Ctrl+C and Ctrl+Break while this launcher waits for its exit code.
            ctrlc::set_handler(|| {}).map_err(io::Error::other)?;
        }
        Err(_) => {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cli.creation_flags(CREATE_NO_WINDOW);
        }
    }
    if let Some(input) = stdin {
        cli.stdin(input);
    }
    if let Some(output) = stdout {
        cli.stdout(output);
    }
    if let Some(error) = stderr {
        cli.stderr(error);
    }
    std::process::exit(cli.status()?.code().unwrap_or(1));
}

fn start() -> io::Result<()> {
    use std::process::Stdio;

    let mode = std::env::args_os().nth(1);
    let mode = mode
        .as_deref()
        .and_then(|arg| arg.to_str())
        .filter(|arg| matches!(*arg, "--integrated-dev" | "--source-build"));
    let package = packaged_runtime()?;
    if let Some((node, resources)) = &package
        && std::env::var_os("ELECTRON_RUN_AS_NODE").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        // A t3 launcher installed before the upgrade can run before Rune refreshes it.
        let mut args = std::env::args_os().skip(1);
        if args.next().is_some_and(|entry| {
            std::path::Path::new(&entry).ends_with("app.asar/apps/server/dist/bin.mjs")
        }) {
            let mut cli = std::process::Command::new(node);
            cli.arg(resources.join("server/dist/bin.mjs"))
                .args(args)
                .env_remove("ELECTRON_RUN_AS_NODE");
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                return Err(cli.exec());
            }
            #[cfg(windows)]
            {
                return run_windows_cli(cli);
            }
        }
    }
    #[cfg(target_os = "linux")]
    if package.is_some() && std::env::var_os("APPIMAGE").is_some() {
        // Keep the runtime's FUSE pipe and mount descriptor in the original process.
        close_fds::set_fds_cloexec(3, &[]);
    }
    let launch_check = launch_check::LaunchCheck::requested()?;
    #[cfg(any(target_os = "linux", windows))]
    if launch_check.is_none()
        && package
            .as_ref()
            .is_some_and(|(_, resources)| resources.join("rune-release.json").is_file())
        && !claim_installed_instance()?
    {
        return Ok(());
    }
    let mut worker = worker_command(mode, package)?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        worker.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = worker
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child.stdin.take();
    let input = BufReader::new(child.stdout.take().unwrap());
    let spec = AppSpec {
        min_size: Some((400.0, 360.0)),
        poll: Some(|s: &mut Desktop, now, tx| {
            s.sync_draft_model_watch();
            let banner = s.sync_provider_status();
            let details = s.reconcile_details();
            let stop = s.poll_stop(now, tx);
            let clock = s.workspace.poll_sidebar_clock(tx);
            let renamed = s.finish_title_on_blur();
            let resized = s.sidebar_layout.cancel_unfocused_drag(tx);
            let contrast = s.contrast.cancel_unfocused_drag(tx);
            s.poll_answer(tx)
                || stop
                || clock
                || renamed
                || resized
                || contrast
                || banner
                || details
        }),
        on_key: Some(|s: &mut Desktop, ev, tx| {
            #[cfg(target_os = "linux")]
            if linux_menu::shortcut(s, ev, tx) {
                return true;
            }
            if !s.pairing_offers.is_empty() || s.update.open {
                return false;
            }
            if s.zoom_key(ev, tx) {
                return true;
            }
            if !ev.mods.alt && !ev.mods.shift && ev.is_command(',') {
                if !tx.key_repeat() {
                    s.open_settings(tx);
                }
                return true;
            }
            if s.settings_open {
                return false;
            }
            if !ev.mods.alt && !ev.mods.shift && ev.is_command('k') {
                if !tx.key_repeat() {
                    s.toggle_palette(tx);
                }
                return true;
            }
            if ev.key == rune::Key::Escape && s.cancel_title_edit(tx) {
                return true;
            }
            if ev.key == rune::Key::Escape && s.close_image() {
                return true;
            }
            if !ev.mods.alt
                && !ev.mods.shift
                && ev.is_command('p')
                && tx.focused() != Some("thread-terminal-grid")
                && s.file_location().is_some()
                && !s.pending
            {
                if !tx.key_repeat() {
                    s.toggle_file_picker(tx);
                }
                return true;
            }
            if s.palette.is_some() {
                return false;
            }
            if s.environment.is_some()
                && !ev.mods.alt
                && !ev.mods.shift
                && (ev.is_command('[') || ev.is_command(']'))
            {
                if !tx.key_repeat() {
                    s.go(ev.is_command(']'));
                }
                return true;
            }
            if ev.key == rune::Key::Escape && s.dismiss_details(tx) {
                return true;
            }
            if ev.mods.shift
                && !ev.mods.alt
                && ev.is_command('b')
                && tx.focused() != Some("thread-terminal-grid")
                && s.can_show_details()
            {
                if !tx.key_repeat() {
                    s.toggle_details(tx);
                }
                return true;
            }
            if !ev.mods.shift
                && !ev.mods.alt
                && ev.is_command('o')
                && s.can_open_editor()
                && tx.focused() != Some("thread-terminal-grid")
            {
                if !tx.key_repeat() {
                    s.open_editor();
                }
                return true;
            }
            if s.thread_action_shortcut(ev, tx)
                || s.model_shortcut(ev, tx)
                || s.permission_shortcut(ev, tx)
            {
                return true;
            }
            if s.terminal_available() && !ev.mods.alt && !ev.mods.shift && ev.is_command('j') {
                if !tx.key_repeat() {
                    s.toggle_terminal(tx);
                }
                return true;
            }
            if ev.mods.alt
                && !ev.mods.shift
                && ev.is_command('b')
                && s.file_location().is_some()
                && !s.pending
            {
                if !tx.key_repeat() {
                    s.toggle_files_panel(tx);
                }
                return true;
            }
            if ev.key == rune::Key::Escape
                && s.workspace.explorer.is_some()
                && s.workspace.file_preview.is_none()
            {
                s.close_explorer();
                return true;
            }
            if ev.key == rune::Key::Escape && s.workspace.file_preview.is_some() {
                s.dismiss_file_preview(tx);
                return true;
            }
            if ev.key == rune::Key::Escape && s.workspace.diff.visible(s) {
                s.close_diff();
                return true;
            }
            if s.question_shortcut(ev, tx) {
                return true;
            }
            if s.environment.is_some() && !ev.mods.alt && !ev.mods.shift && ev.is_command('b') {
                if !tx.key_repeat() {
                    s.toggle_sidebar();
                }
                return true;
            }
            if s.can_operate && !ev.mods.shift && ev.is_command('n') {
                if ev.mods.alt {
                    if !tx.key_repeat() {
                        s.prepare_scratch();
                    }
                    return true;
                }
                if !tx.key_repeat() {
                    s.new_thread_in_workspace();
                }
                return true;
            }
            false
        }),
        families: (appearance::SANS, appearance::MONO),
        ..AppSpec::new(product_name(), (1100.0, 780.0), Desktop::new, view)
    };
    let app = App::new(spec).on_open_url(|s, url, _| {
        s.write_request(None, Request::OpenUrl { url });
    });
    #[cfg(target_os = "macos")]
    let app = app.font_smoothing(rune::FontSmoothing::Off);
    #[cfg(target_os = "macos")]
    let app = window_zoom::view_menu(app);
    let app = app
        .tab_visits_controls(true)
        .menu_item(
            rune::MenuItem::new("Check for Updates…", Desktop::check_updates).id("check-updates"),
        )
        .menu_item(
            rune::MenuItem::new("Settings…", Desktop::open_settings)
                .id("settings")
                .key_equivalent("mod+,".parse().expect("valid settings shortcut"))
                .keys_first(),
        )
        .menu_item(
            rune::MenuItem::new("Command Palette…", Desktop::toggle_palette)
                .id("command-palette")
                .key_equivalent("mod+k".parse().expect("valid palette shortcut"))
                .keys_first()
                .menu("View"),
        )
        .titlebar(native_titlebar(1.))
        .init(move || {
            let mut desktop = Desktop {
                output,
                ..Desktop::new()
            };
            desktop.send(Request::Local);
            desktop
        });
    #[cfg(target_os = "linux")]
    let app = linux_menu::register(app);
    let handle = app.handle();
    if mode.is_some() {
        let owner = handle.clone();
        std::thread::spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            owner.close();
        });
    }
    let images = image_decode::start(handle.clone());
    let source_build = mode == Some("--source-build");
    let reader = std::thread::spawn(move || {
        let mut received_response = false;
        let mut restart = None;
        for line in input.lines() {
            let Ok(line) = line else { break };
            let Ok(reply) = serde_json::from_str::<WorkerMessage>(&line) else {
                break;
            };
            received_response |= reply.id.is_some();
            if let Reply::Image {
                favicon,
                retryable,
                watch,
                key,
                operation,
                data,
                full,
            } = reply.reply
            {
                if images
                    .send(image_decode::Job {
                        watch,
                        key,
                        operation,
                        data,
                        kind: if favicon {
                            image_decode::Kind::Project { retryable }
                        } else if full {
                            image_decode::Kind::Preview
                        } else {
                            image_decode::Kind::Thumbnail
                        },
                    })
                    .is_err()
                {
                    break;
                }
                continue;
            }
            if let Reply::RestartUpdated { executable } = reply.reply {
                restart = Some(executable);
                handle.close();
                continue;
            }
            #[cfg(target_os = "linux")]
            if matches!(&reply.reply, Reply::PairingOffer { .. }) {
                handle.open();
            }
            handle.update(move |s, tx| {
                #[cfg(target_os = "linux")]
                if let Reply::MenuAction { action } = reply.reply {
                    action.activate(s, tx);
                    return;
                }
                if let Reply::EditorOpened { watch, message } = reply.reply {
                    if s.workspace.feed == Some(watch) {
                        tx.announce(message, rune::LiveRegion::Polite);
                    }
                    return;
                }
                if let Reply::ClientAppearance {
                    sans,
                    code,
                    smoothing,
                    chat_width,
                    contrast,
                    error,
                } = reply.reply
                {
                    s.chat_width = chat_width;
                    s.font_smoothing = Some(smoothing);
                    if s.saving_client_appearance {
                        s.appearance_error = error;
                        s.saving_client_appearance = false;
                    }
                    s.receive_contrast(contrast);
                    appearance::apply_fonts(tx, &sans, &code, smoothing);
                    s.font_families.receive(sans, code);
                    return;
                }
                if let Reply::ThreadModels {
                    watch,
                    operation,
                    instance,
                    options,
                    error,
                } = reply.reply
                {
                    s.receive_models(watch, &operation, &instance, options, error);
                    return;
                }
                if let Reply::ThreadPermissions {
                    watch,
                    operation,
                    instance,
                    options,
                    error,
                } = reply.reply
                {
                    s.receive_permissions(watch, &operation, &instance, options, error, tx);
                    return;
                }
                if let Reply::TerminalCopy {
                    watch,
                    operation,
                    request,
                    text,
                } = reply.reply
                {
                    s.receive_terminal_copy(watch, &operation, request, text, tx);
                    return;
                }
                let focus = matches!(reply.reply, Reply::LaunchOptions { .. })
                    && ((s.pending && reply.id == Some(s.request_id))
                        || (s.landing.matches(reply.id)
                            && tx.focused().is_none()
                            && !s.settings_open
                            && s.palette.is_none()));
                let update_palette = matches!(
                    reply.reply,
                    Reply::WorkspaceClock { .. }
                        | Reply::WorkspaceLive { .. }
                        | Reply::Live {
                            update: LiveUpdate::Deleted,
                            ..
                        }
                );
                s.receive(reply);
                s.sync_permission_menu(tx);
                s.close_stale_models();
                if update_palette && let Some(palette) = &s.palette {
                    palette.reveal_selection(tx);
                }
                if focus {
                    tx.focus("new-thread-prompt", true);
                }
            });
        }
        handle.update(|s, _| s.worker_closed());
        let status = child.wait()?;
        if source_build && !received_response && !status.success() {
            handle.close();
        }
        Ok::<_, io::Error>((status, restart))
    });
    // Rune's fast exit would skip reaping the worker and its scoped backend.
    rune::orderly_exit();
    let (app, verified) = if let Some(check) = launch_check {
        let passed = std::rc::Rc::new(std::cell::Cell::new(false));
        (check.observe(app, passed.clone()), Some(passed))
    } else {
        (app, None)
    };
    let result = app.try_run();
    let (status, restart) = reader
        .join()
        .map_err(|_| io::Error::other("The desktop worker reader stopped unexpectedly."))??;
    #[cfg(any(target_os = "linux", windows))]
    rune::release_instance();
    result.map_err(io::Error::other)?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "The desktop worker exited with {status}."
        )));
    }
    if let Some(executable) = restart {
        std::process::Command::new(executable)
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("ARGV0")
            .env_remove("APPIMAGE_EXIT_AFTER_INSTALL")
            .spawn()?;
    }
    if let Some(passed) = verified {
        if !passed.get() {
            return Err(io::Error::other(
                "Window closed before showing the fixture message",
            ));
        }
        println!("T3 Code Rune launch check passed");
    }
    Ok(())
}

fn main() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--exchange-mac-update"))
    {
        return mac_update::exchange();
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some()
        && std::env::var("APPIMAGE_EXIT_AFTER_INSTALL").as_deref() == Ok("true")
    {
        return Ok(());
    }
    start()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archiving_preserves_the_open_folder_draft() {
        let mut desktop = Desktop {
            project_path: Some(TextEdit::new("/shared/repo")),
            pending: true,
            ..Desktop::new()
        };
        desktop.receive(WorkerMessage {
            id: Some(0),
            reply: Reply::ProjectSaved { added: false },
        });
        assert_eq!(desktop.project_path.as_ref().unwrap().text, "/shared/repo");
        desktop.receive(WorkerMessage {
            id: Some(0),
            reply: Reply::ProjectSaved { added: true },
        });
        assert!(desktop.project_path.is_none());
    }

    #[test]
    fn an_old_reply_cannot_finish_reconnection_after_the_local_server_exits() {
        let mut desktop = Desktop {
            environment: Some("This computer".into()),
            request_id: 4,
            pending: true,
            ..Desktop::new()
        };
        desktop.receive(WorkerMessage {
            id: None,
            reply: Reply::LocalClosed,
        });
        assert!(desktop.environment.is_none());
        assert!(!desktop.pending);
        desktop.request_id += 1;
        desktop.pending = true;
        desktop.starting_local = true;
        for reply in [
            Reply::Disconnected,
            Reply::Error {
                message: "Refresh failed".into(),
            },
        ] {
            desktop.receive(WorkerMessage { id: Some(4), reply });
            assert!(desktop.pending && desktop.starting_local);
        }
        desktop.receive(WorkerMessage {
            id: Some(5),
            reply: Reply::Connected {
                label: "This computer".into(),
                details: None,
                stage: stage::Stage::Dev,
                local: true,
                can_operate: true,
                can_write_files: true,
                can_read_files: true,
                can_pin: true,
                terminal_read: true,
                terminal_write: true,
            },
        });
        assert_eq!(desktop.environment.as_deref(), Some("This computer"));
    }
}
