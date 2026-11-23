use super::{Desktop, MessageRow, Request, label};
use rune::{El, FileDialogOptions, TextEdit, TextStyle, Tx, col, el, row, spacer, text_input, tok};
use rune_kit::{Dialog, Kit, MarkdownStyle, Menu, MenuItem};
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
pub(super) struct Plan {
    filename: String,
    start: usize,
    preview_end: Option<usize>,
    collapsible: bool,
}

// ECMAScript trimEnd includes the BOM and preserves NEXT LINE in Markdown.
fn trim_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

impl Plan {
    fn markdown<'a>(&self, source: &'a str, closed: bool, title: Option<&str>) -> Cow<'a, str> {
        let body = trim_end(source.get(self.start..).unwrap_or(source));
        if closed {
            let preview = trim_end(
                self.preview_end
                    .and_then(|end| body.get(..end))
                    .unwrap_or(body),
            );
            if preview.is_empty() {
                return Cow::Owned(title.unwrap_or("Plan preview unavailable.").into());
            }
            if self.preview_end.is_some()
                || preview
                    .lines()
                    .any(|line| line.len() != trim_end(line).len())
            {
                let mut lines = preview.lines().map(trim_end).collect::<Vec<_>>().join("\n");
                if self.preview_end.is_some() {
                    lines.push_str("\n\n...");
                }
                return Cow::Owned(lines);
            }
        }
        Cow::Borrowed(body)
    }

    pub(super) fn card(
        &self,
        s: &Desktop,
        item: &MessageRow,
        theme: &tok::Theme,
        kit: &Kit,
        style: &MarkdownStyle,
    ) -> El {
        let pane = s.workspace.conversation.as_ref().unwrap();
        let closed = self.collapsible && !pane.expanded_activities.contains(&item.key);
        let title = item.caption.as_deref().unwrap_or("Proposed plan");
        let header = row()
            .w_full()
            .center_y()
            .gap(8.)
            .child(
                label("Plan", 12., 500., theme.text)
                    .px(8.)
                    .py(2.)
                    .radius(6.)
                    .fill(theme.text.alpha(0.08)),
            )
            .child(
                super::thread_find::plain(s, item, title, label(title, 14., 500., theme.text))
                    .truncate()
                    .min_w(0.)
                    .grow(1.),
            );
        let header = header.child(options_button(s, &item.key, theme));
        let body = super::thread_find::markdown(
            s,
            item,
            self.markdown(&item.body, closed, item.caption.as_deref())
                .as_ref(),
        )
        .selectable(true)
        .style(style.clone())
        .build(kit);
        let mut card = col()
            .w_full()
            .px(20.)
            .py(20.)
            .radius(16.)
            .border(1., theme.separator)
            .fill(theme.text.alpha(0.035))
            .gap(16.)
            .child(header)
            .child(if closed {
                col()
                    .w_full()
                    .max_h(416.)
                    .clip_fade_linear(0., 0., 0., 24.)
                    .child(body)
            } else {
                body
            });
        if self.collapsible {
            let key = item.key.clone();
            let watch = pane.watch;
            let name = if closed {
                "Expand plan"
            } else {
                "Collapse plan"
            };
            card = card.child(
                row()
                    .self_center()
                    .label(name)
                    .role("button")
                    .focusable(true)
                    .key(format!("toggle-plan-{key}"))
                    .expanded(!closed)
                    .center()
                    .h(28.)
                    .px(10.)
                    .border(1., theme.separator)
                    .radius(6.)
                    .hover(theme.hover)
                    .child(label(name, 12., 500., theme.text))
                    .on_press(move |s: &mut Desktop| {
                        if let Some(pane) = &mut s.workspace.conversation
                            && pane.watch == watch
                        {
                            if !pane.expanded_activities.remove(&key) {
                                pane.expanded_activities.insert(key.clone());
                            }
                            pane.revision += 1;
                        }
                    }),
            );
        }
        col().w_full().py(10.).child(card)
    }
}

pub(super) enum Actions {
    Menu(String),
    Save {
        key: String,
        root: String,
        path: Box<TextEdit>,
        saving: bool,
        error: Option<String>,
    },
}

fn anchor(key: &str) -> String {
    format!("plan-actions-{key}")
}

fn options_button(s: &Desktop, key: &str, theme: &tok::Theme) -> El {
    let target = key.to_owned();
    let watch = s.workspace.terminal_watch();
    el().key(format!("{}-trigger", anchor(key)))
        .label("Plan actions")
        .help("Plan actions")
        .role("button")
        .focusable(true)
        .center()
        .size(24., 24.)
        .radius(6.)
        .border(1., theme.separator)
        .hover(theme.hover)
        .child(label("⋯", 16., 500., theme.text_secondary))
        .on_press(move |s: &mut Desktop| {
            if let Some(pane) = &mut s.workspace.conversation
                && pane.watch == watch
            {
                pane.plan_actions = Some(Actions::Menu(target.clone()));
            }
        })
}

impl Desktop {
    fn dismiss_plan_actions(&mut self) {
        if let Some(pane) = &mut self.workspace.conversation
            && !matches!(pane.plan_actions, Some(Actions::Save { saving: true, .. }))
        {
            pane.plan_actions = None;
        }
    }

    fn take_plan(&mut self) -> Option<&MessageRow> {
        let pane = self.workspace.conversation.as_mut()?;
        let Actions::Menu(key) = pane.plan_actions.take()? else {
            return None;
        };
        pane.rows.iter().find(|r| r.key == key && r.plan.is_some())
    }

    fn download_plan(&mut self, tx: &mut Tx) {
        let Some(row) = self.take_plan() else { return };
        let filename = row.plan.as_ref().unwrap().filename.clone();
        let contents = format!("{}\n", trim_end(&row.body));
        tx.save_file(
            FileDialogOptions {
                title: "Download as markdown".into(),
                file_name: Some(filename),
                ..Default::default()
            }
            .filter("Markdown", &["md"]),
            move |s: &mut Desktop, result, _| match result {
                Ok(Some(path)) => {
                    s.write_request(None, Request::DownloadPlan { path, contents });
                }
                Ok(None) => {}
                Err(e) => s.error = Some(e.to_string()),
            },
        );
    }

    fn open_save_plan(&mut self, tx: &mut Tx) {
        if !self.can_write_files {
            return;
        }
        let Some(root) = self.thread_workspace_root().map(str::to_owned) else {
            return;
        };
        let Some(row) = self.take_plan() else { return };
        let key = row.key.clone();
        let path = Box::new(TextEdit::new(&row.plan.as_ref().unwrap().filename));
        self.workspace.conversation.as_mut().unwrap().plan_actions = Some(Actions::Save {
            key,
            root,
            path,
            saving: false,
            error: None,
        });
        tx.focus("plan-path", true);
    }

    fn save_plan(&mut self) {
        if !self.can_write_files {
            return;
        }
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        let Some(watch) = pane.watch else { return };
        let Some(Actions::Save {
            key,
            root,
            path,
            saving,
            error,
        }) = &mut pane.plan_actions
        else {
            return;
        };
        if *saving || path.text.trim().is_empty() {
            return;
        }
        *saving = true;
        *error = None;
        let request = Request::SavePlan {
            watch,
            key: key.clone(),
            root: root.clone(),
            path: path.text.trim().into(),
        };
        self.write_request(None, request);
    }

    pub(crate) fn finish_plan_save(&mut self, watch: u64, failure: Option<String>) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        if pane.watch != Some(watch) {
            return;
        }
        let Some(Actions::Save { saving, error, .. }) = &mut pane.plan_actions else {
            return;
        };
        if !*saving {
            return;
        }
        if failure.is_some() {
            *error = failure;
            *saving = false;
        } else {
            pane.plan_actions = None;
        }
    }
}

pub(super) fn overlay(s: &Desktop, theme: &tok::Theme, kit: &Kit, overlays: &mut Vec<El>) {
    let Some(actions) = s
        .workspace
        .conversation
        .as_ref()
        .and_then(|p| p.plan_actions.as_ref())
    else {
        return;
    };
    match actions {
        Actions::Menu(key) => {
            Menu::new(anchor(key))
                .open(true)
                .on_close(Desktop::dismiss_plan_actions)
                .item(
                    MenuItem::new("Copy to clipboard").on_select_tx(|s: &mut Desktop, tx| {
                        if let Some(row) = s.take_plan() {
                            tx.copy(format!("{}\n", trim_end(&row.body)));
                            tx.announce("Plan copied", rune::LiveRegion::Polite);
                        }
                    }),
                )
                .item(MenuItem::new("Download as markdown").on_select_tx(Desktop::download_plan))
                .item(
                    MenuItem::new("Save to workspace")
                        .disabled(!s.can_write_files || s.thread_workspace_root().is_none())
                        .on_select_tx(Desktop::open_save_plan),
                )
                .build(kit, overlays);
        }
        Actions::Save {
            root,
            path,
            saving,
            error,
            ..
        } => {
            let input = text_input(path, TextStyle::new(14., 400.))
                .key("plan-path")
                .label("Workspace path")
                .w_full()
                .h(32.)
                .single_line()
                .disabled(*saving)
                .color(theme.text)
                .on_edit(|s: &mut Desktop| {
                    let Some(Actions::Save { path, .. }) =
                        &mut s.workspace.conversation.as_mut().unwrap().plan_actions
                    else {
                        unreachable!()
                    };
                    path
                })
                .on_submit(|s: &mut Desktop, _| s.save_plan());
            let mut form = col()
                .w_full()
                .px(16.)
                .pb(16.)
                .gap(8.)
                .child(label("Workspace path", 12., 500., theme.text_secondary))
                .child(
                    col()
                        .w_full()
                        .px(8.)
                        .border(1., theme.separator)
                        .radius(6.)
                        .child(input),
                );
            if let Some(error) = error {
                form = form.child(label(error, 13., 400., theme.text_secondary).wrap())
            }
            let buttons = row()
                .w_full()
                .gap(8.)
                .child(spacer())
                .child(super::action(
                    "Cancel",
                    theme,
                    !*saving,
                    Desktop::dismiss_plan_actions,
                ))
                .child(super::action(
                    if *saving { "Saving…" } else { "Save" },
                    theme,
                    !*saving && !path.text.trim().is_empty(),
                    Desktop::save_plan,
                ));
            Dialog::new("save-plan", "Save plan to workspace")
                .description(format!("Enter a path relative to {root}."))
                .open(true)
                .width(576.)
                .fit()
                .child(form.child(buttons))
                .on_close(Desktop::dismiss_plan_actions)
                .build(kit, overlays);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanding_restores_the_original_markdown_hard_breaks() {
        let header = "# 計画\r\n\r\n";
        let first_lines = "Line one  \r\n第二步  \u{85}\r\nThird.\u{feff}\r\n\r\n";
        let source = format!("{header}{first_lines}Last step.");
        let plan = Plan {
            filename: "plan.md".into(),
            start: header.len(),
            preview_end: Some(first_lines.len()),
            collapsible: true,
        };
        assert_eq!(
            plan.markdown(&source, true, None),
            "Line one\n第二步  \u{85}\nThird.\n\n..."
        );
        assert_eq!(
            plan.markdown(&source, false, None),
            format!("{first_lines}Last step.")
        );
    }
}
