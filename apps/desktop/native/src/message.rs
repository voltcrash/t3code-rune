use super::{Conversation, Desktop, MessageRow, images, label};
use rune::{Color, El, TextStyle, col, el, icon, row, text, tok};
use rune_kit::{Kit, Markdown, MarkdownStyle, glyphs};

fn copy_prompt(pane: &Conversation, message: &MessageRow, theme: &tok::Theme) -> El {
    let thread = pane.id.clone();
    let key = message.key.clone();
    el().key(format!("copy-prompt-{key}"))
        .role("button")
        .label("Copy message")
        .help("Copy message")
        .focusable(true)
        .size(24., 24.)
        .radius(5.)
        .center()
        .hover(theme.hover)
        .child(icon(&glyphs::COPY, 12.).color(theme.text_secondary))
        .on_press_tx(move |s: &mut Desktop, tx| {
            if let Some(pane) = &s.workspace.conversation
                && pane.id == thread
                && let Some(row) = pane.rows.iter().find(|row| row.key == key)
            {
                tx.copy(&row.body);
            }
        })
}

pub(super) fn prompt(
    s: &Desktop,
    pane: &Conversation,
    message: &MessageRow,
    theme: &tok::Theme,
    surface: Color,
) -> El {
    let mut content = col().gap(4.).w_frac(0.8).self_end();
    let mut body = col()
        .gap(6.)
        .self_end()
        .padding(12.)
        .radius(16.)
        .fill(surface);
    if !message.body.is_empty() {
        body = body.child(
            text(&message.body)
                .font(TextStyle::new(14., 400.).lh(22.75))
                .color(theme.text)
                .exact_lines()
                .wrap_anywhere()
                .selectable(true),
        );
    }
    for file in &message.attachments {
        if let Some(key) = &file.image {
            content = content.child(images::thumbnail(s, key, &file.name, theme, 192.).self_end());
        } else {
            body = body.child(label(&file.name, 12., 500., theme.text_secondary));
        }
    }
    if !message.body.is_empty() || message.attachments.iter().any(|a| a.image.is_none()) {
        content = content.child(body);
    }
    if !message.body.is_empty() {
        content = content.child(
            row()
                .self_end()
                .pr(4.)
                .group_opacity(0., 1.)
                .child(copy_prompt(pane, message, theme)),
        );
    }
    col().w_full().pb(16.).hover_group().child(content)
}

pub(super) fn reply(
    s: &Desktop,
    message: &MessageRow,
    theme: &tok::Theme,
    kit: &Kit,
    style: &MarkdownStyle,
) -> El {
    let mut body = col().w_full().gap(6.).child(
        Markdown::new(&message.body)
            .style(style.clone())
            .selectable(true)
            .build(kit),
    );
    for file in &message.attachments {
        body = body.child(if let Some(key) = &file.image {
            images::thumbnail(s, key, &file.name, theme, 192.)
        } else {
            label(&file.name, 12., 500., theme.text_secondary)
        });
    }
    col().w_full().px(4.).pt(2.).pb(8.).child(body)
}
