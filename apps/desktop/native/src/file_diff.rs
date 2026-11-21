use super::{Desktop, MessageRow, Request, action, label};
use rune::{El, ScrollSpec, TextStyle, col, row, text, tok};
use rune_kit::Kit;
use rune_kit::diff::{DiffMode, DiffState, DiffView, FileDiff, FileStatus, LineKind, parse};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Row {
    pub added: Option<String>,
    pub removed: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Side {
    Before,
    After,
}
#[derive(Deserialize)]
struct Content {
    side: Side,
    text: String,
}

#[derive(Default, Deserialize)]
pub(crate) struct Recorded {
    patch: Option<String>,
    #[serde(default)]
    content: Vec<Content>,
    message: Option<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    count: usize,
}

struct Selection {
    watch: u64,
    operation: String,
}

#[derive(Default)]
pub(crate) struct Panel {
    selection: Option<Selection>,
    state: DiffState,
    recorded: Recorded,
}

impl Panel {
    pub(crate) fn visible(&self, app: &Desktop) -> bool {
        self.selection.as_ref().is_some_and(|s| {
            !app.workspace.show_projects
                && app.new_threads.active.is_none()
                && app.workspace.conversation.as_ref().and_then(|p| p.watch) == Some(s.watch)
        })
    }

    pub(crate) fn receive(&mut self, operation: &str, mut recorded: Recorded) {
        if !self
            .selection
            .as_ref()
            .is_some_and(|s| s.operation == operation)
        {
            return;
        }
        let files = recorded
            .patch
            .as_deref()
            .map(renderable_files)
            .unwrap_or_default();
        if !files.is_empty() {
            recorded.patch = None;
        }
        self.state.set_files(files);
        self.recorded = recorded;
    }
}

// Rune keeps the lines it could read and reduces each hunk's counts to match.
fn header_counts(line: &str) -> Option<(u32, u32)> {
    let (ranges, _) = line.strip_prefix("@@ -")?.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let count = |range: &str| {
        range
            .split_once(',')
            .map_or(Some(1), |(_, n)| n.parse().ok())
    };
    Some((count(old)?, count(new)?))
}

fn hunks_match(patch: &str, files: &[FileDiff]) -> bool {
    let mut hunks = files.iter().flat_map(|f| &f.hunks);
    let mut lines = patch
        .lines()
        .filter(|line| *line != "\\ No newline at end of file");
    let mut after_hunk = false;
    while let Some(line) = lines.next() {
        if line.starts_with("@@") {
            let Some(hunk) = hunks.next() else {
                return false;
            };
            if header_counts(line) != Some((hunk.old_count, hunk.new_count)) {
                return false;
            }
            for body in &hunk.lines {
                let Some(raw) = lines.next() else {
                    return false;
                };
                let prefix = match body.kind {
                    LineKind::Context => ' ',
                    LineKind::Added => '+',
                    LineKind::Removed => '-',
                };
                if !(raw.strip_prefix(prefix) == Some(&body.text)
                    || prefix == ' ' && raw.is_empty() && body.text.is_empty())
                {
                    return false;
                }
            }
            after_hunk = true;
        } else if line.starts_with("diff --git ")
            || (line.starts_with("--- ")
                && lines.clone().next().is_some_and(|s| s.starts_with("+++ "))
                && lines.clone().nth(1).is_some_and(|s| s.starts_with("@@ ")))
        {
            after_hunk = false;
        } else if after_hunk && !line.is_empty() {
            return false;
        }
    }
    hunks.next().is_none()
}

fn renderable_files(patch: &str) -> Vec<FileDiff> {
    let files = parse(patch);
    if hunks_match(patch, &files)
        && files.iter().all(|file| {
            !file.path().is_empty()
                && (!file.hunks.is_empty()
                    || file.binary
                    || file.status != FileStatus::Modified
                    || file.old_mode != file.new_mode)
        })
    {
        files
    } else {
        Vec::new()
    }
}

impl Desktop {
    pub(crate) fn close_diff(&mut self) {
        if let Some(selected) = self.workspace.diff.selection.take() {
            self.workspace.diff = Panel::default();
            self.write_request(
                None,
                Request::InspectFile {
                    watch: selected.watch,
                    operation: selected.operation,
                    key: None,
                },
            );
        }
    }

    fn open_diff(&mut self, watch: u64, key: &str) {
        if self.pending || self.workspace.conversation.as_ref().and_then(|p| p.watch) != Some(watch)
        {
            return;
        }
        self.close_files_panel();
        let operation = Uuid::new_v4().to_string();
        self.workspace.diff = Panel {
            selection: Some(Selection {
                watch,
                operation: operation.clone(),
            }),
            recorded: Recorded {
                message: Some("Loading diff…".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        self.write_request(
            None,
            Request::InspectFile {
                watch,
                operation,
                key: Some(key.to_owned()),
            },
        );
    }
}

pub(super) fn details(
    app: &Desktop,
    item: &MessageRow,
    file: &Row,
    theme: &tok::Theme,
    kit: &Kit,
) -> El {
    let mut bar = row().center_y().gap(6.);
    if let (Some(added), Some(removed)) = (&file.added, &file.removed) {
        bar = bar
            .child(label(&format!("+{added}"), 12., 400., kit.c.success.fg))
            .child(label(&format!("-{removed}"), 12., 400., kit.c.danger.fg));
    }
    let watch = app.workspace.conversation.as_ref().and_then(|p| p.watch);
    let key = item.key.clone();
    bar = bar.child(
        action(
            "Open diff",
            theme,
            watch.is_some() && !app.pending,
            move |app| {
                if let Some(watch) = watch {
                    app.open_diff(watch, &key);
                }
            },
        )
        .key(format!("open-diff-{}", item.key)),
    );
    col().gap(4.).child(source(&item.body, theme)).child(bar)
}

fn source(value: &str, theme: &tok::Theme) -> El {
    text(value)
        .font(TextStyle::new(12., 400.).mono().lh(18.))
        .color(theme.text_secondary)
        .wrap_anywhere()
        .selectable(true)
}

pub(super) fn view(
    app: &Desktop,
    theme: &tok::Theme,
    kit: &Kit,
    leading: f32,
    trailing: f32,
) -> El {
    let panel = &app.workspace.diff;
    let files = panel.state.files();
    let mut header = row()
        .h(52.)
        .window_drag()
        .center_y()
        .pl(leading + 12.)
        .pr(trailing + 12.)
        .gap(4.)
        .child(label("Diff", 14., 500., theme.text).grow(1.));
    if !files.is_empty() {
        let (caption, next) = if panel.state.mode() == DiffMode::Unified {
            ("Split diff view", DiffMode::Split)
        } else {
            ("Unified diff view", DiffMode::Unified)
        };
        header = header.child(action(caption, theme, true, move |app| {
            app.workspace.diff.state.set_mode(next)
        }));
    }
    header = header.child(action("Close diff", theme, true, Desktop::close_diff));
    let mut view = col().grow(1.).min_w(0.).h_full().child(header);
    let recorded = &panel.recorded;
    if recorded.truncated {
        view = view.child(
            label(
                "Recorded diff was truncated.",
                12.,
                400.,
                theme.text_secondary,
            )
            .px(12.),
        );
    }
    if !files.is_empty() {
        if let Some(omitted) = recorded.count.checked_sub(files.len()).filter(|n| *n > 0) {
            view = view.child(
                label(
                    &format!("Files without a recorded patch: {omitted}"),
                    12.,
                    400.,
                    theme.text_secondary,
                )
                .px(12.),
            );
        }
        return view.child(
            DiffView::new(
                |s: &Desktop| &s.workspace.diff.state,
                |s: &mut Desktop| &mut s.workspace.diff.state,
            )
            .key("recorded-file-diff")
            .build(app, kit),
        );
    }
    let mut body = col().grow(1.).min_h(0.).px(12.).gap(8.).scroll(ScrollSpec {
        stick_bottom: false,
    });
    if let Some(message) = &recorded.message {
        body = body.child(label(message, 13., 400., theme.text_secondary).wrap());
    } else if let Some(raw) = recorded.patch.as_deref().filter(|s| !s.trim().is_empty()) {
        body = body
            .child(label(
                "Unsupported diff format. Showing raw patch.",
                13.,
                400.,
                theme.text_secondary,
            ))
            .child(source(raw, theme));
    } else if recorded.content.is_empty() {
        body = body.child(
            label(
                "No diff was recorded for this file change.",
                13.,
                400.,
                theme.text_secondary,
            )
            .wrap(),
        );
    } else {
        for block in &recorded.content {
            let title = match block.side {
                Side::Before => "Recorded content before",
                Side::After => "Recorded content after",
            };
            body = body
                .child(label(title, 13., 500., theme.text))
                .child(source(&block.text, theme));
        }
    }
    view.child(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_result_preserves_the_selected_patch_and_raw_fallback() {
        let mut panel = Panel {
            selection: Some(Selection {
                watch: 7,
                operation: "new".into(),
            }),
            ..Default::default()
        };
        let patch = |text: &str| Recorded {
            patch: Some(text.into()),
            ..Default::default()
        };
        panel.receive(
            "new",
            patch("--- a/日本語.ts\n+++ b/日本語.ts\n@@ -1 +1 @@\n-old\n+new\n"),
        );
        panel.receive("old", patch("unrelated"));
        let files = panel.state.files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path(), "日本語.ts");
        assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
        assert!(panel.recorded.patch.is_none());

        for hunk in [
            "@@ -1 +oops @@\n-a\n+b",
            "@@ -1 +1 @@\nunexpected",
            "@@ -2,3 +2,3 @@\n one line",
            "@@ -1 +1 @@\n-old\n+new\n+extra",
        ] {
            let raw = format!(
                "diff --git a/src/x b/src/x\nindex 4a..5b 100644\n--- a/src/x\n+++ b/src/x\n{hunk}"
            );
            panel.receive("new", patch(&raw));
            assert!(panel.state.files().is_empty(), "{hunk}");
            assert_eq!(panel.recorded.patch.as_deref(), Some(raw.as_str()));
        }
        panel.receive(
            "new",
            patch("diff --git a/src/x b/src/x\nold mode 100644\nnew mode 100755\n"),
        );
        assert_eq!(panel.state.files().len(), 1);
        assert!(panel.recorded.patch.is_none());
    }
}
