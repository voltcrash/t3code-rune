use super::{Desktop, Request, action_with_tx, label};
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
    List { directory: String, cancel: bool },
}

pub(crate) struct Preview {
    location: Location,
    operation: u64,
    path: String,
    source: Option<Source>,
    error: Option<String>,
}

struct Source {
    contents: String,
    lines: Vec<usize>,
    highlights: Rc<[syntax::Highlight]>,
    truncated: bool,
}

impl Source {
    fn new(path: &str, contents: String, truncated: bool) -> Self {
        let lines = std::iter::once(0)
            .chain(contents.match_indices('\n').map(|(offset, _)| offset + 1))
            .collect();
        let highlights = syntax::highlight(syntax::lang_for_path(path), &contents);
        Self {
            contents,
            lines,
            highlights,
            truncated,
        }
    }

    fn line(&self, index: usize) -> Range<usize> {
        let start = self.lines[index];
        let end = self
            .lines
            .get(index + 1)
            .map_or(self.contents.len(), |n| n - 1);
        let line = &self.contents[start..end];
        start..(start + line.strip_suffix('\r').unwrap_or(line).len())
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
    pub(super) fn thread_workspace_root(&self) -> Option<&str> {
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
        if let Some(p) = self.workspace.file_preview.take() {
            self.file_request(
                p.location,
                p.operation,
                Input::Read {
                    path: p.path,
                    cancel: true,
                },
            );
        }
    }

    pub(crate) fn dismiss_file_preview(&mut self, tx: &mut rune::Tx) {
        self.close_file_preview();
        if self.workspace.explorer.is_some() {
            tx.focus_when_shown("workspace-tree", true);
        }
    }

    pub(super) fn open_workspace_file(&mut self, location: Location, path: String) {
        if self.file_location().as_ref() != Some(&location) {
            return;
        }
        self.close_diff();
        let operation = self.next_request_id();
        self.workspace.file_preview = Some(Preview {
            location: location.clone(),
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
        if let Some(p) = &self.workspace.file_preview {
            self.open_workspace_file(p.location.clone(), p.path.clone());
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
            p.source = contents.map(|text| Source::new(&p.path, text, truncated));
            p.error = error;
        }
    }

    pub(crate) fn reconcile_file_location(&mut self) {
        let changed = |location: &Location| {
            self.workspace.terminal_watch() != Some(location.watch)
                || self.new_threads.active.is_some()
                || self.thread_workspace_root() != Some(location.root.as_str())
        };
        let preview_changed = self
            .workspace
            .file_preview
            .as_ref()
            .is_some_and(|p| changed(&p.location));
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
    leading: f32,
    trailing: f32,
) -> El {
    let Some(preview) = &app.workspace.file_preview else {
        return el();
    };
    let header = row()
        .h(52.)
        .window_drag()
        .center_y()
        .pl(leading + 12.)
        .pr(trailing + 12.)
        .gap(8.)
        .child(rune::icon(rune_kit::file_icon(&preview.path), 16.).color(theme.text_secondary))
        .child(
            label(&preview.path, 13., 500., theme.text)
                .grow(1.)
                .min_w(0.)
                .truncate()
                .help(&preview.path),
        )
        .child(action_with_tx(
            "Close file",
            theme,
            true,
            Desktop::dismiss_file_preview,
        ));
    let mut panel = col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(header)
        .child(el().h(1.).w_full().fill(theme.separator));
    let Some(source) = &preview.source else {
        return panel.child(
            label(
                preview.error.as_deref().unwrap_or("Loading file…"),
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
            label("File preview truncated.", 12., 400., theme.text_secondary)
                .px(12.)
                .py(4.),
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
                        label(&(i + 1).to_string(), 12., 400., colors.text_secondary)
                            .w(gutter)
                            .pl(8.)
                            .a11y_hidden(),
                    )
                    .child(line)
            },
        )
        .virtual_rows_version(preview.operation);
    panel.child(
        col()
            .key(format!("file-source-{}", preview.operation))
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
            location: Location {
                watch: 3,
                root: "/repo".into(),
            },
            operation: 12,
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
    }
}
