mod detail;
mod markdown;
use super::{
    Desktop, Request,
    navigation::Page,
    workspace::{action, label},
};
pub(crate) use detail::{Activity, Document, Reference};
use rune::{
    El, GrowthSpec, Icon, ScrollSpec, TextEdit, TextStyle, col, el, icon, row, text_input, tok,
};
use serde::{Deserialize, Serialize};

pub(crate) const ICON: Icon = Icon {
    name: "issue-circle-dot",
    view: 24.,
    body: r#"<circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="12" r="2" fill="currentColor"/>"#,
};

const COMPLETED: Icon = Icon {
    name: "issue-completed",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/></g>"#,
};
const CLOSED: Icon = Icon {
    name: "issue-closed",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="m6 18 12-12"/></g>"#,
};

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub title: String,
    detail_close: String,
    detail_refresh: String,
    older: String,
    browser: String,
    description: String,
    labels: String,
    none: String,
    assignees: String,
    search: String,
    open: String,
    closed: String,
    all: String,
    refresh: String,
    sort: String,
    filters: String,
    newest: String,
    updated: String,
    pub back: String,
    unconfigured: String,
    add_project: String,
    more: String,
    loading: String,
    empty: String,
    no_match: String,
    tracker: String,
    ago: String,
    just_now: String,
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Filter {
    #[default]
    Open,
    Closed,
    All,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Open,
    Completed,
    Closed,
}

#[derive(Deserialize)]
struct Issue {
    identity: String,
    reference: Reference,
    title: String,
    key: String,
    author: String,
    scope: Option<String>,
    comments: u64,
    updated_at: i64,
    status: Status,
}

#[derive(Deserialize)]
pub(crate) struct TrackerIcon {
    name: String,
    view: f32,
    body: String,
}

#[derive(Deserialize)]
struct Group {
    key: String,
    name: String,
    host: Option<String>,
    projects: String,
    icon: Option<TrackerIcon>,
    notice: Option<String>,
    rows: Vec<Issue>,
}

#[derive(Default, Deserialize)]
pub(crate) struct List {
    groups: Vec<Group>,
    #[serde(default)]
    hosts: Vec<crate::pull_requests::HostOption>,
    #[serde(default)]
    errors: Vec<String>,
    more: bool,
    has_trackers: bool,
    #[serde(default)]
    searching: bool,
}

/// One drawn line of the list, in the web's order: a tracker heading, its notes, its issues.
#[derive(Clone, Copy)]
enum Line {
    Heading(usize),
    Notice(usize),
    Error(usize),
    Empty(usize),
    Row(usize, usize),
}

impl List {
    fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            lines.push(Line::Heading(g));
            if group.notice.is_some() {
                lines.push(Line::Notice(g));
            }
            if group.notice.is_none() && group.rows.is_empty() {
                lines.push(Line::Empty(g));
            }
            lines.extend((0..group.rows.len()).map(|r| Line::Row(g, r)));
        }
        lines.extend((0..self.errors.len()).map(Line::Error));
        lines
    }
}

/// Rune icons are static, so each tracker's mark is leaked once and reused.
pub(crate) fn tracker_icon(icon: &TrackerIcon) -> &'static Icon {
    thread_local! {
        static ICONS: std::cell::RefCell<std::collections::HashMap<String, &'static Icon>> =
            Default::default();
    }
    ICONS.with(|icons| {
        *icons
            .borrow_mut()
            .entry(icon.name.clone())
            .or_insert_with(|| {
                Box::leak(Box::new(Icon {
                    name: Box::leak(icon.name.clone().into_boxed_str()),
                    view: icon.view,
                    body: Box::leak(icon.body.clone().into_boxed_str()),
                }))
            })
    })
}

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Sort,
    Filter,
    Host,
}

#[derive(Default)]
pub(crate) struct Issues {
    pub labels: Option<Labels>,
    detail: Option<detail::Panel>,
    watch: Option<u64>,
    query: TextEdit,
    filter: Filter,
    newest: bool,
    host: String,
    hosts: Vec<crate::pull_requests::HostOption>,
    pub(crate) views: crate::saved_views::Views<ViewPrefs>,
    menu: std::cell::Cell<Option<MenuKind>>,
    operation: u64,
    pending: bool,
    list: List,
    lines: Vec<Line>,
    error: Option<String>,
}

impl Desktop {
    pub(crate) fn show_issue_list(&mut self, tx: &mut rune::Tx) {
        self.open_issues();
        self.sync_issues(tx);
    }

    pub(crate) fn open_issues(&mut self) {
        if !self.pending && self.workspace.issues.labels.is_some() {
            self.open_index_page(Page::Issues);
        }
    }

    pub(crate) fn sync_issues(&mut self, tx: &mut rune::Tx) -> bool {
        let active = self.history.on_issues() && self.workspace.issues.labels.is_some();
        let watch = self.workspace.feed.filter(|_| active);
        let changed_watch = watch != self.workspace.issues.watch;
        let detail_changed = self.sync_issue_detail(watch, changed_watch, tx);
        if !changed_watch {
            return detail_changed;
        }
        let entering = self.workspace.issues.watch.is_none() && watch.is_some();
        if let Some(old) = self.workspace.issues.watch {
            self.write_request(
                None,
                Request::Issues {
                    watch: old,
                    operation: 0,
                    action: "close",
                    state: Filter::Open,
                    sort: "updated",
                    query: String::new(),
                    host: String::new(),
                },
            );
        }
        self.workspace.issues.watch = watch;
        self.workspace.issues.menu.set(None);
        self.workspace.issues.list = Default::default();
        self.workspace.issues.lines.clear();
        self.workspace.issues.error = None;
        self.workspace.issues.pending = false;
        if entering
            && self.history.issue().is_none()
            && !self.settings_open
            && self.palette.is_none()
            && !self.workspace.usage.open
        {
            tx.focus_when_shown("issue-search", false);
        }
        if watch.is_some() {
            self.load_issues("list");
        }
        true
    }

    fn load_issues(&mut self, action: &'static str) {
        let Some(watch) = self.workspace.issues.watch else {
            return;
        };
        let operation = self.next_request_id();
        let page = &mut self.workspace.issues;
        let state = page.filter;
        let sort = if page.newest { "created" } else { "updated" };
        let query = page.query.text.trim().to_string();
        let host = page.host.clone();
        if matches!(action, "list" | "search") {
            page.list = Default::default();
            page.lines.clear();
        }
        page.operation = operation;
        page.pending = true;
        page.error = None;
        self.write_request(
            None,
            Request::Issues {
                watch,
                operation,
                action,
                state,
                sort,
                query,
                host,
            },
        );
    }

    pub(crate) fn receive_issues(
        &mut self,
        watch: u64,
        operation: u64,
        list: Option<List>,
        error: Option<String>,
    ) {
        let page = &mut self.workspace.issues;
        if page.watch == Some(watch) && page.operation == operation {
            if let Some(mut result) = list {
                page.lines = result.lines();
                page.hosts = std::mem::take(&mut result.hosts);
                page.list = result;
            }
            page.error = error;
            page.pending = false;
        }
    }
}

/// What a saved issue view keeps: the list's search, state, sort and tracker.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ViewPrefs {
    state: Filter,
    newest: bool,
    host: String,
    query: String,
}

pub(crate) struct SavedIssues;

impl crate::saved_views::SavedList for SavedIssues {
    type Prefs = ViewPrefs;
    const LIST: &'static str = "issues";
    const KEY: &'static str = "issue-views";
    fn views(s: &Desktop) -> &crate::saved_views::Views<ViewPrefs> {
        &s.workspace.issues.views
    }
    fn views_mut(s: &mut Desktop) -> &mut crate::saved_views::Views<ViewPrefs> {
        &mut s.workspace.issues.views
    }
    fn current(s: &Desktop) -> ViewPrefs {
        let page = &s.workspace.issues;
        ViewPrefs {
            state: page.filter,
            newest: page.newest,
            host: page.host.clone(),
            query: page.query.text.trim().to_owned(),
        }
    }
    fn apply(s: &mut Desktop, prefs: ViewPrefs) {
        let page = &mut s.workspace.issues;
        page.filter = prefs.state;
        page.newest = prefs.newest;
        page.host = prefs.host;
        page.query = TextEdit::new(&prefs.query);
        s.load_issues("list");
    }
    fn labels(labels: &crate::saved_views::Labels) -> &crate::saved_views::ListLabels {
        labels.issues()
    }
    fn suggested(_: &Desktop, _: &crate::saved_views::ListLabels) -> String {
        String::new()
    }
}

pub(crate) fn trigger(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let name = &s.workspace.issues.labels.as_ref()?.title;
    let button = crate::workspace::footer_button("sidebar-issues", name, &ICON, theme, !s.pending);
    Some(if s.pending {
        button
    } else {
        button.on_press_tx(Desktop::show_issue_list)
    })
}

fn menu(s: &Desktop, kind: MenuKind, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) -> El {
    use rune_kit::{Menu, MenuItem};
    let page = &s.workspace.issues;
    let Some(labels) = &page.labels else {
        return el();
    };
    let (key, caption, glyph) = match kind {
        MenuKind::Host => {
            return crate::pull_requests::host_filter(
                "issue-host",
                &labels.tracker,
                &labels.all,
                &page.host,
                &page.hosts,
                page.menu.get() == Some(MenuKind::Host),
                |s| {
                    let menu = &s.workspace.issues.menu;
                    menu.set((menu.get() != Some(MenuKind::Host)).then_some(MenuKind::Host));
                },
                |s| s.workspace.issues.menu.set(None),
                |s, host| {
                    let page = &mut s.workspace.issues;
                    page.menu.set(None);
                    if page.host != host {
                        page.host = host;
                        s.load_issues("list");
                    }
                },
                theme,
                &s.contrast.kit(dark),
                overlays,
            );
        }
        MenuKind::Sort => ("issue-sort", &labels.sort, &crate::pull_requests::SORT),
        MenuKind::Filter => (
            "issue-filters",
            &labels.filters,
            &crate::pull_requests::FILTER,
        ),
    };
    let trigger =
        crate::pull_requests::toolbar_button(&format!("{key}-trigger"), caption, glyph, theme)
            .focusable(true)
            .hover(theme.hover)
            .on_press(move |s: &mut Desktop| {
                let menu = &s.workspace.issues.menu;
                menu.set(if menu.get() == Some(kind) {
                    None
                } else {
                    Some(kind)
                });
            });
    let open = page.menu.get() == Some(kind);
    let mut menu = Menu::new(key)
        .trigger(trigger)
        .open(open)
        .on_close(|s: &mut Desktop| s.workspace.issues.menu.set(None));
    if open {
        match kind {
            MenuKind::Host => {}
            MenuKind::Filter => {
                for (value, name) in [
                    (Filter::Open, &labels.open),
                    (Filter::Closed, &labels.closed),
                    (Filter::All, &labels.all),
                ] {
                    menu = menu.item(MenuItem::new(name).checked(page.filter == value).on_select(
                        move |s: &mut Desktop| {
                            s.workspace.issues.menu.set(None);
                            if s.workspace.issues.filter != value {
                                s.workspace.issues.filter = value;
                                s.load_issues("list");
                            }
                        },
                    ));
                }
            }
            MenuKind::Sort => {
                for (newest, name) in [(false, &labels.updated), (true, &labels.newest)] {
                    menu = menu.item(
                        MenuItem::new(name)
                            .checked(page.newest == newest)
                            .on_select(move |s: &mut Desktop| {
                                s.workspace.issues.menu.set(None);
                                if s.workspace.issues.newest != newest {
                                    s.workspace.issues.newest = newest;
                                    s.load_issues("list");
                                }
                            }),
                    );
                }
            }
        }
    }
    menu.build(&s.contrast.kit(dark), overlays)
}

fn heading(group: &Group, theme: &tok::Theme, spaced: bool) -> El {
    let muted = theme.text_secondary;
    let mut line = row()
        .w_full()
        .px(12.)
        .pt(if spaced { 24. } else { 0. })
        .pb(4.)
        .gap(6.)
        .center_y();
    if let Some(mark) = &group.icon {
        line = line.child(icon(tracker_icon(mark), 14.).color(muted));
    }
    line = line.child(label(&group.name, 12., 500., muted));
    if let Some(host) = &group.host {
        line = line.child(label(&format!("· {host}"), 12., 400., muted));
    }
    line.child(label(&format!("· {}", group.projects), 12., 400., muted))
}

fn issue_row(s: &Desktop, issue: &Issue, labels: &Labels, theme: &tok::Theme, dark: bool) -> El {
    let (mark, ink) = match issue.status {
        Status::Open => (
            &ICON,
            rune::oklch(if dark { 0.85 } else { 0.60 }, 0.15, 163.),
        ),
        Status::Completed => (
            &COMPLETED,
            rune::oklch(if dark { 0.81 } else { 0.54 }, 0.19, 293.),
        ),
        Status::Closed => (&CLOSED, theme.text_secondary),
    };
    let small = |value: &str| label(value, 11., 400., theme.text_secondary);
    let mut first = row()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .center_y()
        .child(
            rune::text(issue.key.clone())
                .font(TextStyle::new(12., 400.).mono())
                .color(theme.text_secondary),
        )
        .child(
            label(&issue.title, 14., 400., theme.text)
                .truncate()
                .shrink(),
        );
    if issue.comments > 0 {
        first = first.child(el().grow(1.)).child(
            row()
                .gap(2.)
                .center_y()
                .child(icon(&COMMENT, 12.).color(theme.text_secondary))
                .child(small(&issue.comments.to_string())),
        );
    }
    let mut second = row()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .center_y()
        .child(small(&issue.author));
    if let Some(scope) = &issue.scope {
        second = second.child(small(scope).truncate().shrink());
    }
    let second = second
        .child(el().grow(1.))
        .child(small(&crate::pull_requests::age(
            issue.updated_at,
            &labels.ago,
            &labels.just_now,
        )));
    let reference = issue.reference.clone();
    let selected = s.history.issue() == Some(&reference);
    row()
        .role("button")
        .label(&format!("{} {}", issue.key, issue.title))
        .w_full()
        .h(56.)
        .px(12.)
        .gap(8.)
        .center_y()
        .radius(6.)
        .fill(if selected {
            theme.hover
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(theme.hover)
        .focusable(true)
        .on_press_tx(move |s: &mut Desktop, tx| {
            s.open_index_page(Page::Issue(reference.clone()));
            s.sync_issues(tx);
        })
        .child(icon(mark, 16.).color(ink))
        .child(col().grow(1.).min_w(0.).gap(2.).child(first).child(second))
}

const COMMENT: Icon = Icon {
    name: "message-square",
    view: 24.,
    body: r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(crate) fn view(
    s: &Desktop,
    theme: &tok::Theme,
    dark: bool,
    width: f32,
    overlays: &mut Vec<El>,
) -> El {
    let page = &s.workspace.issues;
    let Some(labels) = &page.labels else {
        return el();
    };
    let split = width >= 820. && page.detail.is_some();
    if !split && let Some(panel) = &page.detail {
        page.menu.set(None);
        return detail::view(s, panel, labels, theme, dark);
    }
    crate::saved_views::dialog::<SavedIssues>(s, theme, overlays);
    let wide = width >= 640. && !split;
    let search = text_input(&page.query, TextStyle::new(13., 400.))
        .key("issue-search")
        .label(&labels.search)
        .placeholder(&labels.search, theme.text_secondary)
        .single_line()
        .min_w(0.)
        .grow(1.)
        .px(4.)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.workspace.issues.query)
        .on_input_change(|s: &mut Desktop, _| s.load_issues("search"))
        .on_submit(|s: &mut Desktop, _| s.load_issues("list"));
    let search = row()
        .center_y()
        .gap(4.)
        .px(10.)
        .h(34.)
        .radius(8.)
        .border(1., theme.card_border)
        .child(icon(&rune_kit::glyphs::SEARCH, 14.).color(theme.text_secondary))
        .child(search);
    let mut refresh = el()
        .key("issue-refresh")
        .role("button")
        .label(&labels.refresh)
        .help(&labels.refresh)
        .size(32., 32.)
        .center()
        .radius(8.)
        .border(1., theme.card_border)
        .child(icon(&crate::workspace::palette::REFRESH, 14.).color(theme.text_secondary));
    if page.pending {
        refresh = refresh.disabled(true).opacity(0.5)
    } else {
        refresh = refresh
            .hover(theme.hover)
            .focusable(true)
            .on_press(|s: &mut Desktop| s.load_issues("refresh"))
    };
    let controls = row()
        .center_y()
        .gap(8.)
        .child(menu(s, MenuKind::Sort, theme, dark, overlays))
        .child(menu(s, MenuKind::Filter, theme, dark, overlays))
        .child(crate::saved_views::menu::<SavedIssues>(
            s,
            theme,
            &s.contrast.kit(dark),
            overlays,
        ))
        .children(
            (!page.hosts.is_empty() || !page.host.is_empty())
                .then(|| menu(s, MenuKind::Host, theme, dark, overlays)),
        )
        .child(refresh);
    let toolbar = if wide {
        row()
            .w_full()
            .gap(8.)
            .center_y()
            .child(search.grow(1.).min_w(0.))
            .child(controls)
    } else {
        col()
            .w_full()
            .gap(8.)
            .child(search.w_full())
            .child(controls)
    };
    let mut body = col()
        .w_full()
        .grow(1.)
        .min_h(0.)
        .px(if wide { 30. } else { 12. })
        .pt(24.)
        .pb(12.)
        .gap(16.)
        .child(toolbar);
    if !page.pending && page.error.is_none() && !page.list.has_trackers && page.operation != 0 {
        body = body.child(
            col()
                .w_full()
                .grow(1.)
                .center()
                .gap(6.)
                .child(icon(&ICON, 24.).color(theme.text_secondary))
                .child(label(&labels.unconfigured, 16., 600., theme.text))
                .child(label(&labels.add_project, 13., 400., theme.text_secondary).wrap()),
        );
    }
    if let Some(error) = &page.error {
        body = body.child(
            label(error, 13., 400., theme.text_secondary)
                .role("status")
                .wrap(),
        );
    }
    if page.pending && page.lines.is_empty() {
        body = body.child(
            label(&labels.loading, 13., 400., theme.text_secondary)
                .role("status")
                .px(12.),
        );
    }
    let colors = *theme;
    let rows = col()
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            page.lines.len(),
            56.,
            |s: &Desktop, i| {
                let page = &s.workspace.issues;
                match page.lines[i] {
                    Line::Heading(g) => format!("tracker:{}", page.list.groups[g].key).into(),
                    Line::Notice(g) => format!("notice:{g}").into(),
                    Line::Error(e) => format!("error:{e}").into(),
                    Line::Empty(g) => format!("empty:{g}").into(),
                    Line::Row(g, r) => page.list.groups[g].rows[r].identity.clone().into(),
                }
            },
            move |s: &Desktop, i| {
                let page = &s.workspace.issues;
                let (Some(line), Some(labels)) = (page.lines.get(i).copied(), &page.labels) else {
                    return el();
                };
                let note = |text: &str, color: rune::Color, size: f32| {
                    label(text, size, 400., color).wrap().px(12.).pb(4.)
                };
                match line {
                    Line::Heading(g) => heading(&page.list.groups[g], &colors, i > 0),
                    Line::Notice(g) => note(
                        page.list.groups[g].notice.as_deref().unwrap_or_default(),
                        colors.text_secondary,
                        14.,
                    ),
                    Line::Error(e) => note(
                        &page.list.errors[e],
                        rune::oklch(if dark { 0.704 } else { 0.577 }, 0.2, 25.),
                        12.,
                    ),
                    Line::Empty(_) => note(
                        if page.list.searching {
                            &labels.no_match
                        } else {
                            &labels.empty
                        },
                        colors.text_secondary,
                        14.,
                    ),
                    Line::Row(g, r) => {
                        issue_row(s, &page.list.groups[g].rows[r], labels, &colors, dark)
                    }
                }
            },
        )
        .virtual_rows_version((page.operation, page.pending, colors.text, s.history.issue()));
    body = body.child(
        col()
            .key("issue-list")
            .grow(1.)
            .min_h(0.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows),
    );
    if page.list.more {
        body = body.child(action(&labels.more, theme, !page.pending, |s| {
            s.load_issues("more")
        }));
    }
    if split && let Some(panel) = &page.detail {
        row()
            .w_full()
            .grow(1.)
            .min_h(0.)
            .child(body.w((width * 0.38).clamp(260., 420.)).grow(0.))
            .child(el().w(1.).h_full().fill(theme.separator))
            .child(detail::view(s, panel, labels, theme, dark))
    } else {
        body
    }
}
