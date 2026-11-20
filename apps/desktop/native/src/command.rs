use super::{Conversation, Desktop, MessageRow, Request};
use rune::{El, ScrollSpec, TextStyle, col, el, icon, rich, row, text, tok};
use rune_kit::{Kit, glyphs, syntax};
use uuid::Uuid;

pub(super) struct Expanded {
    key: String,
    operation: String,
    text: String,
}
impl Conversation {
    pub(crate) fn update_output(&mut self, operation: &str, text: String) {
        if let Some(command) = &mut self.command
            && command.operation == operation
        {
            command.text = text;
            self.revision += 1;
        }
    }
}

impl Desktop {
    pub(crate) fn close_command(&mut self) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        if let Some(command) = pane.command.take()
            && let Some(watch) = pane.watch
        {
            pane.revision += 1;
            self.write_request(
                None,
                Request::InspectCommand {
                    watch,
                    key: None,
                    operation: command.operation,
                },
            );
        }
    }

    fn toggle_command(&mut self, watch: u64, key: &str) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        if self.pending || pane.watch != Some(watch) {
            return;
        }
        if pane.command.as_ref().is_some_and(|c| c.key == key) {
            return self.close_command();
        }
        let operation = Uuid::new_v4().to_string();
        pane.command = Some(Expanded {
            key: key.to_owned(),
            operation: operation.clone(),
            text: "Loading output…".into(),
        });
        pane.revision += 1;
        self.write_request(
            None,
            Request::InspectCommand {
                watch,
                key: Some(key.to_owned()),
                operation,
            },
        );
    }
}

pub(super) fn row_view(
    app: &Desktop,
    item: &MessageRow,
    caption: &str,
    theme: &tok::Theme,
    kit: &Kit,
) -> El {
    let pane = app.workspace.conversation.as_ref().unwrap();
    let open = pane.command.as_ref().filter(|c| c.key == item.key);
    let key = item.key.clone();
    let mut toggle = row()
        .key(format!("command-{key}"))
        .role("button")
        .label(caption)
        .expanded(open.is_some())
        .w_full()
        .px(2.)
        .radius(6.)
        .hover(theme.hover)
        .center_y()
        .gap(6.)
        .min_h(24.)
        .child(
            el().size(24., 24.)
                .center()
                .child(icon(&super::approval::TERMINAL, 16.).color(if item.failed {
                    kit.c.danger.icon
                } else {
                    theme.text_secondary
                })),
        )
        .child(
            super::label(caption, 14., 400., theme.text_secondary)
                .truncate()
                .grow(1.)
                .min_w(0.),
        )
        .child(
            icon(&glyphs::CHEVRON_DOWN, 14.)
                .color(theme.text_secondary)
                .rotate(if open.is_some() {
                    0.
                } else {
                    -std::f32::consts::FRAC_PI_2
                }),
        );
    if let Some(watch) = pane.watch
        && !app.pending
    {
        toggle = toggle
            .focusable(true)
            .on_press(move |app: &mut Desktop| app.toggle_command(watch, &key));
    } else {
        toggle = toggle.disabled(true);
    }
    let mut content = col().w_full().pb(8.).gap(2.).child(toggle);
    if let Some(command) = open {
        let line = |s: &str| {
            text(s)
                .font(TextStyle::new(12., 400.).mono().lh(18.))
                .color(theme.text_secondary)
                .wrap_anywhere()
                .selectable(true)
        };
        let language = item.language.as_deref().unwrap_or("shellscript");
        let tokens = syntax::highlight(Some(language), &item.body);
        let source = rich(
            item.body.clone(),
            syntax::SyntaxTheme::from_kit(kit).spans(&tokens, 0),
        )
        .font(TextStyle::new(12., 400.).mono().lh(18.))
        .color(theme.text)
        .wrap_anywhere()
        .selectable(true);
        let mut details = col().w_full().gap(6.).pb(8.).child(source).child(
            col()
                .key(format!("output-{}", item.key))
                .max_h(320.)
                .scroll(ScrollSpec { stick_bottom: true })
                .child(line(&command.text)),
        );
        if let Some(code) = item.exit_code.filter(|code| *code != 0) {
            details = details.child(line(&format!("exit {code}")).color(kit.c.danger.fg));
        }
        content = content.child(details);
    }
    content
}
