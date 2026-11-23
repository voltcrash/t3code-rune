use super::{Desktop, Request, label};
use rune::{El, col, el, icon, row, tok};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub(super) title: String,
    empty: String,
    empty_description: String,
    loading: String,
    loading_description: String,
    failed: String,
    unarchive: String,
    unarchive_thread: String,
    delete: String,
    delete_thread: String,
    dates: String,
    ago: String,
    just_now: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Thread {
    id: String,
    title: String,
    archived_at: i64,
    created_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct Group {
    title: String,
    threads: Vec<Thread>,
}

#[derive(Default)]
pub(crate) struct Archived {
    operation: u64,
    groups: Option<Vec<Group>>,
    error: Option<String>,
    pending: HashSet<String>,
    pub(super) deleting: Option<Thread>,
}

impl Desktop {
    pub(crate) fn load_archived(&mut self) {
        let operation = self.next_request_id();
        self.archived.operation = operation;
        self.write_request(None, Request::ArchivedThreads { operation });
    }

    pub(crate) fn receive_archived(
        &mut self,
        operation: u64,
        groups: Vec<Group>,
        error: Option<String>,
    ) {
        if self.archived.operation == operation {
            self.archived.pending.clear();
            self.archived.groups = Some(groups);
            self.archived.error = error;
        }
    }

    /// An edit's reply lands here; a pending row then reloads to show where the thread went.
    /// Returns whether the page owned the edit, so its error shows on the page, not behind it.
    pub(crate) fn archived_edit_finished(&mut self, error: &Option<String>) -> bool {
        if self.archived.pending.is_empty() {
            return false;
        }
        self.load_archived();
        self.settings_error = error.clone();
        true
    }

    fn unarchive(&mut self, id: String) {
        let Some(watch) = self.workspace.feed else {
            return;
        };
        if self.can_operate && self.archived.pending.insert(id.clone()) {
            self.write_request(
                None,
                Request::UnarchiveThread {
                    thread: id,
                    watch,
                    operation: uuid::Uuid::new_v4().to_string(),
                },
            );
        }
    }

    fn delete_archived(&mut self, thread: Thread) {
        if self.confirm_thread_delete {
            self.archived.deleting = Some(thread);
        } else {
            self.delete_archived_now(thread.id);
        }
    }

    fn confirm_archived_delete(&mut self) {
        if let Some(thread) = self.archived.deleting.take() {
            self.delete_archived_now(thread.id);
        }
    }

    fn delete_archived_now(&mut self, id: String) {
        let Some(watch) = self.workspace.feed else {
            return;
        };
        if self.can_operate && self.archived.pending.insert(id.clone()) {
            self.write_request(
                None,
                Request::DeleteThread {
                    thread: id,
                    watch,
                    operation: uuid::Uuid::new_v4().to_string(),
                },
            );
        }
    }
}

pub(super) const ARCHIVE: rune::Icon = rune::Icon {
    name: "archive",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></g>"#,
};
const ARCHIVE_X: rune::Icon = rune::Icon {
    name: "archive-x",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="m9.5 17 5-5"/><path d="m9.5 12 5 5"/></g>"#,
};
const TRASH: rune::Icon = rune::Icon {
    name: "trash-2",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></g>"#,
};

fn age(at: i64, labels: &Labels) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let elapsed = (now - at).max(0);
    if elapsed < 60 {
        return labels.just_now.clone();
    }
    labels.ago.replace(
        "{age}",
        &crate::workspace::sidebar_status::relative_age(elapsed),
    )
}

fn section(title: &str, rows: Vec<El>, theme: &tok::Theme) -> El {
    let mut group = col()
        .w_full()
        .radius(12.)
        .clip_rounded(12.)
        .border(1., theme.separator);
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            group = group.child(el().w_full().h(1.).fill(theme.separator));
        }
        group = group.child(row);
    }
    col()
        .w_full()
        .gap(8.)
        .child(label(title, 16., 600., theme.text).pl(12.))
        .child(group)
}

fn row_copy(title: &str, description: &str, theme: &tok::Theme) -> El {
    col()
        .min_w(0.)
        .grow(1.)
        .gap(3.)
        .child(label(title, 14., 500., theme.text).truncate())
        .child(label(description, 12., 400., theme.text_secondary).wrap())
}

fn action(
    key: String,
    name: String,
    tooltip: &str,
    glyph: &'static rune::Icon,
    color: rune::Color,
    disabled: bool,
    theme: &tok::Theme,
) -> El {
    let button = el()
        .key(key)
        .role("button")
        .label(&name)
        .help(tooltip)
        .size(32., 32.)
        .radius(8.)
        .center()
        .border(1., theme.separator)
        .fill(theme.text.alpha(0.03))
        .child(icon(glyph, 14.).color(color));
    if disabled {
        button.disabled(true).opacity(0.5)
    } else {
        button.focusable(true).hover(theme.text.alpha(0.08))
    }
}

pub(super) fn view(s: &Desktop, labels: &Labels, narrow: bool, theme: &tok::Theme) -> El {
    let pad = if narrow { 12. } else { 16. };
    let Some(groups) = s.archived.groups.as_ref().filter(|g| !g.is_empty()) else {
        let (title, description) = match (&s.archived.groups, &s.archived.error) {
            (None, _) => (&labels.loading, &labels.loading_description),
            (Some(_), Some(error)) => (&labels.failed, error),
            (Some(_), None) => (&labels.empty, &labels.empty_description),
        };
        let heading = row()
            .gap(8.)
            .center_y()
            .child(icon(&ARCHIVE, 14.).color(theme.text_secondary))
            .child(label(title, 14., 500., theme.text));
        let body = col()
            .w_full()
            .padding(pad)
            .gap(3.)
            .child(heading)
            .child(label(description, 12., 400., theme.text_secondary).wrap());
        return col()
            .w_full()
            .child(section(&labels.title, vec![body], theme));
    };
    let danger = crate::confirm_dialog::destructive(theme);
    let mut page = col().w_full().gap(24.);
    for group in groups {
        let rows = group
            .threads
            .iter()
            .map(|thread| {
                let busy = s.archived.pending.contains(&thread.id) || !s.can_operate;
                let dates = labels
                    .dates
                    .replace("{archived}", &age(thread.archived_at, labels))
                    .replace("{created}", &age(thread.created_at, labels));
                let id = thread.id.clone();
                let restore = action(
                    format!("unarchive-{}", thread.id),
                    labels.unarchive_thread.replace("{title}", &thread.title),
                    &labels.unarchive,
                    &ARCHIVE_X,
                    theme.text,
                    busy,
                    theme,
                );
                let restore = if busy {
                    restore
                } else {
                    restore.on_press(move |s: &mut Desktop| s.unarchive(id.clone()))
                };
                let target = thread.clone();
                let delete = action(
                    format!("delete-archived-{}", thread.id),
                    labels.delete_thread.replace("{title}", &thread.title),
                    &labels.delete,
                    &TRASH,
                    danger,
                    busy,
                    theme,
                );
                let delete = if busy {
                    delete
                } else {
                    delete.on_press(move |s: &mut Desktop| s.delete_archived(target.clone()))
                };
                row()
                    .w_full()
                    .padding(pad)
                    .gap(12.)
                    .center_y()
                    .child(row_copy(&thread.title, &dates, theme))
                    .child(row().gap(8.).child(restore).child(delete))
            })
            .collect();
        page = page.child(section(&group.title, rows, theme));
    }
    page
}

pub(crate) fn delete_dialog(s: &Desktop, theme: &tok::Theme, overlays: &mut Vec<El>) {
    let (Some(thread), Some(labels)) = (&s.archived.deleting, &s.thread_labels) else {
        return;
    };
    let title = labels.delete_title.replace("{title}", &thread.title);
    crate::confirm_dialog::overlay(
        crate::confirm_dialog::Confirm {
            key: "delete-archived-thread",
            title: &title,
            description: Some(&labels.delete_description),
            cancel: &labels.cancel,
            confirm: &labels.confirm,
            destructive: true,
        },
        theme,
        |s| s.archived.deleting = None,
        Desktop::confirm_archived_delete,
        overlays,
    );
}
