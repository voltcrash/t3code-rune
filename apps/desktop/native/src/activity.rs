use super::{Desktop, MessageRow};
use rune::{Color, El, Icon, ScrollSpec, TextStyle, col, el, icon, row, tok};
use rune_kit::{Kit, Markdown, MarkdownStyle, glyphs};

const TASKS: Icon = Icon {
    name: "list-todo",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="6" height="6" rx="1"/><path d="m3 17 2 2 4-4m4-8h8m-8 10h8"/></g>"#,
};
const THOUGHT: Icon = Icon {
    name: "brain",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 18V5a3 3 0 0 0-6-.4A4.5 4.5 0 0 0 3 11a4.5 4.5 0 0 0 1.5 7.5A4 4 0 0 0 12 18Zm0 0V5a3 3 0 0 1 6-.4 4.5 4.5 0 0 1 3 6.4 4.5 4.5 0 0 1-1.5 7.5A4 4 0 0 1 12 18Z"/><path d="M6 5q0 2 2 3m10-3q0 2-2 3m-4 4q-2 0-3 3m3-3q2 0 3 3"/></g>"#,
};

pub(super) fn disclosure(
    key: &str,
    caption: &str,
    glyph: &'static Icon,
    ink: Color,
    open: bool,
    theme: &tok::Theme,
) -> El {
    row()
        .key(format!("activity-{key}"))
        .role("button")
        .label(caption)
        .expanded(open)
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
                .child(icon(glyph, 16.).color(ink)),
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
                .rotate(if open {
                    0.
                } else {
                    -std::f32::consts::FRAC_PI_2
                }),
        )
}

pub(super) const PREVIEW_SOURCE_BYTES: usize = 8192;

fn thought_preview(body: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, TagEnd};
    let source = &body[..body.floor_char_boundary(PREVIEW_SOURCE_BYTES.min(body.len()))];
    let mut plain = String::new();
    for event in Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_MATH,
    ) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => plain.push_str(&value),
            Event::HardBreak
            | Event::SoftBreak
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::CodeBlock,
            ) => plain.push(' '),
            _ => {}
        }
    }
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(2048)
        .collect()
}

pub(super) fn row_view(
    app: &Desktop,
    item: &MessageRow,
    caption: &str,
    theme: &tok::Theme,
    kit: &Kit,
    style: &MarkdownStyle,
) -> El {
    let pane = app.workspace.conversation.as_ref().unwrap();
    let open = pane.expanded_activities.contains(&item.key);
    let reasoning = item.role == "reasoning";
    let title = if reasoning && !open {
        let preview = item.preview.get_or_init(|| thought_preview(&item.body));
        if preview.is_empty() { caption } else { preview }
    } else {
        caption
    };
    let key = item.key.clone();
    let thread = pane.id.clone();
    let button = disclosure(
        &key,
        title,
        if reasoning {
            &THOUGHT
        } else if item.file.is_some() {
            &glyphs::FILE_CODE
        } else {
            &TASKS
        },
        if item.failed {
            kit.c.danger.icon
        } else {
            theme.text_secondary
        },
        open,
        theme,
    )
    .focusable(true)
    .on_press(move |s: &mut Desktop| {
        if let Some(pane) = &mut s.workspace.conversation
            && pane.id == thread
        {
            if !pane.expanded_activities.remove(&key) {
                pane.expanded_activities.insert(key.clone());
            }
            pane.revision += 1;
        }
    });
    let mut group = col().w_full().pb(8.).child(button);
    if open {
        let body = if let Some(file) = &item.file {
            super::file_diff::details(app, item, file, theme, kit)
        } else if reasoning {
            Markdown::new(&item.body)
                .style(style.clone())
                .selectable(true)
                .build(kit)
        } else {
            rune::text(&item.body)
                .font(TextStyle::new(12., 400.).mono().lh(18.))
                .wrap_anywhere()
                .selectable(true)
                .color(theme.text_secondary)
        };
        group = group.child(
            col()
                .w_full()
                .pl(if reasoning { 28. } else { 0. })
                .py(4.)
                .max_h(384.)
                .scroll(ScrollSpec {
                    stick_bottom: reasoning && caption == "Thinking",
                })
                .child(body),
        );
    }
    group
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thought_preview_reads_markdown_text_and_caps_visible_characters() {
        let body = format!(
            "**Inspect** [the manifest](https://example.com)\n\n- `node`\n- {}",
            "🚀".repeat(3000)
        );
        let preview = thought_preview(&body);
        assert!(preview.starts_with("Inspect the manifest node 🚀"));
        assert_eq!(preview.chars().count(), 2048);
    }
}
