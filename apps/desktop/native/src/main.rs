#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use rune::{
    App, AppSpec, Cx, El, ScrollSpec, TextEdit, TextStyle, col, el, row, text, text_input, tok,
};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};

#[derive(Deserialize)]
struct Project {
    title: String,
    path: String,
    archived: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Reply {
    Connected { label: String },
    Projects { projects: Vec<Project> },
    Error { message: String },
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
    id: u64,
    #[serde(flatten)]
    request: Request<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Request<'a> {
    Local,
    Pair { link: &'a str },
    Refresh,
    Disconnect,
}

struct Desktop {
    output: Option<std::process::ChildStdin>,
    link: TextEdit,
    environment: Option<String>,
    projects: Vec<Project>,
    request_id: u64,
    pending: bool,
    starting_local: bool,
    error: Option<String>,
}

impl Desktop {
    fn new() -> Self {
        Self {
            output: None,
            link: TextEdit::new("").secure(),
            environment: None,
            projects: Vec::new(),
            request_id: 0,
            pending: false,
            starting_local: false,
            error: None,
        }
    }

    fn send(&mut self, request: Request<'_>) {
        if self.pending {
            return;
        }
        let Some(mut output) = self.output.as_mut() else {
            return;
        };
        self.request_id += 1;
        let starting_local = matches!(request, Request::Local);
        let message = ClientMessage {
            id: self.request_id,
            request,
        };
        let sent = serde_json::to_writer(&mut output, &message).is_ok()
            && output
                .write_all(b"\n")
                .and_then(|()| output.flush())
                .is_ok();
        if sent {
            self.starting_local = starting_local;
            self.pending = true;
            self.error = None;
        } else {
            *self = Self::new();
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
            Reply::Connected { label } => {
                self.environment = Some(label);
                self.pending = true;
            }
            Reply::Projects { projects } => self.projects = projects,
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
    } else if let Some(environment) = &s.environment {
        panel = panel
            .h_full()
            .child(title("Projects"))
            .child(text(environment).color(theme.text_secondary))
            .child(
                row()
                    .gap(8.0)
                    .child(button("Refresh", !s.pending, &theme, |s| {
                        s.send(Request::Refresh)
                    }))
                    .child(button("Disconnect", !s.pending, &theme, |s| {
                        s.send(Request::Disconnect)
                    })),
            );
        let mut list = col()
            .w_full()
            .grow(1.0)
            .shrink()
            .gap(8.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        for project in &s.projects {
            let mut heading = row().center_y().gap(8.0).child(
                text(&project.title)
                    .font(TextStyle::new(14.0, 600.0))
                    .color(theme.text)
                    .truncate(),
            );
            if project.archived {
                heading = heading.child(text("Archived").color(theme.text_secondary));
            }
            list = list.child(
                col()
                    .key(&project.path)
                    .w_full()
                    .gap(4.0)
                    .padding(12.0)
                    .radius(8.0)
                    .border(1.0, theme.separator)
                    .child(heading)
                    .child(
                        text(&project.path)
                            .color(theme.text_secondary)
                            .truncate_middle(),
                    ),
            );
        }
        if !s.pending && s.error.is_none() && s.projects.is_empty() {
            list = list.child(text("No projects").color(theme.text_secondary));
        }
        panel = panel.child(list);
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

fn start() -> io::Result<()> {
    use std::process::{Command, Stdio};

    let node = std::env::var_os("T3CODE_RUNE_NODE").ok_or_else(|| {
        io::Error::other("Launch the native desktop with vp run dev:desktop:rune.")
    })?;
    let worker = std::env::var_os("T3CODE_RUNE_WORKER")
        .ok_or_else(|| io::Error::other("Missing desktop worker path."))?;
    let mut child = Command::new(node)
        .arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child.stdin.take();
    let input = BufReader::new(child.stdout.take().unwrap());
    let app =
        App::new(AppSpec::new("T3 Code", (1100.0, 780.0), Desktop::new, view)).init(move || {
            let mut desktop = Desktop {
                output,
                ..Desktop::new()
            };
            desktop.send(Request::Local);
            desktop
        });
    let handle = app.handle();
    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            let Ok(reply) = serde_json::from_str(&line) else {
                break;
            };
            handle.update(move |s, _| s.receive(reply));
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
            },
        });
        assert_eq!(desktop.environment.as_deref(), Some("This computer"));
    }
}
