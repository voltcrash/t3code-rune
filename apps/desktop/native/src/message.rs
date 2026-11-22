use super::{Conversation, Desktop, MessageRow, images, label};
use rune::{Color, El, TextStyle, col, el, icon, row, text, tok};
use rune_kit::{Kit, Markdown, MarkdownStyle, glyphs};

const DOWNLOAD: rune::Icon = rune::Icon {
    name: "download",
    view: 24.,
    body: r#"<path d="M12 3v12m-5-5 5 5 5-5M4 16v4h16v-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) fn download_button(
    s: &Desktop,
    watch: Option<u64>,
    key: &str,
    id: &str,
    name: &str,
    download: String,
    theme: &tok::Theme,
) -> El {
    let mut button = el()
        .size(28., 28.)
        .center()
        .radius(6.)
        .role("button")
        .label(&download)
        .help(&download)
        .child(icon(&DOWNLOAD, 14.).color(theme.text_secondary));
    if let Some(watch) = watch {
        let key = key.to_owned();
        let id = id.to_owned();
        let name = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
        let safe: String = name.chars().filter(|c| !c.is_control()).collect();
        let filename = if safe.is_empty() || safe == "." || safe == ".." {
            s.workspace.attachment_labels.file.clone()
        } else {
            safe
        };
        button =
            button
                .focusable(true)
                .hover(theme.hover)
                .on_press_tx(move |_: &mut Desktop, tx| {
                    let key = key.clone();
                    let attachment = id.clone();
                    tx.save_file(
                        rune::FileDialogOptions {
                            title: download.clone(),
                            file_name: Some(filename.clone()),
                            ..Default::default()
                        },
                        move |s: &mut Desktop, result, _| match result {
                            Ok(Some(path)) => {
                                s.write_request(
                                    None,
                                    crate::Request::DownloadAttachment {
                                        watch,
                                        key,
                                        attachment,
                                        path,
                                    },
                                );
                            }
                            Ok(None) => {}
                            Err(e) => s.error = Some(e.to_string()),
                        },
                    );
                });
    } else {
        button = button.disabled(true);
    }
    button.no_window_drag()
}

fn file_card(
    s: &Desktop,
    message: &MessageRow,
    name: &str,
    file: &images::FileCard,
    theme: &tok::Theme,
    surface: Color,
) -> El {
    let labels = &s.workspace.attachment_labels;
    let title = if file.pasted {
        labels.pasted.as_str()
    } else {
        name
    };
    let metadata = if file.pasted {
        file.size.clone()
    } else {
        format!(
            "{} · {}",
            file.extension.as_deref().unwrap_or(&labels.file),
            file.size
        )
    };
    let download = labels.download.replace("{label}", title);
    let watch = s.workspace.conversation.as_ref().and_then(|p| p.watch);
    let save = download_button(s, watch, &message.key, &file.id, name, download, theme);
    let trigger = super::workspace_files::attachment_trigger(&message.key, &file.id);
    let mut identity = row().grow(1.).min_w(0.).gap(8.).center_y().radius(8.);
    if let Some(watch) = watch {
        let key = message.key.clone();
        let id = file.id.clone();
        let caption = labels.preview.replace("{name}", title);
        identity = identity
            .key(trigger)
            .role("button")
            .label(&caption)
            .help(&caption)
            .hover(theme.hover)
            .focusable(true)
            .on_press_tx(move |s: &mut Desktop, tx| {
                s.open_attachment_preview(watch, &key, &id, tx);
            });
    }
    row()
        .w_full()
        .max_w(256.)
        .min_w(0.)
        .h(52.)
        .px(8.)
        .gap(8.)
        .radius(16.)
        .fill(surface)
        .center_y()
        .child(
            identity
                .child(
                    el().size(36., 36.)
                        .center()
                        .radius(8.)
                        .fill(theme.bg.alpha(0.6))
                        .child(icon(rune_kit::file_icon(name), 18.).color(theme.text_secondary)),
                )
                .child(
                    col()
                        .min_w(0.)
                        .grow(1.)
                        .gap(2.)
                        .child(label(title, 14., 500., theme.text).truncate())
                        .child(label(&metadata, 12., 400., theme.text_secondary).truncate()),
                ),
        )
        .child(save)
}

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
        } else if let Some(details) = &file.file {
            content = content
                .child(file_card(s, message, &file.name, details, theme, surface).self_end());
        } else {
            body = body.child(label(&file.name, 12., 500., theme.text_secondary));
        }
    }
    if !message.body.is_empty()
        || message
            .attachments
            .iter()
            .any(|a| a.image.is_none() && a.file.is_none())
    {
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
        } else if let Some(details) = &file.file {
            file_card(s, message, &file.name, details, theme, theme.card)
        } else {
            label(&file.name, 12., 500., theme.text_secondary)
        });
    }
    col().w_full().px(4.).pt(2.).pb(8.).child(body)
}
