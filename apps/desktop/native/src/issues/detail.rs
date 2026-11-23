use super::{Desktop, Labels, Request, action, label};
use rune::{El, GrowthSpec, ScrollSpec, col, el, row, tok};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reference {
    project_id: String,
    tracker: String,
    host: String,
    scope: String,
    key: String,
}

#[derive(Deserialize)]
pub(crate) struct Activity {
    id: String,
    at: i64,
    caption: String,
    body: String,
}

#[derive(Deserialize)]
struct Link {
    title: String,
    url: String,
}

#[derive(Deserialize)]
pub(crate) struct Document {
    title: String,
    key: String,
    scope: String,
    state: String,
    byline: String,
    activity_title: String,
    open: bool,
    url: Option<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    body: String,
    links: Vec<Link>,
    activity: Vec<Activity>,
    cursor: Option<String>,
}

struct Block {
    key: String,
    at: i64,
    caption: Option<String>,
    text: String,
    markdown: bool,
}

fn blocks(id: &str, at: i64, caption: Option<String>, text: String) -> Vec<Block> {
    if text.len() <= 8192 && text.lines().take(129).count() <= 128 {
        return vec![Block {
            key: format!("{id}/0"),
            at,
            caption,
            text,
            markdown: true,
        }];
    }
    let mut result = Vec::new();
    let mut remaining = text.as_str();
    let mut caption = caption;
    while !remaining.is_empty() {
        let bound = remaining.floor_char_boundary(remaining.len().min(2048));
        let end = if bound < remaining.len() {
            remaining[..bound].rfind('\n').map_or(bound, |n| n + 1)
        } else {
            bound
        };
        let (part, rest) = remaining.split_at(end);
        result.push(Block {
            key: format!("{id}/{}", result.len()),
            at,
            caption: caption.take(),
            text: part.strip_suffix('\n').unwrap_or(part).into(),
            markdown: false,
        });
        remaining = rest;
    }
    result
}

pub(super) struct Panel {
    reference: Reference,
    scroll_key: String,
    operation: u64,
    document: Option<Document>,
    description: Vec<Block>,
    activity: Vec<Block>,
    seen: HashSet<String>,
    pending: bool,
    error: Option<String>,
}

impl Panel {
    fn prepend(&mut self, entries: Vec<Activity>) {
        let mut added = Vec::new();
        for entry in entries {
            if self.seen.insert(entry.id.clone()) {
                added.extend(blocks(&entry.id, entry.at, Some(entry.caption), entry.body));
            }
        }
        added.append(&mut self.activity);
        added.sort_by_key(|block| block.at);
        self.activity = added;
    }
}

impl Desktop {
    pub(super) fn sync_issue_detail(
        &mut self,
        watch: Option<u64>,
        changed_watch: bool,
        tx: &mut rune::Tx,
    ) -> bool {
        let reference = watch.and(self.history.issue());
        let previous = self.workspace.issues.detail.as_ref();
        if !changed_watch && previous.map(|panel| &panel.reference) == reference {
            return false;
        }
        let reference = reference.cloned();
        let leaving = previous.is_some() && reference.is_none();
        self.workspace.issues.detail = reference.map(|reference| Panel {
            scroll_key: format!("issue-detail-scroll-{}", self.next_request_id()),
            reference,
            operation: 0,
            document: None,
            description: Vec::new(),
            activity: Vec::new(),
            seen: HashSet::new(),
            pending: false,
            error: None,
        });
        if self.workspace.issues.detail.is_some() {
            if !self.settings_open && self.palette.is_none() && !self.workspace.usage.open {
                tx.focus_when_shown("issue-detail-close", false);
            }
            self.request_issue_detail(watch, false);
        } else if leaving {
            if let Some(old) = self.workspace.issues.watch {
                self.write_request(
                    None,
                    Request::IssueDetail {
                        watch: old,
                        operation: 0,
                        reference: None,
                        cursor: None,
                    },
                );
            }
            if watch.is_some()
                && !self.settings_open
                && self.palette.is_none()
                && !self.workspace.usage.open
            {
                tx.focus_when_shown("issue-search", false);
            }
        }
        true
    }

    fn request_issue_detail(&mut self, watch: Option<u64>, older: bool) {
        let Some(watch) = watch else { return };
        let operation = self.next_request_id();
        let Some(panel) = &mut self.workspace.issues.detail else {
            return;
        };
        let cursor = if older {
            let Some(cursor) = panel.document.as_ref().and_then(|doc| doc.cursor.clone()) else {
                return;
            };
            Some(cursor)
        } else {
            None
        };
        panel.operation = operation;
        panel.pending = true;
        panel.error = None;
        let reference = Some(panel.reference.clone());
        self.write_request(
            None,
            Request::IssueDetail {
                watch,
                operation,
                reference,
                cursor,
            },
        );
    }

    pub(crate) fn receive_issue_detail(
        &mut self,
        watch: u64,
        operation: u64,
        doc: Option<Box<Document>>,
        older: Option<Vec<Activity>>,
        cursor: Option<String>,
        error: Option<String>,
    ) {
        if self.workspace.feed != Some(watch) {
            return;
        }
        let Some(panel) = &mut self.workspace.issues.detail else {
            return;
        };
        if panel.operation != operation {
            return;
        }
        panel.pending = false;
        panel.error = error;
        if let Some(doc) = doc {
            let mut doc = *doc;
            panel.description = blocks("description", 0, None, std::mem::take(&mut doc.body));
            panel.activity.clear();
            panel.seen.clear();
            panel.prepend(std::mem::take(&mut doc.activity));
            panel.document = Some(doc);
        } else if let Some(older) = older {
            panel.prepend(older);
            if let Some(doc) = &mut panel.document {
                doc.cursor = cursor;
            }
        }
    }

    pub(crate) fn close_issue_detail(&mut self, tx: &mut rune::Tx) -> bool {
        if self.history.issue().is_none()
            || self.pending
            || self.workspace.issues.menu.is_some()
            || self.workspace.drawer.get() == Some(true)
        {
            return false;
        }
        self.show_issue_list(tx);
        true
    }
}

fn rows(s: &Desktop, description: bool) -> &[Block] {
    s.workspace.issues.detail.as_ref().map_or(&[], |panel| {
        if description {
            &panel.description
        } else {
            &panel.activity
        }
    })
}

fn text_rows(s: &Desktop, description: bool, theme: &tok::Theme, dark: bool) -> El {
    let colors = *theme;
    col()
        .key(if description {
            "issue-body"
        } else {
            "issue-activity"
        })
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            rows(s, description).len(),
            52.,
            move |s: &Desktop, i| {
                rows(s, description)
                    .get(i)
                    .map_or("".into(), |v| v.key.as_str().into())
            },
            move |s: &Desktop, i| {
                let Some(block) = rows(s, description).get(i) else {
                    return el();
                };
                let mut item = col().w_full().min_w(0.).gap(4.);
                if !description && !block.text.is_empty() {
                    item = item
                        .border(1., colors.separator)
                        .radius(8.)
                        .padding(10.)
                        .mb(5.);
                }
                if let Some(caption) = &block.caption {
                    item = item
                        .pt(8.)
                        .child(label(caption, 12., 500., colors.text_secondary).wrap());
                }
                if !block.text.is_empty() {
                    item = item.child(if block.markdown {
                        rune_kit::Markdown::new(&block.text)
                            .selectable(true)
                            .build(&s.contrast.kit(dark))
                    } else {
                        label(&block.text, 13., 400., colors.text)
                            .wrap()
                            .selectable(true)
                    });
                }
                item
            },
        )
        .virtual_rows_version(
            s.workspace
                .issues
                .detail
                .as_ref()
                .map(|panel| (panel.operation, panel.pending)),
        )
}

pub(super) fn view(
    s: &Desktop,
    panel: &Panel,
    labels: &Labels,
    theme: &tok::Theme,
    dark: bool,
) -> El {
    let doc = panel.document.as_ref();
    let tool = |key,
                caption: &str,
                icon: &'static rune::Icon,
                enabled,
                run: fn(&mut Desktop, &mut rune::Tx)| {
        let button = el()
            .key(key)
            .size(26., 26.)
            .radius(5.)
            .center()
            .label(caption)
            .help(caption)
            .role("button")
            .child(rune::icon(icon, 14.).color(theme.text_secondary));
        if enabled {
            button.focusable(true).hover(theme.hover).on_press_tx(run)
        } else {
            button.disabled(true).opacity(0.5)
        }
    };
    let mut header = row().w_full().h(34.).center_y().gap(4.);
    if let Some(doc) = doc {
        let shade = if doc.open {
            rune::oklch(if dark { 0.85 } else { 0.55 }, 0.13, 163.)
        } else {
            theme.text_secondary
        };
        let scope = label(&doc.scope, 12., 500., theme.text_secondary)
            .truncate()
            .min_w(0.);
        header = header.child(scope);
        if let Some(url) = &doc.url {
            let url = url.clone();
            header = header.child(
                crate::workspace::action_with_tx(&doc.key, theme, true, move |_, tx| {
                    let _ = tx.open_url(&url);
                })
                .label(&labels.browser)
                .help(&labels.browser),
            );
        }
        header = header
            .child(el().grow(1.))
            .child(rune::icon(&super::ICON, 13.).color(shade))
            .child(label(&doc.state, 12., 500., theme.text).truncate());
    } else {
        header = header.child(
            label(&panel.reference.scope, 12., 500., theme.text_secondary)
                .grow(1.)
                .min_w(0.)
                .truncate(),
        );
    }
    header = header
        .child(tool(
            "issue-detail-refresh",
            &labels.detail_refresh,
            &crate::workspace::palette::REFRESH,
            !panel.pending,
            |s, _| s.request_issue_detail(s.workspace.issues.watch, false),
        ))
        .child(tool(
            "issue-detail-close",
            &labels.detail_close,
            &rune_kit::glyphs::CLOSE,
            true,
            |s, tx| {
                s.close_issue_detail(tx);
            },
        ));
    let mut body = col()
        .key(&panel.scroll_key)
        .w_full()
        .grow(1.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    let mut contents = col().w_full().px(12.).py(10.).gap(8.);
    if let Some(error) = &panel.error {
        contents = contents.child(
            label(error, 13., 400., theme.text_secondary)
                .wrap()
                .role("status"),
        );
    }
    if panel.pending {
        contents =
            contents.child(label(&labels.loading, 13., 400., theme.text_secondary).role("status"));
    }
    if let Some(doc) = doc {
        contents = contents
            .child(
                label(&doc.title, 16., 600., theme.text)
                    .wrap()
                    .selectable(true),
            )
            .child(label(&doc.byline, 12., 400., theme.text_secondary).wrap());
        for (name, values) in [
            (&labels.labels, &doc.labels),
            (&labels.assignees, &doc.assignees),
        ] {
            let value = if values.is_empty() {
                labels.none.clone()
            } else {
                values.join(", ")
            };
            contents = contents.child(
                row()
                    .w_full()
                    .center_y()
                    .gap(8.)
                    .child(
                        label(name, 12., 400., theme.text_secondary)
                            .w(92.)
                            .truncate(),
                    )
                    .child(
                        label(&value, 12., 500., theme.text)
                            .grow(1.)
                            .min_w(0.)
                            .wrap(),
                    ),
            );
        }
        contents = contents
            .child(label(&labels.description, 13., 600., theme.text))
            .child(text_rows(s, true, theme, dark));
        for link in &doc.links {
            let url = link.url.clone();
            contents = contents.child(
                label(&link.title, 13., 500., theme.text)
                    .wrap()
                    .role("link")
                    .focusable(true)
                    .hover(theme.hover)
                    .on_press_tx(move |_: &mut Desktop, tx| {
                        let _ = tx.open_url(&url);
                    }),
            );
        }
        contents =
            contents.child(label(&doc.activity_title, 13., 500., theme.text_secondary).pt(10.));
        if doc.cursor.is_some() {
            contents = contents.child(
                action(&labels.older, theme, !panel.pending, |s| {
                    s.request_issue_detail(s.workspace.issues.watch, true)
                })
                .self_start()
                .border(1., theme.card_border),
            );
        }
        contents = contents.child(text_rows(s, false, theme, dark));
    }
    body = body.child(contents);
    col()
        .grow(1.)
        .min_w(0.)
        .min_h(0.)
        .child(header.px(8.).py(6.))
        .child(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_comments_preserve_the_open_document_and_source_order_at_the_boundary() {
        let mut panel = Panel {
            reference: Reference {
                project_id: "project".into(),
                tracker: "github".into(),
                host: "github.com".into(),
                scope: "example/app".into(),
                key: "42".into(),
            },
            scroll_key: "selected".into(),
            operation: 2,
            document: None,
            description: Vec::new(),
            activity: Vec::new(),
            seen: Default::default(),
            pending: true,
            error: None,
        };
        let comment = |id: &str, text: &str| Activity {
            id: id.into(),
            at: 1,
            caption: id.into(),
            body: text.into(),
        };
        panel.prepend(vec![comment("current", "Original")]);
        let long = "λ".repeat(5000);
        panel.prepend(vec![
            comment("older", &long),
            comment("current", "Duplicate"),
        ]);
        let (last, earlier) = panel.activity.split_last().unwrap();
        assert_eq!(last.text, "Original");
        assert!(
            earlier
                .iter()
                .all(|part| !part.markdown && part.text.len() <= 2048)
        );
        assert_eq!(
            earlier
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            long
        );
        assert_eq!(
            earlier.iter().filter(|part| part.caption.is_some()).count(),
            1
        );
    }
}
