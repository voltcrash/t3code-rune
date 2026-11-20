use super::{Desktop, Request, Workspace};
use rune::{El, TextEdit, TextStyle, auto_grow, col, el, row, spacer, text, text_input, tok};
use uuid::Uuid;

pub(super) struct Draft {
    editor: TextEdit,
    operation: Option<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            editor: TextEdit::new(""),
            operation: None,
        }
    }
}

impl Draft {
    fn submission(&mut self, thread: &str) -> Option<Request<'static>> {
        if self.editor.text.trim().is_empty() {
            return None;
        }
        let operation = self
            .operation
            .get_or_insert_with(|| Uuid::new_v4().to_string())
            .clone();
        Some(Request::SendMessage {
            thread: thread.into(),
            text: self.editor.text.clone(),
            operation,
        })
    }
}

impl Workspace {
    pub fn finish_message(&mut self, thread: &str, operation: &str) -> bool {
        if let Some(draft) = self.drafts.get_mut(thread)
            && draft.operation.as_deref() == Some(operation)
        {
            *draft = Draft::default();
        }
        self.conversation
            .as_ref()
            .is_some_and(|pane| pane.id == thread)
    }
}

impl Desktop {
    fn send_message(&mut self) {
        if self.pending || !self.can_operate {
            return;
        }
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        let Some(draft) = self.workspace.drafts.get_mut(&pane.id) else {
            return;
        };
        if let Some(request) = draft.submission(&pane.id) {
            self.send(request);
        }
    }
}

pub(super) fn view(s: &Desktop, id: &str, model: &str, theme: &tok::Theme) -> El {
    let draft = &s.workspace.drafts[id];
    let input_id = id.to_owned();
    let edit_id = id.to_owned();
    let field = text_input(&draft.editor, TextStyle::new(14.0, 400.0).lh(21.0))
        .key("message-input")
        .label("Message")
        .w_full()
        .color(theme.text)
        .placeholder("Ask anything...", theme.text_secondary)
        .disabled(s.pending)
        .on_edit(move |s: &mut Desktop| &mut s.workspace.drafts.get_mut(&input_id).unwrap().editor)
        .on_input_change(move |s: &mut Desktop, _| {
            if let Some(draft) = s.workspace.drafts.get_mut(&edit_id) {
                draft.operation = None;
            }
        })
        .on_submit(|s: &mut Desktop, _| s.send_message());
    let send = el()
        .key("send-message")
        .label("Send message")
        .role("button")
        .help("Send message")
        .size(32.0, 32.0)
        .radius(16.0)
        .center()
        .fill(rune::oklch(0.56, 0.19, 259.0))
        .child(
            text("↑")
                .font(TextStyle::new(18.0, 600.0))
                .color(rune::Color::WHITE),
        );
    let send = if s.pending || draft.editor.text.trim().is_empty() {
        send.disabled(true).opacity(0.5)
    } else {
        send.focusable(true).on_press(Desktop::send_message)
    };
    let controls = row()
        .w_full()
        .center_y()
        .gap(9.0)
        .child(spacer())
        .child(
            text(model)
                .font(TextStyle::new(13.0, 500.0))
                .color(theme.text_secondary)
                .truncate(),
        )
        .child(send);
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
        .child(auto_grow(field, 2, 7).w_full().min_h(46.0).max_h(144.0))
        .child(controls);
    col().w_full().px(24.0).pt(8.0).pb(20.0).child(surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_reuses_the_operation_until_the_matching_send_is_confirmed() {
        let mut ws = Workspace::new();
        let draft = ws.drafts.entry("thread".into()).or_default();
        draft.editor = TextEdit::new("send once");
        let Request::SendMessage { operation, .. } = draft.submission("thread").unwrap() else {
            unreachable!()
        };
        ws.finish_message("thread", "unrelated");
        let draft = ws.drafts.get_mut("thread").unwrap();
        let Request::SendMessage {
            operation: retry,
            text,
            ..
        } = draft.submission("thread").unwrap()
        else {
            unreachable!()
        };
        assert_eq!(retry, operation);
        assert_eq!(text, "send once");
        ws.finish_message("thread", &operation);
        assert!(
            ws.drafts
                .get_mut("thread")
                .unwrap()
                .submission("thread")
                .is_none()
        );
    }
}
