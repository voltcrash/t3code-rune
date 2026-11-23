use super::{Desktop, Origin, Preview, Request, Topbar, label};
use rune::{El, el, row, tok};

#[derive(Clone)]
pub(super) struct Attachment {
    pub(super) watch: u64,
    pub(super) key: String,
    pub(super) id: String,
    pub(super) size: String,
    pub(super) kind: String,
}

pub(in crate::workspace) fn trigger(key: &str, id: &str) -> String {
    format!("preview-file-{key}-{id}")
}

impl Attachment {
    pub(super) fn request(&self, operation: u64, cancel: bool) -> Request<'static> {
        Request::PreviewAttachment {
            watch: self.watch,
            key: self.key.clone(),
            attachment: self.id.clone(),
            operation,
            cancel,
        }
    }
    pub(super) fn reads_text(&self) -> bool {
        matches!(self.kind.as_str(), "text" | "markdown" | "html" | "mermaid")
    }
}

impl Desktop {
    pub(in crate::workspace) fn open_attachment_preview(
        &mut self,
        watch: u64,
        key: &str,
        id: &str,
        tx: &mut rune::Tx,
    ) {
        if self.pending {
            return;
        }
        let Some((attachment, details)) = self.timeline_attachment(watch, key, id) else {
            return;
        };
        let path = attachment.name.clone();
        let file = Attachment {
            watch,
            key: key.to_owned(),
            id: id.to_owned(),
            size: details.size.clone(),
            kind: details.kind.clone(),
        };
        self.close_browser();
        self.close_diff();
        self.close_file_preview();
        let operation = self.next_request_id();
        if file.reads_text() {
            self.write_request(None, file.request(operation, false));
        }
        self.workspace.file_preview = Some(Preview {
            rendered: file.kind == "markdown",
            origin: Origin::Attachment(file),
            operation,
            path,
            source: None,
            error: None,
        });
        tx.focus_when_shown("attachment-panel-close", true);
    }

    pub(super) fn timeline_attachment(
        &self,
        watch: u64,
        key: &str,
        id: &str,
    ) -> Option<(
        &crate::workspace::images::Attachment,
        &crate::workspace::images::FileCard,
    )> {
        let pane = self.workspace.conversation.as_ref()?;
        if self.workspace.show_projects
            || self.new_threads.active.is_some()
            || pane.watch != Some(watch)
        {
            return None;
        }
        pane.rows
            .iter()
            .find(|r| r.key == key)?
            .attachments
            .iter()
            .find_map(|a| {
                let file = a.file.as_ref()?;
                (file.id == id).then_some((a, file))
            })
    }
}

pub(super) fn header(
    app: &Desktop,
    file: &Attachment,
    preview: &Preview,
    theme: &tok::Theme,
    bar: Topbar,
) -> El {
    use rune_kit::glyphs;
    let labels = &app.workspace.attachment_labels;
    let button = |title: &str, glyph: &'static rune::Icon| {
        el().size(28., 28.)
            .no_window_drag()
            .center()
            .radius(6.)
            .role("button")
            .label(title)
            .help(title)
            .hover(theme.hover)
            .focusable(true)
            .child(rune::icon(glyph, 14.).color(theme.text_secondary))
    };
    let mut header = row()
        .h(bar.height)
        .window_drag()
        .center_y()
        .gap(4.)
        .pl(bar.leading + 8.)
        .pr(bar.trailing + 8.)
        .child(label(&labels.attachment, 12., 400., theme.text_secondary).truncate())
        .child(rune::icon(&glyphs::CARET_RIGHT, 10.).color(theme.text_secondary))
        .child(
            label(&preview.path, 12., 500., theme.text)
                .grow(1.)
                .min_w(0.)
                .truncate()
                .help(&preview.path),
        )
        .child(label(&file.size, 12., 400., theme.text_secondary));
    if let Some(source) = &preview.source {
        if file.kind == "markdown" && source.can_render_markdown() {
            let (caption, glyph) = if preview.rendered {
                (&labels.source, &glyphs::FILE_CODE)
            } else {
                (&labels.rendered, &glyphs::EYE)
            };
            header = header.child(button(caption, glyph).on_press(|s: &mut Desktop| {
                if let Some(p) = &mut s.workspace.file_preview {
                    p.rendered = !p.rendered;
                }
            }));
        }
        header = header.child(
            button(
                if source.truncated {
                    &labels.copy_preview
                } else {
                    &labels.copy
                },
                &glyphs::COPY,
            )
            .on_press_tx(|s: &mut Desktop, tx| {
                if let Some(source) = s
                    .workspace
                    .file_preview
                    .as_ref()
                    .and_then(|p| p.source.as_ref())
                {
                    tx.copy(&source.contents);
                }
            }),
        );
    }
    header
        .child(crate::workspace::message::download_button(
            app,
            Some(file.watch),
            &file.key,
            &file.id,
            &preview.path,
            labels.download.replace("{label}", &preview.path),
            theme,
        ))
        .children(app.maximize_control(theme))
        .child(
            button(&labels.file_close, &glyphs::CLOSE)
                .key("attachment-panel-close")
                .on_press_tx(Desktop::dismiss_file_preview),
        )
}
