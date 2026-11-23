#[path = "workspace_files/attachment.rs"]
mod attachment;
use super::{Desktop, Request, Topbar, action_with_tx, label};
use attachment::Attachment;
pub(in crate::workspace) use attachment::trigger as attachment_trigger;
use rune::{El, GrowthSpec, Span, TextStyle, col, el, rich, row, tok};
use rune_kit::{Kit, syntax};
use serde::Serialize;
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct Location {
    pub watch: u64,
    pub(super) root: String,
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub(crate) enum Input {
    Search { query: String, cancel: bool },
    Filter { query: String, cancel: bool },
    Read { path: String, cancel: bool },
    Resolve { href: String },
    List { directory: String, cancel: bool },
}

#[derive(Clone)]
enum Origin {
    Workspace(Location),
    Attachment(Attachment),
}

#[derive(serde::Deserialize)]
pub(crate) struct LinkTarget {
    path: String,
    line: Option<usize>,
}

pub(crate) struct Preview {
    reveal_line: Option<usize>,
    origin: Origin,
    rendered: bool,
    operation: u64,
    path: String,
    source: Option<Source>,
    error: Option<String>,
}

const SOURCE_ROW_BYTES: usize = 4096;

struct Source {
    contents: String,
    lines: Vec<usize>,
    continuations: Vec<usize>,
    highlights: Rc<[syntax::Highlight]>,
    truncated: bool,
}

impl Source {
    fn new(path: &str, contents: String, truncated: bool, attachment: bool) -> Self {
        let mut lines = Vec::new();
        let mut continuations = Vec::new();
        let mut start = 0;
        for line in contents.split('\n') {
            lines.push(start);
            let mut offset = 0;
            let visible = line.strip_suffix('\r').unwrap_or(line);
            while attachment && visible.len() - offset > SOURCE_ROW_BYTES {
                offset = line.floor_char_boundary(offset + SOURCE_ROW_BYTES);
                continuations.push(lines.len());
                lines.push(start + offset);
            }
            start += line.len() + 1;
        }
        let highlights = if !attachment || Self::rich_text_fits(contents.len(), lines.len()) {
            syntax::highlight(syntax::lang_for_path(path), &contents)
        } else {
            Rc::from([])
        };
        Self {
            contents,
            lines,
            continuations,
            highlights,
            truncated,
        }
    }

    // Rune lays out rendered Markdown as one document. Larger attachments use virtual source rows.
    fn rich_text_fits(bytes: usize, lines: usize) -> bool {
        bytes <= 64 * 1024 && lines <= 512
    }

    fn can_render_markdown(&self) -> bool {
        Self::rich_text_fits(self.contents.len(), self.lines.len())
    }

    fn line(&self, index: usize) -> Range<usize> {
        let start = self.lines[index];
        let end = self
            .lines
            .get(index + 1)
            .copied()
            .unwrap_or(self.contents.len());
        if self.continuations.binary_search(&(index + 1)).is_ok() {
            return start..end;
        }
        let line = self.contents[start..end]
            .strip_suffix('\n')
            .unwrap_or(&self.contents[start..end]);
        start..(start + line.strip_suffix('\r').unwrap_or(line).len())
    }

    fn line_number(&self, row: usize) -> Option<usize> {
        self.continuations
            .binary_search(&row)
            .err()
            .map(|preceding| row + 1 - preceding)
    }

    fn spans(&self, line: Range<usize>, theme: &syntax::SyntaxTheme) -> Vec<Span> {
        let start = self
            .highlights
            .partition_point(|(r, _)| r.end <= line.start);
        self.highlights[start..]
            .iter()
            .take_while(|(r, _)| r.start < line.end)
            .map(|(r, paint)| {
                let mut span = Span::new(
                    r.start.saturating_sub(line.start),
                    r.end.min(line.end) - line.start,
                );
                span.color = Some(theme.paint(*paint));
                if let syntax::Paint::Kind(kind) = paint {
                    span.weight = theme.weight(*kind);
                }
                span
            })
            .collect()
    }
}

impl Desktop {
    pub(crate) fn thread_workspace_root(&self) -> Option<&str> {
        let thread = self.workspace.visible_thread()?;
        thread.worktree.as_deref().or_else(|| {
            self.workspace
                .project(&thread.project)
                .map(|p| p.path.as_str())
        })
    }

    pub(crate) fn file_location(&self) -> Option<Location> {
        if !self.can_read_files || self.new_threads.active.is_some() {
            return None;
        }
        Some(Location {
            watch: self.workspace.terminal_watch()?,
            root: self.thread_workspace_root()?.into(),
        })
    }

    pub(super) fn file_request(&mut self, location: Location, operation: u64, input: Input) {
        self.write_request(
            None,
            Request::WorkspaceFiles {
                location,
                operation,
                input,
            },
        );
    }

    pub(crate) fn close_file_preview(&mut self) {
        self.workspace.resolving_file_link = None;
        if let Some(p) = self.workspace.file_preview.take() {
            match p.origin {
                Origin::Workspace(location) => self.file_request(
                    location,
                    p.operation,
                    Input::Read {
                        path: p.path,
                        cancel: true,
                    },
                ),
                Origin::Attachment(file) if file.reads_text() => {
                    self.write_request(None, file.request(p.operation, true));
                }
                Origin::Attachment(_) => {}
            }
        }
    }

    pub(crate) fn dismiss_file_preview(&mut self, tx: &mut rune::Tx) {
        let trigger =
            self.workspace
                .file_preview
                .as_ref()
                .and_then(|preview| match &preview.origin {
                    Origin::Attachment(file) => Some(attachment_trigger(&file.key, &file.id)),
                    Origin::Workspace(_) => None,
                });
        if self.workspace.explorer.is_none() {
            self.set_panel_maximized(false);
        }
        self.close_file_preview();
        if let Some(key) = trigger {
            tx.focus_when_shown(key, true);
        } else if self.workspace.explorer.is_some() {
            tx.focus_when_shown("workspace-tree", true);
        }
    }

    pub(super) fn open_workspace_file(&mut self, location: Location, path: String) {
        self.preview_workspace_path(location, path, None);
    }

    pub(super) fn resolve_workspace_link(&mut self, location: Location, href: String) {
        let operation = self.next_request_id();
        self.workspace.resolving_file_link = Some((operation, location.clone()));
        self.file_request(location, operation, Input::Resolve { href });
    }

    pub(crate) fn receive_workspace_link(&mut self, operation: u64, link: Option<LinkTarget>) {
        let Some((_, location)) = self
            .workspace
            .resolving_file_link
            .take_if(|(id, _)| *id == operation)
        else {
            return;
        };
        if let Some(link) = link {
            self.preview_workspace_path(location, link.path, link.line);
        }
    }

    fn preview_workspace_path(&mut self, location: Location, path: String, line: Option<usize>) {
        if self.file_location().as_ref() != Some(&location) {
            return;
        }
        self.close_browser();
        self.close_diff();
        self.close_file_preview();
        let operation = self.next_request_id();
        self.workspace.file_preview = Some(Preview {
            origin: Origin::Workspace(location.clone()),
            reveal_line: line,
            rendered: false,
            operation,
            path: path.clone(),
            source: None,
            error: None,
        });
        self.file_request(
            location,
            operation,
            Input::Read {
                path,
                cancel: false,
            },
        );
    }

    pub(super) fn reload_file_preview(&mut self) {
        let Some(p) = &self.workspace.file_preview else {
            return;
        };
        match &p.origin {
            Origin::Workspace(location) => {
                self.preview_workspace_path(location.clone(), p.path.clone(), p.reveal_line)
            }
            Origin::Attachment(file) => {
                if !file.reads_text() {
                    return;
                }
                let file = file.clone();
                let operation = self.next_request_id();
                if let Some(p) = &mut self.workspace.file_preview {
                    p.operation = operation;
                    p.error = None;
                }
                self.write_request(None, file.request(operation, false));
            }
        }
    }

    pub(crate) fn receive_file_source(
        &mut self,
        operation: u64,
        contents: Option<String>,
        truncated: bool,
        error: Option<String>,
    ) {
        if let Some(p) = &mut self.workspace.file_preview
            && p.operation == operation
        {
            p.source = contents.map(|text| {
                Source::new(
                    &p.path,
                    text,
                    truncated,
                    matches!(p.origin, Origin::Attachment(_)),
                )
            });
            if p.source
                .as_ref()
                .is_some_and(|source| !source.can_render_markdown())
            {
                p.rendered = false;
            }
            p.error = error;
        }
    }

    pub(crate) fn reconcile_file_location(&mut self) {
        let changed = |location: &Location| {
            self.workspace.terminal_watch() != Some(location.watch)
                || self.new_threads.active.is_some()
                || self.thread_workspace_root() != Some(location.root.as_str())
        };
        let preview_changed =
            self.workspace
                .file_preview
                .as_ref()
                .is_some_and(|p| match &p.origin {
                    Origin::Workspace(location) => changed(location),
                    Origin::Attachment(file) => self
                        .timeline_attachment(file.watch, &file.key, &file.id)
                        .is_none_or(|(a, f)| {
                            a.name != p.path || f.kind != file.kind || f.size != file.size
                        }),
                });
        let search_changed = self.file_search_location().is_some_and(changed);
        if self
            .workspace
            .explorer
            .as_ref()
            .is_some_and(|tree| changed(&tree.location))
        {
            self.close_explorer();
        }
        if preview_changed {
            self.close_file_preview();
        }
        if search_changed {
            self.dismiss_palette();
        }
    }
}

pub(super) fn view(
    app: &Desktop,
    theme: &tok::Theme,
    kit: &Kit,
    bar: Topbar,
    tree: Option<El>,
) -> El {
    let Some(preview) = &app.workspace.file_preview else {
        return el();
    };
    let header = if let Origin::Attachment(file) = &preview.origin {
        attachment::header(app, file, preview, theme, bar)
    } else {
        row()
            .h(bar.height)
            .window_drag()
            .center_y()
            .pl(bar.leading + 12.)
            .pr(bar.trailing + 12.)
            .gap(8.)
            .child(rune::icon(rune_kit::file_icon(&preview.path), 16.).color(theme.text_secondary))
            .child(
                label(&preview.path, 13., 500., theme.text)
                    .grow(1.)
                    .min_w(0.)
                    .truncate()
                    .help(&preview.path),
            )
            .children(app.maximize_control(theme))
            .child(action_with_tx(
                "Close file",
                theme,
                true,
                Desktop::dismiss_file_preview,
            ))
    };
    let panel = col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(header)
        .child(el().h(1.).w_full().fill(theme.separator));
    let mut body = row()
        .grow(1.)
        .min_h(0.)
        .w_full()
        .child(source_view(app, preview, theme, kit));
    if let Some(tree) = tree {
        body = body
            .child(el().w(1.).h_full().fill(theme.separator))
            .child(tree);
    }
    panel.child(body)
}

fn source_view(app: &Desktop, preview: &Preview, theme: &tok::Theme, kit: &Kit) -> El {
    let mut panel = col().grow(1.).min_w(0.).min_h(0.);
    let labels = &app.workspace.attachment_labels;
    let attachment = matches!(preview.origin, Origin::Attachment(_));
    if matches!(&preview.origin, Origin::Attachment(file) if !file.reads_text()) {
        return panel
            .center()
            .child(label(&labels.no_preview, 14., 500., theme.text).wrap());
    }
    let Some(source) = &preview.source else {
        if attachment && let Some(error) = &preview.error {
            return panel
                .center()
                .gap(8.)
                .child(label(error, 13., 400., theme.text_secondary).wrap())
                .child(super::action(
                    &labels.retry,
                    theme,
                    !app.pending,
                    Desktop::reload_file_preview,
                ));
        }
        return panel.child(
            label(
                preview.error.as_deref().unwrap_or(if attachment {
                    &labels.loading
                } else {
                    "Loading file…"
                }),
                13.,
                400.,
                theme.text_secondary,
            )
            .padding(12.)
            .wrap(),
        );
    };
    if source.truncated {
        panel = panel.child(
            label(
                if attachment {
                    &labels.truncated
                } else {
                    "File preview truncated."
                },
                12.,
                400.,
                theme.text_secondary,
            )
            .px(12.)
            .py(4.),
        );
    }
    if preview.rendered {
        return panel.child(
            col()
                .grow(1.)
                .min_h(0.)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    rune_kit::Markdown::new(&source.contents)
                        .selectable(true)
                        .build(kit)
                        .padding(12.),
                ),
        );
    }
    let syntax = syntax::SyntaxTheme::from_kit(kit);
    let colors = *theme;
    let gutter = 16. + source.lines.len().to_string().len() as f32 * 8.;
    let rows = col()
        .growth(GrowthSpec::default())
        .virtual_rows(
            source.lines.len(),
            20.,
            |_: &Desktop, i| i.to_string().into(),
            move |s: &Desktop, i| {
                let Some(source) = s
                    .workspace
                    .file_preview
                    .as_ref()
                    .and_then(|p| p.source.as_ref())
                else {
                    return el();
                };
                if i >= source.lines.len() {
                    return el();
                }
                let range = source.line(i);
                let line = rich(
                    &source.contents[range.clone()],
                    source.spans(range, &syntax),
                )
                .font(TextStyle::new(12., 400.).mono().lh(20.))
                .color(colors.text)
                .selectable(true);
                row()
                    .h(20.)
                    .gap(8.)
                    .child(
                        label(
                            &source
                                .line_number(i)
                                .map_or_else(String::new, |n| n.to_string()),
                            12.,
                            400.,
                            colors.text_secondary,
                        )
                        .w(gutter)
                        .pl(8.)
                        .a11y_hidden(),
                    )
                    .child(line)
            },
        )
        .virtual_rows_version(preview.operation);
    let operation = preview.operation;
    panel.child(
        col()
            .key(format!("file-source-{operation}"))
            .on_layout_tx(move |s: &mut Desktop, _, tx| {
                if let Some(p) = &mut s.workspace.file_preview
                    && p.operation == operation
                    && let Some(line) = p.reveal_line.take()
                    && let Some(source) = &p.source
                {
                    let row = line.min(source.lines.len()).saturating_sub(1);
                    tx.scroll_to_y(format!("file-source-{operation}"), row as f32 * 20.);
                }
            })
            .grow(1.)
            .min_h(0.)
            .scroll_xy()
            .child(rows),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_read_cannot_replace_the_selected_source() {
        let mut app = Desktop::new();
        app.workspace.file_preview = Some(Preview {
            rendered: false,
            origin: Origin::Workspace(Location {
                watch: 3,
                root: "/repo".into(),
            }),
            operation: 12,
            reveal_line: None,
            path: "src/main.rs".into(),
            source: None,
            error: None,
        });
        app.receive_file_source(12, Some("/* 漢字\r\n */\r\n".into()), true, None);
        app.receive_file_source(11, None, false, Some("Stale failure".into()));
        let preview = app.workspace.file_preview.as_ref().unwrap();
        let source = preview.source.as_ref().unwrap();
        assert!(preview.error.is_none() && source.truncated);
        assert_eq!(&source.contents[source.line(0)], "/* 漢字");
        assert_eq!(&source.contents[source.line(1)], " */");
        assert_eq!(&source.contents[source.line(2)], "");
        let theme = syntax::SyntaxTheme::from_kit(&Kit::neutral(true));
        assert!(!source.spans(source.line(1), &theme).is_empty());
        let preview = app.workspace.file_preview.as_mut().unwrap();
        preview.path = "notes.md".into();
        preview.origin = Origin::Attachment(Attachment {
            watch: 3,
            key: "row".into(),
            id: "attachment".into(),
            size: "600 KB".into(),
            kind: "markdown".into(),
        });
        for text in ["a\n\n".repeat(200_000), "é中🎨".repeat(100_000)] {
            app.workspace.file_preview.as_mut().unwrap().rendered = true;
            app.receive_file_source(12, Some(text), false, None);
            let p = app.workspace.file_preview.as_ref().unwrap();
            assert!(!p.rendered);
            let source = p.source.as_ref().unwrap();
            for index in 0..source.lines.len() {
                assert!(
                    source.line(index).len() <= 4096,
                    "source row exceeds shaping budget"
                );
            }
            if !source.contents.contains('\n') {
                let reconstructed = (0..source.lines.len())
                    .map(|i| &source.contents[source.line(i)])
                    .collect::<String>();
                assert_eq!(reconstructed, source.contents);
                assert_eq!(source.line_number(0), Some(1));
                assert_eq!(source.line_number(1), None);
            }
        }
        let boundary = format!("{}\r\nend", "🎨".repeat(1024));
        let source = Source::new("note.txt", boundary.clone(), false, true);
        assert_eq!(source.contents, boundary);
        assert_eq!(source.lines.len(), 2);
        assert_eq!(source.line(0).len(), SOURCE_ROW_BYTES);
        let text = format!("{}\r\nend", "🎨".repeat(1100));
        let source = Source::new("note.txt", text, false, true);
        assert_eq!(source.line_number(1), None);
        assert_eq!(source.line_number(2), Some(2));
        assert_eq!(&source.contents[source.line(2)], "end");
    }
}
