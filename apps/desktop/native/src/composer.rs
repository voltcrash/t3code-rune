use super::workspace::Conversation;
use super::{Desktop, Request, Workspace};
use rune::{El, TextEdit, TextStyle, auto_grow, col, el, row, spacer, text, text_input, tok};
use std::time::{Duration, Instant};
use uuid::Uuid;

struct SendAttempt {
    id: String,
    instance: String,
    model: String,
}

pub(super) struct Draft {
    editor: TextEdit,
    files: Vec<String>,
    pub(super) model: Option<crate::models::Handoff>,
    pub(super) permission: Option<crate::permissions::Selection>,
    operation: Option<SendAttempt>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            editor: TextEdit::new(""),
            operation: None,
            files: vec![],
            permission: None,
            model: None,
        }
    }
}

impl Draft {
    pub(super) fn set_model(
        &mut self,
        pick: Option<crate::models::Handoff>,
        instance: &str,
        model: &str,
    ) {
        if self
            .operation
            .as_ref()
            .is_some_and(|sent| sent.instance != instance || sent.model != model)
        {
            self.operation = None;
        }
        self.model = pick;
        if let Some(permission) = &mut self.permission {
            permission.instance = instance.into();
        }
    }
    pub(super) fn target_instance<'a>(&'a self, current: &'a str) -> &'a str {
        self.model
            .as_ref()
            .filter(|m| m.source == current)
            .map_or(current, |m| m.instance.as_str())
    }
    pub(super) fn select_permission(&mut self, choice: crate::permissions::Selection) {
        self.permission = Some(choice);
    }
    pub(super) fn preserve_answer(&mut self, value: &str) {
        let combined = if self.editor.text.is_empty() {
            value.to_owned()
        } else {
            format!("{}\n\n{value}", self.editor.text)
        };
        self.editor = TextEdit::new(&combined);
        self.operation = None;
    }
    pub(super) fn edit_files(&mut self, change: impl FnOnce(&mut Vec<String>)) {
        let before = self.files.len();
        change(&mut self.files);
        if self.files.len() != before {
            self.operation = None;
        }
    }
    fn submission(
        &mut self,
        thread: &str,
        instance: &str,
        model: &str,
    ) -> Option<Request<'static>> {
        if self.editor.text.trim().is_empty() && self.files.is_empty() {
            return None;
        }
        let operation = self
            .operation
            .get_or_insert_with(|| {
                let (instance, model) = self.model.as_ref().map_or((instance, model), |m| {
                    (m.instance.as_str(), m.model.as_str())
                });
                SendAttempt {
                    id: Uuid::new_v4().to_string(),
                    instance: instance.into(),
                    model: model.into(),
                }
            })
            .id
            .clone();
        Some(Request::SendMessage {
            thread: thread.into(),
            permission: self.permission.clone(),
            model: self.model.clone(),
            text: self.editor.text.clone(),
            files: self.files.clone(),
            operation,
        })
    }
}

impl Workspace {
    pub fn finish_message(&mut self, thread: &str, operation: &str) -> bool {
        if let Some(draft) = self.drafts.get_mut(thread)
            && draft
                .operation
                .as_ref()
                .is_some_and(|sent| sent.id == operation)
        {
            *draft = Draft::default();
        }
        self.conversation
            .as_ref()
            .is_some_and(|pane| pane.id == thread)
    }
}

#[derive(Clone, Copy)]
pub(super) enum StopFeedback {
    Pending(Instant),
    Retry,
    Landed(Instant),
}

impl Conversation {
    pub(super) fn set_active_run(&mut self, run: Option<String>) {
        if self.active_run != run {
            self.stop_operation = None;
            self.stop_feedback = if run.is_none() && self.stop_feedback.is_some() {
                Some(StopFeedback::Landed(
                    Instant::now() + Duration::from_millis(500),
                ))
            } else {
                None
            };
            self.active_run = run;
        }
    }
}

impl Desktop {
    pub(super) fn poll_stop(&mut self, _: f64, tx: &mut rune::Tx) -> bool {
        let Some(pane) = self.workspace.conversation.as_mut() else {
            return false;
        };
        let Some(phase) = pane.stop_feedback else {
            return false;
        };
        let deadline = match phase {
            StopFeedback::Pending(t) | StopFeedback::Landed(t) => t,
            StopFeedback::Retry => return false,
        };
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            tx.wake_after(remaining);
            return false;
        }
        pane.stop_feedback =
            matches!(phase, StopFeedback::Pending(_)).then_some(StopFeedback::Retry);
        true
    }

    fn stop_run(&mut self, thread: &str, run: &str, tx: &mut rune::Tx) {
        if !self.can_operate {
            return;
        }
        let Some(pane) = self.workspace.conversation.as_mut() else {
            return;
        };
        if pane.id != thread
            || pane.active_run.as_deref() != Some(run)
            || matches!(pane.stop_feedback, Some(StopFeedback::Pending(_)))
        {
            return;
        }
        let timeout = Duration::from_secs(10);
        pane.stop_feedback = Some(StopFeedback::Pending(Instant::now() + timeout));
        tx.wake_after(timeout);
        let operation = Uuid::new_v4().to_string();
        pane.stop_operation = Some(operation.clone());
        self.error = None;
        self.write_request(
            None,
            Request::StopRun {
                thread: thread.into(),
                run: run.into(),
                operation,
            },
        );
    }

    fn send_message(&mut self) {
        if self.pending || !self.can_operate {
            return;
        }
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        if matches!(pane.stop_feedback, Some(StopFeedback::Landed(_))) {
            return;
        }
        if self.workspace.questions.contains_key(&pane.id) {
            return;
        }
        let Some(thread) = self.workspace.visible_thread() else {
            return;
        };
        let instance = thread
            .permission
            .as_ref()
            .map(|p| p.instance.clone())
            .unwrap_or_default();
        let model = thread.model.clone();
        let Some(draft) = self.workspace.drafts.get_mut(&pane.id) else {
            return;
        };
        if let Some(request) = draft.submission(&pane.id, &instance, &model) {
            self.send(request);
        }
    }
}

pub(super) fn view(
    s: &Desktop,
    thread: &crate::Thread,
    theme: &tok::Theme,
    cx: &rune::Cx<Desktop>,
    overlays: &mut Vec<El>,
) -> El {
    let dark = s.dark(cx);
    let id = thread.id.as_str();
    let draft = &s.workspace.drafts[id];
    let input_id = id.to_owned();
    let edit_id = id.to_owned();
    let asking = s.workspace.questions.contains_key(id);
    let field = if asking {
        super::questions::input(s, id, theme)
    } else {
        text_input(&draft.editor, TextStyle::new(14.0, 400.0).lh(21.0))
            .key("message-input")
            .label("Message")
            .w_full()
            .color(theme.text)
            .placeholder("Ask anything...", theme.text_secondary)
            .disabled(s.pending)
            .on_edit(move |s: &mut Desktop| {
                &mut s.workspace.drafts.get_mut(&input_id).unwrap().editor
            })
            .on_input_change(move |s: &mut Desktop, _| {
                if let Some(draft) = s.workspace.drafts.get_mut(&edit_id) {
                    draft.operation = None;
                }
            })
            .on_submit(|s: &mut Desktop, _| s.send_message())
    };
    let pane = s
        .workspace
        .conversation
        .as_ref()
        .expect("composer has a conversation");
    let guarded = matches!(pane.stop_feedback, Some(StopFeedback::Landed(_)));
    let empty = draft.editor.text.trim().is_empty() && draft.files.is_empty();
    let send = send_button(
        "Send message",
        !s.pending && !empty && !guarded,
        Desktop::send_message,
    );
    let target = super::attachments::Target::Thread(id.to_owned());
    let mut controls = row().w_full().center_y().gap(9.0);
    if !asking {
        controls = controls.child(target.button(theme, s.pending));
    }
    if !asking && let Some(permission) = &thread.permission {
        let selected = draft
            .permission
            .as_ref()
            .filter(|p| p.instance == draft.target_instance(&permission.instance))
            .or(Some(permission));
        controls = controls.child(crate::permissions::control(
            s,
            selected.map(|p| (p.mode.as_str(), p.label.as_str())),
            theme,
            &rune_kit::Kit::neutral(dark),
            overlays,
            s.pending || s.workspace.terminal_watch().is_none(),
            true,
        ));
    }
    controls = controls.child(spacer());
    if !asking {
        controls = controls.child(crate::models::control(s, thread, theme, cx, overlays));
    }
    let show_stop = s.can_operate
        && pane.active_run.is_some()
        && (asking || empty || pane.stop_feedback.is_some());
    if asking {
        controls = controls.child(super::questions::controls(s, id, theme));
    } else if !show_stop || !empty {
        controls = controls.child(send);
    }
    if show_stop && let Some(run) = &pane.active_run {
        let waiting = matches!(pane.stop_feedback, Some(StopFeedback::Pending(_)));
        let caption = if waiting {
            "Stopping"
        } else {
            "Stop generation"
        };
        let error = rune::color::hex(if dark { "#fb414a" } else { "#fb2c36" }).unwrap();
        let mut stop = el()
            .key(format!("stop-{run}"))
            .role("button")
            .label(caption)
            .help(if waiting { "Stopping…" } else { "Interrupt" })
            .size(32.0, 32.0)
            .radius(16.0)
            .center()
            .fill(error.alpha(0.9))
            .hover(error)
            .shadow(1.0, 2.0, error.alpha(0.24))
            .child(el().size(8.0, 8.0).radius(1.5).fill(rune::Color::WHITE));
        if waiting {
            stop = stop.disabled(true).opacity(0.64);
        } else {
            let thread = pane.id.clone();
            let run = run.clone();
            stop = stop
                .focusable(true)
                .on_press_tx(move |s: &mut Desktop, tx| s.stop_run(&thread, &run, tx));
        }
        controls = controls.child(stop);
    }
    let list = if asking {
        None
    } else {
        target.list(&draft.files, theme, s.pending)
    };
    target.drop_area(
        surface(id, field, controls, theme, list, None),
        asking || s.pending,
    )
}

pub(super) fn send_button(caption: &str, enabled: bool, run: fn(&mut Desktop)) -> El {
    let send = el()
        .key("send-message")
        .label(caption)
        .role("button")
        .help(caption)
        .size(32.0, 32.0)
        .radius(16.0)
        .center()
        .fill(rune::oklch(0.56, 0.19, 259.0))
        .child(
            text("↑")
                .font(TextStyle::new(18.0, 600.0))
                .color(rune::Color::WHITE),
        );
    if !enabled {
        send.disabled(true).opacity(0.5)
    } else {
        send.focusable(true).on_press(run)
    }
}

pub(super) fn surface(
    id: &str,
    field: El,
    controls: El,
    theme: &tok::Theme,
    list: Option<El>,
    footer: Option<El>,
) -> El {
    let surface = col()
        .key(format!("composer-{id}"))
        .w_full()
        .max_w(736.0)
        .self_center()
        .radius(16.0)
        .border(1.0, theme.separator)
        .fill(theme.text.alpha(0.055))
        .px(12.0)
        .pt(14.0)
        .pb(8.0)
        .gap(7.0)
        .children(list)
        .child(auto_grow(field, 2, 7).w_full().min_h(46.0).max_h(144.0))
        .child(controls);
    col()
        .w_full()
        .px(24.0)
        .pt(8.0)
        .pb(20.0)
        .gap(4.)
        .child(surface)
        .children(footer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_stop_receipt_cannot_finish_a_retry_or_an_unrelated_request() {
        use crate::{Reply, WorkerMessage};
        let mut app = Desktop::new();
        app.open_thread("thread");
        let pane = app.workspace.conversation.as_mut().unwrap();
        pane.set_active_run(Some("run".into()));
        pane.stop_operation = Some("retry".into());
        pane.stop_feedback = Some(StopFeedback::Pending(
            Instant::now() + Duration::from_secs(10),
        ));
        app.pending = true;
        app.request_id = 17;
        let receipt = |op: &str| WorkerMessage {
            id: None,
            reply: Reply::StopResult {
                thread: "thread".into(),
                run: "run".into(),
                operation: op.into(),
                error: Some("delivery uncertain".into()),
            },
        };
        app.receive(receipt("previous"));
        assert!(app.error.is_none());
        assert!(matches!(
            app.workspace.conversation.as_ref().unwrap().stop_feedback,
            Some(StopFeedback::Pending(_))
        ));
        app.receive(receipt("retry"));
        assert!(app.pending);
        assert_eq!(app.request_id, 17);
        assert!(matches!(
            app.workspace.conversation.as_ref().unwrap().stop_feedback,
            Some(StopFeedback::Retry)
        ));
    }

    #[test]
    fn a_retry_reuses_the_operation_until_the_matching_send_is_confirmed() {
        let mut ws = Workspace::new();
        let draft = ws.drafts.entry("thread".into()).or_default();
        draft.edit_files(|files| files.push("document.pdf".into()));
        let Request::SendMessage { operation, .. } =
            draft.submission("thread", "codex", "original").unwrap()
        else {
            unreachable!()
        };
        ws.finish_message("thread", "unrelated");
        let draft = ws.drafts.get_mut("thread").unwrap();
        draft.select_permission(crate::permissions::Selection {
            instance: "devReplay".into(),
            mode: "approval-required".into(),
            label: "Supervised".into(),
        });
        let Request::SendMessage {
            operation: retry,
            text,
            files,
            ..
        } = draft.submission("thread", "codex", "original").unwrap()
        else {
            unreachable!()
        };
        assert_eq!(retry, operation);
        assert!(text.is_empty());
        assert_eq!(files, ["document.pdf"]);
        let replay = || crate::models::Handoff {
            project: "project".into(),
            source: "codex".into(),
            instance: "devReplay".into(),
            model: "showcase".into(),
            name: "Showcase".into(),
        };
        draft.set_model(Some(replay()), "devReplay", "showcase");
        let Request::SendMessage {
            operation: changed, ..
        } = draft.submission("thread", "codex", "original").unwrap()
        else {
            unreachable!()
        };
        assert_ne!(changed, operation);
        draft.set_model(Some(replay()), "devReplay", "showcase");
        let Request::SendMessage {
            operation: repeated,
            ..
        } = draft.submission("thread", "codex", "original").unwrap()
        else {
            unreachable!()
        };
        assert_eq!(changed, repeated);
        ws.finish_message("thread", &changed);
        assert!(
            ws.drafts
                .get_mut("thread")
                .unwrap()
                .submission("thread", "codex", "original")
                .is_none()
        );
    }
}
