use super::{
    Desktop, Request,
    navigation::Page,
    workspace::{action, label},
};
use rune::{
    Color, El, GrowthSpec, Icon, ScrollSpec, TextEdit, TextStyle, col, el, icon, row, text,
    text_input, tok,
};
use serde::Deserialize;

pub(crate) const ICON: Icon = Icon {
    name: "git-pull-request-arrow",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="5" cy="6" r="3"/><path d="M5 9v12"/><circle cx="19" cy="18" r="3"/><path d="m15 9-3-3 3-3"/><path d="M12 6h5a2 2 0 0 1 2 2v7"/></g>"#,
};
const DRAFT: Icon = Icon {
    name: "git-pull-request-draft",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M18 6V5"/><path d="M18 11v-1"/><line x1="6" x2="6" y1="9" y2="21"/></g>"#,
};
const CLOSED: Icon = Icon {
    name: "git-pull-request-closed",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="3"/><path d="M6 9v12"/><path d="m21 3-6 6"/><path d="m21 9-6-6"/><path d="M18 11.5V15"/><circle cx="18" cy="18" r="3"/></g>"#,
};
const MERGED: Icon = Icon {
    name: "git-merge",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 21V9a9 9 0 0 0 9 9"/></g>"#,
};
const USERS: Icon = Icon {
    name: "users",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></g>"#,
};
const PEN_LINE: Icon = Icon {
    name: "pen-line",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z"/></g>"#,
};
const EYE: Icon = Icon {
    name: "eye",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/></g>"#,
};
const SORT: Icon = Icon {
    name: "arrow-down-up",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/></g>"#,
};
const FILTER: Icon = Icon {
    name: "list-filter",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M7 12h10"/><path d="M10 18h4"/></g>"#,
};

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub title: String,
    pub back: String,
    search: String,
    placeholder: String,
    sort: String,
    filters: String,
    refresh: String,
    sorts: Vec<String>,
    state: String,
    states: Vec<String>,
    involvement: String,
    involvements: Vec<String>,
    empty: String,
    empty_detail: String,
    filtered: String,
    filtered_detail: String,
    check: String,
    checking: String,
    more: String,
    loading: String,
    ago: String,
    just_now: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Open,
    Draft,
    Closed,
    Merged,
}

#[derive(Deserialize)]
struct Row {
    url: String,
    number: u64,
    title: String,
    state: State,
    author: String,
    repository: String,
    updated_at: i64,
    additions: u64,
    deletions: u64,
    measured: bool,
}

#[derive(Deserialize)]
pub(crate) struct Group {
    key: String,
    label: String,
    rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Sort,
    Filter,
}

/// One drawn line of the list: a group heading or the row at that group index.
#[derive(Clone, Copy)]
enum Line {
    Heading(usize),
    Row(usize, usize),
}

pub(crate) struct PullRequests {
    pub labels: Option<Labels>,
    watch: Option<u64>,
    query: TextEdit,
    state: usize,
    involvement: usize,
    sort: usize,
    menu: std::cell::Cell<Option<MenuKind>>,
    operation: u64,
    pending: bool,
    refreshing: bool,
    groups: Vec<Group>,
    lines: Vec<Line>,
    revision: u64,
    more: bool,
    filtered: bool,
    error: Option<String>,
}

impl Default for PullRequests {
    fn default() -> Self {
        Self {
            labels: None,
            watch: None,
            query: TextEdit::default(),
            // Web opens on open pull requests of every involvement, ranked by merge readiness.
            state: 1,
            involvement: 0,
            sort: 0,
            menu: Default::default(),
            operation: 0,
            pending: false,
            refreshing: false,
            groups: Vec::new(),
            lines: Vec::new(),
            revision: 0,
            more: false,
            filtered: false,
            error: None,
        }
    }
}

impl PullRequests {
    fn set_groups(&mut self, groups: Vec<Group>) {
        self.revision += 1;
        self.lines.clear();
        for (g, group) in groups.iter().enumerate() {
            if !group.label.is_empty() {
                self.lines.push(Line::Heading(g));
            }
            self.lines
                .extend((0..group.rows.len()).map(|r| Line::Row(g, r)));
        }
        self.groups = groups;
    }

    fn row(&self, line: Line) -> Option<&Row> {
        match line {
            Line::Row(g, r) => self.groups.get(g)?.rows.get(r),
            Line::Heading(_) => None,
        }
    }
}

impl Desktop {
    pub(crate) fn show_pull_requests(&mut self, tx: &mut rune::Tx) {
        if !self.pending && self.workspace.pull_requests.labels.is_some() {
            self.open_index_page(Page::PullRequests);
        }
        self.sync_pull_requests(tx);
    }

    pub(crate) fn sync_pull_requests(&mut self, tx: &mut rune::Tx) -> bool {
        let active =
            self.history.on_pull_requests() && self.workspace.pull_requests.labels.is_some();
        let watch = self.workspace.feed.filter(|_| active);
        let page = &mut self.workspace.pull_requests;
        if watch == page.watch {
            return false;
        }
        let entering = page.watch.is_none() && watch.is_some();
        let old = std::mem::replace(&mut page.watch, watch);
        page.menu.set(None);
        page.set_groups(Vec::new());
        page.more = false;
        page.error = None;
        page.pending = false;
        page.refreshing = false;
        if let Some(old) = old {
            self.write_request(
                None,
                Request::PullRequests {
                    watch: old,
                    operation: 0,
                    action: "close",
                    state: 0,
                    involvement: 0,
                    sort: 0,
                    query: String::new(),
                },
            );
        }
        if entering && !self.settings_open && self.palette.is_none() && !self.workspace.usage.open {
            tx.focus_when_shown("pull-request-search", false);
        }
        if watch.is_some() {
            self.load_pull_requests("list");
        }
        true
    }

    fn load_pull_requests(&mut self, action: &'static str) {
        let Some(watch) = self.workspace.pull_requests.watch else {
            return;
        };
        let operation = self.next_request_id();
        let page = &mut self.workspace.pull_requests;
        // Only a finished listing can be re-sorted in place; otherwise read it again.
        let action = if action == "sort" && (page.pending || page.groups.is_empty()) {
            "list"
        } else {
            action
        };
        if matches!(action, "list" | "search") {
            page.set_groups(Vec::new());
            page.more = false;
        }
        page.operation = operation;
        page.pending = true;
        page.refreshing = action == "refresh";
        page.error = None;
        let request = Request::PullRequests {
            watch,
            operation,
            action,
            state: page.state,
            involvement: page.involvement,
            sort: page.sort,
            query: page.query.text.trim().to_string(),
        };
        self.write_request(None, request);
    }

    pub(crate) fn receive_pull_requests(&mut self, reply: Reply) {
        let page = &mut self.workspace.pull_requests;
        if page.watch != Some(reply.watch) || page.operation != reply.operation {
            return;
        }
        if let Some(groups) = reply.groups {
            page.set_groups(groups);
            page.more = reply.more;
            page.filtered = reply.filtered;
        }
        page.error = reply.error;
        page.pending = false;
        page.refreshing = false;
    }
}

#[derive(Deserialize)]
pub(crate) struct Reply {
    watch: u64,
    operation: u64,
    groups: Option<Vec<Group>>,
    #[serde(default)]
    more: bool,
    #[serde(default)]
    filtered: bool,
    error: Option<String>,
}

pub(crate) fn trigger(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let name = &s.workspace.pull_requests.labels.as_ref()?.title;
    let button =
        crate::workspace::footer_button("sidebar-pull-requests", name, &ICON, theme, !s.pending);
    Some(if s.pending {
        button
    } else {
        button.on_press_tx(Desktop::show_pull_requests)
    })
}

fn toolbar_button(key: &str, caption: &str, glyph: &'static Icon, theme: &tok::Theme) -> El {
    row()
        .key(key)
        .role("button")
        .label(caption)
        .h(32.)
        .px(10.)
        .gap(6.)
        .center_y()
        .radius(8.)
        .border(1., theme.card_border)
        .child(icon(glyph, 14.).color(theme.text_secondary))
        .child(label(caption, 13., 500., theme.text))
}

fn menu(s: &Desktop, kind: MenuKind, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) -> El {
    use rune_kit::{Menu, MenuItem};
    let page = &s.workspace.pull_requests;
    let Some(labels) = &page.labels else {
        return el();
    };
    let (key, caption, glyph) = match kind {
        MenuKind::Sort => ("pull-request-sort", &labels.sort, &SORT),
        MenuKind::Filter => ("pull-request-filters", &labels.filters, &FILTER),
    };
    let trigger = toolbar_button(&format!("{key}-trigger"), caption, glyph, theme)
        .focusable(true)
        .hover(theme.hover)
        .on_press(move |s: &mut Desktop| {
            let menu = &s.workspace.pull_requests.menu;
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
        .on_close(|s: &mut Desktop| s.workspace.pull_requests.menu.set(None));
    if !open {
        return menu.build(&s.contrast.kit(dark), overlays);
    }
    let choose = |field: fn(&mut PullRequests) -> &mut usize, value: usize, action| {
        move |s: &mut Desktop| {
            let page = &mut s.workspace.pull_requests;
            page.menu.set(None);
            if *field(page) != value {
                *field(page) = value;
                s.load_pull_requests(action);
            }
        }
    };
    match kind {
        MenuKind::Sort => {
            for (i, name) in labels.sorts.iter().enumerate() {
                menu = menu.item(
                    MenuItem::new(name)
                        .checked(page.sort == i)
                        .on_select(choose(|p| &mut p.sort, i, "sort")),
                );
            }
        }
        MenuKind::Filter => {
            menu = menu.group_label(&labels.state);
            for (i, name) in labels.states.iter().enumerate() {
                menu = menu.item(
                    MenuItem::new(name)
                        .checked(page.state == i)
                        .on_select(choose(|p| &mut p.state, i, "list")),
                );
            }
            menu = menu.separator().group_label(&labels.involvement);
            for (i, name) in labels.involvements.iter().enumerate() {
                menu = menu.item(
                    MenuItem::new(name)
                        .checked(page.involvement == i)
                        .on_select(choose(|p| &mut p.involvement, i, "list")),
                );
            }
        }
    }
    menu.build(&s.contrast.kit(dark), overlays)
}

fn tone(state: State, dark: bool) -> (Color, &'static Icon) {
    let hex = |light: &str, night: &str| {
        let c = rune::color::hex(if dark { night } else { light }).expect("valid tone");
        if dark { c.alpha(0.9) } else { c }
    };
    match state {
        State::Open => (hex("#059669", "#6ee7b7"), &ICON),
        State::Draft => (
            if dark {
                rune::color::hex("#a1a1aa").expect("valid tone").alpha(0.8)
            } else {
                rune::color::hex("#71717a").expect("valid tone")
            },
            &DRAFT,
        ),
        State::Closed => (hex("#dc2626", "#fca5a5"), &CLOSED),
        State::Merged => (hex("#7c3aed", "#c4b5fd"), &MERGED),
    }
}

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

fn pull_request_row(s: &Desktop, entry: &Row, theme: &tok::Theme, dark: bool) -> El {
    let Some(labels) = &s.workspace.pull_requests.labels else {
        return el();
    };
    let (ink, glyph) = tone(entry.state, dark);
    let small =
        |value: &str, color: Color| text(value).font(TextStyle::new(11., 400.)).color(color);
    let mut first = row()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .center_y()
        .child(
            text(format!("#{}", entry.number))
                .font(TextStyle::new(12., 400.).mono())
                .color(theme.text_secondary),
        )
        .child(
            label(&entry.title, 14., 400., theme.text)
                .truncate()
                .shrink(),
        );
    if entry.measured && entry.additions + entry.deletions > 0 {
        let added = rune::color::hex(if dark { "#34d399" } else { "#047857" }).expect("valid tone");
        let removed =
            rune::color::hex(if dark { "#f87171" } else { "#b91c1c" }).expect("valid tone");
        first = first.child(el().grow(1.)).child(
            row()
                .gap(4.)
                .child(
                    text(&format!("+{}", entry.additions))
                        .font(TextStyle::new(11., 400.).mono())
                        .color(added),
                )
                .child(
                    text(&format!("−{}", entry.deletions))
                        .font(TextStyle::new(11., 400.).mono())
                        .color(removed),
                ),
        );
    }
    let second = row()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .center_y()
        .child(small(&entry.author, theme.text_secondary))
        .child(
            small(&entry.repository, theme.text_secondary)
                .truncate()
                .shrink(),
        )
        .child(el().grow(1.))
        .child(small(&age(entry.updated_at, labels), theme.text_secondary));
    let url = entry.url.clone();
    row()
        .key(entry.url.as_str())
        .role("button")
        .label(&format!("#{} {}", entry.number, entry.title))
        .w_full()
        .h(56.)
        .px(12.)
        .gap(8.)
        .center_y()
        .radius(8.)
        .hover(theme.hover)
        .focusable(true)
        .on_press(move |s: &mut Desktop| {
            s.write_request(None, Request::OpenUrl { url: &url });
        })
        .child(icon(glyph, 16.).color(ink))
        .child(col().grow(1.).min_w(0.).gap(2.).child(first).child(second))
}

fn heading(group: &Group, theme: &tok::Theme) -> El {
    let glyph = match group.key.as_str() {
        "authored" => &PEN_LINE,
        "reviewRequested" => &EYE,
        _ => &USERS,
    };
    let muted = theme.text_secondary.alpha(0.7);
    row()
        .w_full()
        .h(28.)
        .px(12.)
        .pt(8.)
        .pb(4.)
        .gap(8.)
        .center_y()
        .child(icon(glyph, 14.).color(muted))
        .child(label(&group.label, 12., 500., muted))
        .child(label(
            &group.rows.len().to_string(),
            12.,
            500.,
            theme.text_secondary.alpha(0.5),
        ))
        .child(el().grow(1.).min_w(8.).h(1.).fill(theme.separator))
}

pub(crate) fn view(
    s: &Desktop,
    theme: &tok::Theme,
    dark: bool,
    width: f32,
    overlays: &mut Vec<El>,
) -> El {
    let page = &s.workspace.pull_requests;
    let Some(labels) = &page.labels else {
        return el();
    };
    let wide = width >= 640.;
    let search = text_input(&page.query, TextStyle::new(13., 400.))
        .key("pull-request-search")
        .label(&labels.search)
        .placeholder(&labels.placeholder, theme.text_secondary)
        .single_line()
        .min_w(0.)
        .grow(1.)
        .px(4.)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.workspace.pull_requests.query)
        .on_input_change(|s: &mut Desktop, _| s.load_pull_requests("search"))
        .on_submit(|s: &mut Desktop, _| s.load_pull_requests("list"));
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
        .key("pull-request-refresh")
        .role("button")
        .label(&labels.refresh)
        .help(&labels.refresh)
        .size(32., 32.)
        .center()
        .radius(8.)
        .border(1., theme.card_border)
        .child(icon(&crate::workspace::palette::REFRESH, 14.).color(theme.text_secondary));
    refresh = if page.pending {
        refresh.disabled(true).opacity(0.5)
    } else {
        refresh
            .hover(theme.hover)
            .focusable(true)
            .on_press(|s: &mut Desktop| s.load_pull_requests("refresh"))
    };
    let controls = row()
        .center_y()
        .gap(8.)
        .child(menu(s, MenuKind::Sort, theme, dark, overlays))
        .child(menu(s, MenuKind::Filter, theme, dark, overlays))
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
    if let Some(error) = &page.error {
        body = body.child(
            label(error, 13., 400., theme.text_secondary)
                .role("status")
                .wrap(),
        );
    }
    if page.lines.is_empty() {
        if page.pending {
            return body.child(
                label(&labels.loading, 13., 400., theme.text_secondary)
                    .role("status")
                    .px(12.),
            );
        }
        if page.error.is_none() && page.operation != 0 {
            let (title, detail) = if page.filtered {
                (&labels.filtered, &labels.filtered_detail)
            } else {
                (&labels.empty, &labels.empty_detail)
            };
            let mut actions = row().gap(8.);
            if page.more {
                actions = actions.child(action(&labels.more, theme, true, |s| {
                    s.load_pull_requests("more")
                }));
            }
            actions = actions.child(action(
                if page.refreshing {
                    &labels.checking
                } else {
                    &labels.check
                },
                theme,
                !page.pending,
                |s| s.load_pull_requests("refresh"),
            ));
            return body.child(
                col()
                    .w_full()
                    .grow(1.)
                    .center()
                    .gap(6.)
                    .child(icon(&ICON, 24.).color(theme.text_secondary))
                    .child(label(title, 16., 600., theme.text))
                    .child(label(detail, 13., 400., theme.text_secondary).wrap())
                    .child(actions.pt(8.)),
            );
        }
        return body;
    }
    let colors = *theme;
    let rows = col()
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            page.lines.len(),
            56.,
            |s: &Desktop, i| {
                let page = &s.workspace.pull_requests;
                match page.lines[i] {
                    Line::Heading(g) => format!("group:{}", page.groups[g].key).into(),
                    line => page
                        .row(line)
                        .map_or_else(String::new, |r| r.url.clone())
                        .into(),
                }
            },
            move |s: &Desktop, i| {
                let page = &s.workspace.pull_requests;
                match page.lines.get(i).copied() {
                    Some(Line::Heading(g)) => heading(&page.groups[g], &colors),
                    Some(line) => page
                        .row(line)
                        .map_or_else(el, |entry| pull_request_row(s, entry, &colors, dark)),
                    None => el(),
                }
            },
        )
        .virtual_rows_version((page.revision, page.pending, colors.text));
    body = body.child(
        col()
            .key("pull-request-list")
            .w_full()
            .grow(1.)
            .min_h(0.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows),
    );
    if page.more {
        body = body.child(action(&labels.more, theme, !page.pending, |s| {
            s.load_pull_requests("more")
        }));
    }
    body
}
