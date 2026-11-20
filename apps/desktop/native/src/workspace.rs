use super::{Desktop, Request};
use rune::{
    Cx, El, GrowthSpec, ScrollSpec, TextEdit, TextStyle, col, el, icon, row, spacer, text,
    text_input, tok,
};
use rune_kit::{Kit, Markdown, MarkdownStyle};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub(super) struct Project {
    id: String,
    title: String,
    #[serde(skip)]
    search_title: String,
    path: String,
    archived: bool,
}

#[derive(Deserialize)]
pub(super) struct Thread {
    pub id: String,
    project: String,
    title: String,
    #[serde(skip)]
    search_title: String,
    branch: Option<String>,
    status: String,
    settled: bool,
}

#[derive(Deserialize)]
pub(super) struct MessageRow {
    key: String,
    role: String,
    body: String,
    attachments: Vec<String>,
}

pub(super) struct Conversation {
    pub id: String,
    pub rows: Vec<MessageRow>,
    pub cursor: Option<String>,
    pub sequence: Option<u64>,
    pub earlier_pages: usize,
}

impl Conversation {
    fn refresh(&self) -> Request<'static> {
        Request::Thread {
            thread: self.id.clone(),
            cursor: None,
            sequence: self.sequence,
            earlier_pages: self.earlier_pages,
            oldest: if self.earlier_pages > 0 {
                self.rows.first().map(|row| row.key.clone())
            } else {
                None
            },
        }
    }
}

pub(super) struct Workspace {
    projects: Vec<Project>,
    by_project: HashMap<String, usize>,
    threads: Vec<Thread>,
    by_thread: HashMap<String, usize>,
    filtered: Vec<usize>,
    filter_revision: u64,
    settled_count: usize,
    conversation: Option<Conversation>,
    search: TextEdit,
    show_projects: bool,
    sidebar: bool,
    show_settled: bool,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            by_project: HashMap::new(),
            threads: Vec::new(),
            by_thread: HashMap::new(),
            filtered: Vec::new(),
            filter_revision: 0,
            settled_count: 0,
            conversation: None,
            search: TextEdit::new(""),
            show_projects: false,
            sidebar: true,
            show_settled: false,
        }
    }
    pub fn replace(&mut self, mut projects: Vec<Project>, mut threads: Vec<Thread>) {
        for p in &mut projects {
            p.search_title = p.title.to_lowercase();
        }
        for t in &mut threads {
            t.search_title = t.title.to_lowercase();
        }
        self.by_project = projects
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        self.by_thread = threads
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        if self
            .conversation
            .as_ref()
            .is_some_and(|p| !self.by_thread.contains_key(&p.id))
        {
            self.conversation = None;
        }
        self.settled_count = threads.iter().filter(|t| t.settled).count();
        self.projects = projects;
        self.threads = threads;
        self.filter();
    }

    fn filter(&mut self) {
        let query = self.search.text.trim().to_lowercase();
        self.filtered = self
            .threads
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let matches = if query.is_empty() {
                    self.show_settled || !t.settled
                } else {
                    t.search_title.contains(&query)
                        || self
                            .project(&t.project)
                            .is_some_and(|p| p.search_title.contains(&query))
                };
                matches.then_some(i)
            })
            .collect();
        self.filter_revision += 1;
    }
    fn project(&self, id: &str) -> Option<&Project> {
        self.by_project.get(id).map(|&i| &self.projects[i])
    }
    pub fn update_conversation(
        &mut self,
        id: String,
        mut rows: Vec<MessageRow>,
        cursor: Option<String>,
        older: bool,
        sequence: u64,
        earlier_pages: usize,
    ) {
        if let Some(pane) = &mut self.conversation
            && pane.id == id
        {
            if older {
                rows.append(&mut pane.rows);
                pane.earlier_pages += 1;
            } else {
                pane.sequence = Some(sequence);
                pane.earlier_pages = earlier_pages;
            }
            pane.rows = rows;
            pane.cursor = cursor;
        }
    }
}

impl Desktop {
    fn open_thread(&mut self, id: &str) {
        if self.pending {
            return;
        }
        self.workspace.show_projects = false;
        if self
            .workspace
            .conversation
            .as_ref()
            .is_some_and(|p| p.id == id)
        {
            return;
        }
        self.workspace.conversation = Some(Conversation {
            id: id.to_owned(),
            rows: Vec::new(),
            cursor: None,
            sequence: None,
            earlier_pages: 0,
        });
        self.refresh_thread();
    }

    fn refresh_thread(&mut self) {
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        self.send(pane.refresh());
    }

    fn older(&mut self) {
        let Some(pane) = &self.workspace.conversation else {
            return;
        };
        let Some(cursor) = pane.cursor.clone() else {
            return;
        };
        let thread = pane.id.clone();
        self.send(Request::Thread {
            thread,
            cursor: Some(cursor),
            sequence: None,
            oldest: None,
            earlier_pages: 0,
        });
    }
}

fn label(value: &str, size: f32, weight: f32, color: rune::Color) -> El {
    text(value).font(TextStyle::new(size, weight)).color(color)
}

fn action(
    caption: &str,
    theme: &tok::Theme,
    enabled: bool,
    run: impl Fn(&mut Desktop) + 'static,
) -> El {
    let base = el()
        .key(caption)
        .label(caption)
        .role("button")
        .no_window_drag()
        .h(28.0)
        .px(7.0)
        .radius(5.0)
        .center()
        .child(label(caption, 12.0, 500.0, theme.text_secondary));
    if enabled {
        base.hover(theme.separator).focusable(true).on_press(run)
    } else {
        base.disabled(true).opacity(0.5)
    }
}

fn toggle_sidebar(show: bool, theme: &tok::Theme) -> El {
    let caption = if show { "Show sidebar" } else { "Hide sidebar" };
    el().key(caption)
        .role("button")
        .label(caption)
        .help(caption)
        .no_window_drag()
        .size(28.0, 28.0)
        .center()
        .radius(5.0)
        .hover(theme.separator)
        .focusable(true)
        .child(icon(&rune_kit::shell::SIDEBAR, 16.0).color(theme.text_secondary))
        .on_press(move |s: &mut Desktop| s.workspace.sidebar = show)
}

fn project_badge(title: &str, theme: &tok::Theme) -> El {
    let initials = title
        .split_whitespace()
        .take(2)
        .filter_map(|part| part.chars().next())
        .collect::<String>()
        .to_uppercase();
    let blue = tok::accent(0, true);
    el().size(16.0, 16.0)
        .radius(4.0)
        .fill(blue.alpha(0.2))
        .center()
        .child(label(&initials, 8.0, 600.0, theme.text_secondary))
}

fn thread_row(
    thread: &Thread,
    project: &str,
    selected: bool,
    pending: bool,
    theme: &tok::Theme,
) -> El {
    let id = thread.id.clone();
    let status = match thread.status.as_str() {
        "completed" => "Done",
        "running" => "Working",
        "waiting" => "Waiting",
        "failed" => "Failed",
        "queued" => "Queued",
        "preparing" => "Preparing",
        "starting" => "Starting",
        "interrupted" => "Interrupted",
        "cancelled" => "Cancelled",
        "rolled_back" => "Rolled back",
        _ => "",
    };
    let heading = row()
        .w_full()
        .center_y()
        .gap(6.0)
        .child(project_badge(project, theme))
        .child(
            label(project, 12.0, 400.0, theme.text_secondary)
                .truncate()
                .shrink(),
        )
        .child(spacer())
        .child(label(
            status,
            12.0,
            500.0,
            if status == "Done" {
                rune::oklch(0.75, 0.14, 165.0)
            } else {
                theme.text_secondary
            },
        ));
    let mut base = col()
        .key(&id)
        .label(&thread.title)
        .role("button")
        .radius(6.0)
        .w_full()
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .child(heading)
        .child(label(&thread.title, 14.0, 500.0, theme.text).truncate());
    if let Some(branch) = &thread.branch {
        base = base.child(label(branch, 12.0, 400.0, theme.text_secondary).truncate());
    }
    if selected {
        base = base.fill(theme.separator);
    }
    if pending {
        base.disabled(true)
    } else {
        base.hover(theme.separator)
            .focusable(true)
            .on_press(move |s: &mut Desktop| s.open_thread(&id))
    }
}

fn sidebar(s: &Desktop, inset: f32, theme: &tok::Theme) -> El {
    let search = text_input(&s.workspace.search, TextStyle::new(13.0, 400.0))
        .key("thread-search")
        .label("Search threads")
        .w_full()
        .h(34.0)
        .px(10.0)
        .color(theme.text)
        .placeholder("Search", theme.text_secondary)
        .single_line()
        .on_edit(|s: &mut Desktop| &mut s.workspace.search)
        .on_input_change(|s: &mut Desktop, _| s.workspace.filter());
    let query_empty = s.workspace.search.text.trim().is_empty();
    let count = s.workspace.filtered.len();
    let colors = *theme;
    let rows = col()
        .key("thread-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            76.0,
            move |s: &Desktop, i| {
                s.workspace.threads[s.workspace.filtered[i]]
                    .id
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let t = &s.workspace.threads[s.workspace.filtered[i]];
                let project = s
                    .workspace
                    .project(&t.project)
                    .map_or("", |p| p.title.as_str());
                let selected = !s.workspace.show_projects
                    && s.workspace
                        .conversation
                        .as_ref()
                        .is_some_and(|p| p.id == t.id);
                thread_row(t, project, selected, s.pending, &colors)
            },
        )
        .virtual_rows_version(s.workspace.filter_revision);
    let list = col()
        .key("thread-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(if count == 0 {
            label(
                if query_empty {
                    "No threads"
                } else {
                    "No matching threads"
                },
                13.0,
                400.0,
                theme.text_secondary,
            )
            .padding(10.0)
        } else {
            rows
        });
    let settled = s.workspace.settled_count;
    let mut sidebar = col()
        .key("sidebar")
        .w(256.0)
        .max_w(256.0)
        .h_full()
        .px(8.0)
        .pb(8.0)
        .child(
            row()
                .h(52.0)
                .w_full()
                .pl(inset)
                .center_y()
                .window_drag()
                .gap(4.0)
                .child(toggle_sidebar(false, theme))
                .child(label("T3 Code", 14.0, 600.0, theme.text)),
        )
        .child(
            row()
                .h(48.0)
                .w_full()
                .center_y()
                .px(7.0)
                .gap(4.0)
                .child(icon(&rune_kit::glyphs::SEARCH, 16.0).color(theme.text_secondary))
                .child(search.shrink()),
        )
        .child(list);
    if settled > 0 && query_empty {
        sidebar = sidebar.child(
            action(&format!("Settled  {settled}"), theme, true, |s| {
                s.workspace.show_settled = !s.workspace.show_settled;
                s.workspace.filter();
            })
            .self_start(),
        );
    }
    sidebar.child(
        row()
            .w_full()
            .child(action("Projects", theme, true, |s| {
                s.workspace.show_projects = true
            }))
            .child(spacer())
            .child(action("Disconnect", theme, !s.pending, |s| {
                s.send(Request::Disconnect)
            })),
    )
}

fn projects_view(s: &Desktop, theme: &tok::Theme) -> El {
    let colors = *theme;
    let list = col()
        .key("project-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.workspace.projects.len(),
            65.0,
            |s: &Desktop, i| s.workspace.projects[i].id.as_str().into(),
            move |s: &Desktop, i| {
                let p = &s.workspace.projects[i];
                let theme = &colors;
                let mut title = row()
                    .gap(8.0)
                    .child(label(&p.title, 14.0, 600.0, theme.text));
                if p.archived {
                    title = title.child(label("Archived", 12.0, 400.0, theme.text_secondary));
                }
                col()
                    .key(&p.id)
                    .w_full()
                    .py(9.0)
                    .gap(3.0)
                    .child(title)
                    .child(label(&p.path, 13.0, 400.0, theme.text_secondary).truncate_middle())
            },
        );
    let mut area = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if s.workspace.projects.is_empty() && !s.pending {
        area = area.child(label("No projects", 14.0, 400.0, theme.text_secondary));
    }
    if let Some(environment) = &s.environment {
        area = area.child(label(environment, 13.0, 400.0, theme.text_secondary).py(8.0));
    }
    area.child(list)
}

fn timeline(s: &Desktop, pane: &Conversation, theme: &tok::Theme, dark: bool) -> El {
    let kit = Kit::neutral(dark);
    let style = MarkdownStyle::from_kit(&kit)
        .body_font(TextStyle::new(14.0, 400.0).lh(22.0))
        .text(theme.text)
        .table_full_width(true)
        .table_flush_left(true)
        .table_rules(theme.separator)
        .table_header_fill(rune::Color::TRANSPARENT)
        .table_outline(None)
        .table_cell_padding(10.0, 8.0);
    let mut content = col().w_full().max_w(736.0).self_center();
    if pane.cursor.is_some() {
        content = content.child(action(
            "Load earlier messages",
            theme,
            !s.pending,
            Desktop::older,
        ));
    }
    if pane.sequence.is_none() && !s.pending {
        content = content.child(action(
            "Retry conversation",
            theme,
            true,
            Desktop::refresh_thread,
        ));
    }
    if pane.sequence.is_some() && pane.rows.is_empty() {
        content =
            content.child(label("No messages", 14.0, 400.0, theme.text_secondary).key("empty"));
    }
    let colors = *theme;
    let list = col()
        .key("message-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            pane.rows.len(),
            100.0,
            |s: &Desktop, i| {
                s.workspace.conversation.as_ref().unwrap().rows[i]
                    .key
                    .as_str()
                    .into()
            },
            move |s: &Desktop, i| {
                let theme = &colors;
                let message = &s.workspace.conversation.as_ref().unwrap().rows[i];
                let mut content = col().gap(6.0);
                if message.role == "user" {
                    content = content
                        .max_w(640.0)
                        .self_end()
                        .px(14.0)
                        .py(9.0)
                        .radius(14.0)
                        .fill(theme.text.alpha(0.08))
                        .child(
                            label(&message.body, 14.0, 400.0, theme.text)
                                .wrap()
                                .selectable(true),
                        );
                } else {
                    content = content.w_full().child(
                        Markdown::new(&message.body)
                            .style(style.clone())
                            .selectable(true)
                            .build(&kit),
                    );
                }
                for file in &message.attachments {
                    content = content.child(label(file, 12.0, 500.0, theme.text_secondary));
                }
                col().w_full().py(10.0).child(content)
            },
        );
    col()
        .key(format!("timeline-{}", pane.id))
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(24.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(content.child(list))
}

pub(super) fn view(s: &Desktop, cx: &mut Cx<Desktop>, theme: &tok::Theme) -> El {
    let thread = s.workspace.conversation.as_ref().and_then(|p| {
        s.workspace
            .by_thread
            .get(&p.id)
            .map(|&i| &s.workspace.threads[i])
    });
    let inset = cx.chrome_insets();
    let mut header = row()
        .key("thread-titlebar")
        .w_full()
        .h(52.0)
        .window_drag()
        .center_y()
        .gap(8.0)
        .px(16.0);
    if !s.workspace.sidebar {
        header = header
            .pl(inset.leading + 12.0)
            .child(action("Show sidebar", theme, true, |s| {
                s.workspace.sidebar = true
            }));
    }
    if !s.workspace.show_projects
        && let Some(thread) = thread
    {
        if let Some(project) = s.workspace.project(&thread.project) {
            header = header
                .child(project_badge(&project.title, theme))
                .child(label(&project.title, 13.0, 400.0, theme.text_secondary).truncate())
                .child(label("/", 13.0, 400.0, theme.text_secondary));
        }
        header = header.child(
            label(&thread.title, 14.0, 500.0, theme.text)
                .truncate()
                .shrink(),
        );
    } else {
        header = header.child(label("Projects", 14.0, 500.0, theme.text));
    }
    header = header.child(spacer());
    if s.pending {
        header = header.child(label("Loading…", 12.0, 400.0, theme.text_secondary));
    }
    header = header
        .pr(inset.trailing + 12.0)
        .child(action("Refresh", theme, !s.pending, |s| {
            if s.workspace.conversation.is_some() && !s.workspace.show_projects {
                s.refresh_thread();
            } else {
                s.send(Request::Refresh);
            }
        }));
    let mut main = col().grow(1.0).min_w(0.0).h_full().child(header);
    if let Some(err) = &s.error {
        main = main.child(
            label(err, 13.0, 400.0, theme.text_secondary)
                .wrap()
                .px(16.0),
        );
    }
    main = main.child(
        if !s.workspace.show_projects
            && let Some(pane) = &s.workspace.conversation
        {
            timeline(s, pane, theme, cx.system_dark())
        } else {
            projects_view(s, theme)
        },
    );
    let mut root = row().w_full().h_full().fill(theme.bg);
    if s.workspace.sidebar {
        root = root
            .child(sidebar(s, inset.leading, theme))
            .child(el().w(1.0).h_full().fill(theme.separator));
    }
    root.child(main)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_expanded_history_requests_the_oldest_loaded_row() {
        let mut desktop = Desktop::new();
        desktop.open_thread("thread");
        let workspace = &mut desktop.workspace;
        let message = |key: &str| MessageRow {
            key: key.into(),
            role: "user".into(),
            body: key.into(),
            attachments: Vec::new(),
        };
        workspace.update_conversation(
            "thread".into(),
            vec![message("latest")],
            Some("previous".into()),
            false,
            10,
            0,
        );
        workspace.update_conversation("thread".into(), vec![message("earlier")], None, true, 11, 1);
        desktop.open_thread("thread");
        let pane = desktop.workspace.conversation.as_ref().unwrap();
        let Request::Thread {
            cursor,
            sequence,
            oldest,
            earlier_pages,
            ..
        } = pane.refresh()
        else {
            panic!("expected thread refresh")
        };
        assert_eq!(sequence, Some(10));
        assert_eq!(earlier_pages, 1);
        assert_eq!(oldest.as_deref(), Some("earlier"));
        assert!(cursor.is_none());
        assert_eq!(pane.rows.len(), 2);
    }
}
