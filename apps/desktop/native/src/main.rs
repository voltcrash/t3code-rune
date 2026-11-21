#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use rune::{
    App, AppSpec, Cx, El, TextEdit, TextStyle, Titlebar, col, el, row, text, text_input, tok,
};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};

mod approval;
mod composer;
mod image_decode;
mod navigation;
mod new_thread;
mod questions;
mod stage;
mod workspace;
use workspace::{ConversationWindow, LiveStatus, LiveUpdate, Project, Thread, Workspace};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Reply {
    Connected {
        label: String,
        #[serde(default)]
        stage: stage::Stage,
        local: bool,
        can_operate: bool,
    },
    Workspace {
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
    MessageSent {
        thread: String,
        operation: String,
    },
    LaunchOptions {
        record: Project,
        project: String,
        choices: Vec<new_thread::Choice>,
        base: String,
        worktree: bool,
    },
    ThreadLaunched {
        thread: Thread,
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
    Local,
    Pair {
        link: &'a str,
    },
    PrepareScratch,
    PrepareThread {
        project: String,
    },
    LaunchThread {
        project: String,
        runtime: String,
        model: String,
        text: String,
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
    SendMessage {
        thread: String,
        text: String,
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
    output: Option<std::process::ChildStdin>,
    answer_advance: Option<questions::Advance>,
    link: TextEdit,
    environment: Option<String>,
    stage: stage::Stage,
    local: bool,
    can_operate: bool,
    project_path: Option<TextEdit>,
    workspace: Workspace,
    history: navigation::History,
    new_threads: new_thread::NewThreads,
    palette: Option<workspace::palette::Palette>,
    request_id: u64,
    pending: bool,
    starting_local: bool,
    error: Option<String>,
}

impl Desktop {
    fn new() -> Self {
        Self {
            output: None,
            answer_advance: None,
            link: TextEdit::new("").secure(),
            environment: None,
            stage: Default::default(),
            local: false,
            can_operate: false,
            project_path: None,
            workspace: Workspace::new(),
            history: Default::default(),
            new_threads: Default::default(),
            palette: None,
            request_id: 0,
            pending: false,
            starting_local: false,
            error: None,
        }
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
            *self = Self::new();
        }
        sent
    }

    fn send(&mut self, request: Request<'_>) {
        if self.pending || self.output.is_none() {
            return;
        }
        if matches!(request, Request::Thread { .. }) {
            self.close_conversation_inspectors();
        }
        self.answer_advance = None;
        self.request_id += 1;
        let starting_local = matches!(request, Request::Local);
        if self.write_request(Some(self.request_id), request) {
            self.starting_local = starting_local;
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

    fn receive(&mut self, message: WorkerMessage) {
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
            let thread_count = self.workspace.thread_count();
            self.workspace.apply_live(watch, &thread, sequence, update);
            if thread_count != self.workspace.thread_count() {
                self.history.retain(&self.workspace, &self.new_threads);
                if self.palette.is_some() {
                    self.filter_palette(true);
                }
            }
            return;
        }
        if matches!(message.reply, Reply::LocalClosed) {
            if self.environment.is_none() {
                return;
            }
        } else if !self.pending || message.id != Some(self.request_id) {
            return;
        }
        self.pending = false;
        self.starting_local = false;
        self.error = None;
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
                stage,
                local,
                can_operate,
            } => {
                self.stage = stage;
                self.local = local;
                self.can_operate = can_operate;
                self.environment = Some(label);
                self.pending = true;
            }
            Reply::Workspace {
                projects,
                threads,
                scratch_available,
            } => {
                self.workspace.scratch_available = scratch_available;
                self.replace_workspace(projects, threads);
            }
            Reply::LaunchOptions {
                record,
                project,
                choices,
                base,
                worktree,
            } => {
                self.close_conversation_inspectors();
                self.history
                    .visit(navigation::Page::NewThread(project.clone()));
                self.workspace.upsert_project(record);
                self.new_threads
                    .configure(project, choices, worktree, &base);
                self.workspace.show_projects = false;
            }
            Reply::ThreadLaunched { thread } => {
                let id = thread.id.clone();
                self.history.launched(&thread.project, &id);
                self.new_threads.forget(&thread.project);
                self.workspace.insert_launched(thread);
                self.open_thread(&id);
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
                self.workspace.remove_thread(&thread);
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
            Reply::Live { .. }
            | Reply::StopResult { .. }
            | Reply::CommandOutput { .. }
            | Reply::Image { .. }
            | Reply::ToolDetail { .. }
            | Reply::FileDiff { .. } => {
                unreachable!()
            }
            Reply::Error { message } => self.error = Some(message),
            Reply::Disconnected | Reply::LocalClosed => {
                let lost = matches!(message.reply, Reply::LocalClosed);
                let output = self.output.take();
                *self = Self {
                    output,
                    request_id: self.request_id,
                    error: lost.then(|| {
                        "The local environment stopped. Reconnect to start it again.".to_owned()
                    }),
                    ..Self::new()
                };
            }
        }
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

fn view(s: &Desktop, cx: &mut Cx<Desktop>) -> El {
    let dark = cx.system_dark();
    let mut theme = tok::theme(dark);
    theme.bg = if dark {
        rune::Color::BLACK
    } else {
        rune::oklch(0.992, 0.0, 0.0)
    };
    theme.text = rune::color::hex(if dark { "#f5f5f5" } else { "#18181b" }).unwrap();
    theme.text_secondary = rune::color::hex(if dark { "#a3a3a3" } else { "#71717a" }).unwrap();
    theme.separator = theme.text.alpha(0.12);
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
    } else if s.environment.is_some() {
        return workspace::view(s, cx, &theme);
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
    col().w_full().h_full().center().fill(theme.bg).child(panel)
}

fn worker_command(integrated_dev: bool) -> io::Result<std::process::Command> {
    use std::process::Command;

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
        let resources = exe
            .parent()
            .filter(|dir| dir.ends_with("usr/bin"))
            .and_then(|dir| dir.parent())
            .map(|dir| dir.join("lib/t3-code"));
        rune::set_app_id(if resources.is_some() {
            "com.t3tools.T3Code.Rune"
        } else {
            "com.t3tools.T3Code.Rune.Development"
        });
        resources.map(|dir| (dir.join("node"), dir))
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
    if integrated_dev {
        command.arg("--integrated-dev");
    }
    Ok(command)
}

fn start() -> io::Result<()> {
    use std::process::Stdio;

    let integrated_dev = std::env::args_os().any(|arg| arg == "--integrated-dev");
    let mut child = worker_command(integrated_dev)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child.stdin.take();
    let input = BufReader::new(child.stdout.take().unwrap());
    let spec = AppSpec {
        min_size: Some((400.0, 360.0)),
        poll: Some(|s: &mut Desktop, now, tx| {
            let stop = s.poll_stop(now, tx);
            s.poll_answer(tx) || stop
        }),
        on_key: Some(|s: &mut Desktop, ev, tx| {
            if !ev.mods.alt && !ev.mods.shift && ev.is_command('k') {
                if !tx.key_repeat() {
                    s.toggle_palette(tx);
                }
                return true;
            }
            if ev.key == rune::Key::Escape && s.close_image() {
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
        families: (
            "system-ui, Segoe UI, sans-serif",
            "ui-monospace, Menlo, Consolas, monospace",
        ),
        ..AppSpec::new("T3 Code", (1100.0, 780.0), Desktop::new, view)
    };
    let app = App::new(spec)
        .menu_item(
            rune::MenuItem::new("Command Palette…", Desktop::toggle_palette)
                .id("command-palette")
                .key_equivalent("mod+k".parse().expect("valid palette shortcut"))
                .keys_first()
                .menu("View"),
        )
        .titlebar(Titlebar::new(40.0).traffic_lights(16.0, 19.0))
        .init(move || {
            let mut desktop = Desktop {
                output,
                ..Desktop::new()
            };
            desktop.send(Request::Local);
            desktop
        });
    let handle = app.handle();
    if integrated_dev {
        let owner = handle.clone();
        std::thread::spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            owner.close();
        });
    }
    let images = image_decode::start(handle.clone());
    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            let Ok(reply) = serde_json::from_str::<WorkerMessage>(&line) else {
                break;
            };
            if let Reply::Image {
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
                        full,
                    })
                    .is_err()
                {
                    break;
                }
                continue;
            }
            handle.update(move |s, tx| {
                let focus = matches!(reply.reply, Reply::LaunchOptions { .. })
                    && s.pending
                    && reply.id == Some(s.request_id);
                let deleting = matches!(
                    reply.reply,
                    Reply::Live {
                        update: LiveUpdate::Deleted,
                        ..
                    }
                );
                s.receive(reply);
                if deleting && let Some(palette) = &s.palette {
                    palette.reveal_selection(tx);
                }
                if focus {
                    tx.focus("new-thread-prompt", true);
                }
            });
        }
        handle.update(|s, _| *s = Desktop::new());
    });
    // Rune's fast exit would skip reaping the worker and its scoped backend.
    rune::orderly_exit();
    let result = app.try_run();
    child.wait()?;
    result.map_err(io::Error::other)
}

fn main() -> io::Result<()> {
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
                stage: stage::Stage::Dev,
                local: true,
                can_operate: true,
            },
        });
        assert_eq!(desktop.environment.as_deref(), Some("This computer"));
    }
}
