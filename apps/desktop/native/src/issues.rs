use super::{
    Desktop, Request,
    navigation::Page,
    workspace::{action, label},
};
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
}

#[derive(Clone, Copy, Default, PartialEq, Serialize)]
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
    title: String,
    key: String,
    context: String,
    group: String,
    status: Status,
    url: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct List {
    rows: Vec<Issue>,
    more: bool,
    notices: Vec<String>,
    has_trackers: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Sort,
    Filter,
}

#[derive(Default)]
pub(crate) struct Issues {
    pub labels: Option<Labels>,
    watch: Option<u64>,
    query: TextEdit,
    filter: Filter,
    newest: bool,
    menu: Option<MenuKind>,
    operation: u64,
    pending: bool,
    list: List,
    error: Option<String>,
}

impl Desktop {
    pub(crate) fn open_issues(&mut self) {
        if !self.pending && self.workspace.issues.labels.is_some() {
            self.open_index_page(Page::Issues);
        }
    }

    pub(crate) fn sync_issues(&mut self, tx: &mut rune::Tx) -> bool {
        let active = self.history.on_issues() && self.workspace.issues.labels.is_some();
        let watch = self.workspace.feed.filter(|_| active);
        if watch == self.workspace.issues.watch {
            return false;
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
                },
            );
        }
        self.workspace.issues.watch = watch;
        self.workspace.issues.menu = None;
        self.workspace.issues.list = Default::default();
        self.workspace.issues.error = None;
        self.workspace.issues.pending = false;
        if entering && !self.settings_open && self.palette.is_none() && !self.workspace.usage.open {
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
        if matches!(action, "list" | "search") {
            page.list = Default::default();
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
            if let Some(result) = list {
                page.list = result;
            }
            page.error = error;
            page.pending = false;
        }
    }
}

pub(crate) fn trigger(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let name = &s.workspace.issues.labels.as_ref()?.title;
    let button = el()
        .key("sidebar-issues")
        .role("button")
        .label(name)
        .help(name)
        .size(28., 28.)
        .radius(6.)
        .center()
        .child(icon(&ICON, 16.).color(theme.text_secondary));
    Some(if s.pending {
        button.disabled(true).opacity(0.5)
    } else {
        button
            .focusable(true)
            .hover(theme.hover)
            .on_press(Desktop::open_issues)
    })
}

fn menu(s: &Desktop, kind: MenuKind, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) -> El {
    use rune_kit::{Menu, MenuItem};
    let page = &s.workspace.issues;
    let Some(labels) = &page.labels else {
        return el();
    };
    let (key, caption) = match kind {
        MenuKind::Sort => ("issue-sort", &labels.sort),
        MenuKind::Filter => ("issue-filters", &labels.filters),
    };
    let trigger = action(caption, theme, true, move |s| {
        let menu = &mut s.workspace.issues.menu;
        *menu = if *menu == Some(kind) {
            None
        } else {
            Some(kind)
        };
    })
    .border(1., theme.card_border);
    let open = page.menu == Some(kind);
    let mut menu = Menu::new(key)
        .trigger(trigger)
        .open(open)
        .on_close(|s: &mut Desktop| s.workspace.issues.menu = None);
    if open {
        match kind {
            MenuKind::Filter => {
                for (value, name) in [
                    (Filter::Open, &labels.open),
                    (Filter::Closed, &labels.closed),
                    (Filter::All, &labels.all),
                ] {
                    menu = menu.item(MenuItem::new(name).checked(page.filter == value).on_select(
                        move |s: &mut Desktop| {
                            s.workspace.issues.menu = None;
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
                                s.workspace.issues.menu = None;
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

pub(crate) fn view(
    s: &Desktop,
    theme: &tok::Theme,
    dark: bool,
    wide: bool,
    overlays: &mut Vec<El>,
) -> El {
    let page = &s.workspace.issues;
    let Some(labels) = &page.labels else {
        return el();
    };
    let search = text_input(&page.query, TextStyle::new(13., 400.))
        .key("issue-search")
        .label(&labels.search)
        .placeholder(&labels.search, theme.text_secondary)
        .single_line()
        .min_w(0.)
        .grow(1.)
        .h(30.)
        .px(4.)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.workspace.issues.query)
        .on_input_change(|s: &mut Desktop, _| s.load_issues("search"))
        .on_submit(|s: &mut Desktop, _| s.load_issues("list"));
    let search = row()
        .center_y()
        .gap(4.)
        .px(8.)
        .h(32.)
        .radius(6.)
        .border(1., theme.card_border)
        .child(icon(&rune_kit::glyphs::SEARCH, 14.).color(theme.text_secondary))
        .child(search);
    let mut refresh = el()
        .key("issue-refresh")
        .role("button")
        .label(&labels.refresh)
        .help(&labels.refresh)
        .size(30., 30.)
        .center()
        .radius(6.)
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
        .gap(6.)
        .child(menu(s, MenuKind::Sort, theme, dark, overlays))
        .child(menu(s, MenuKind::Filter, theme, dark, overlays))
        .child(refresh);
    let toolbar = if wide {
        row()
            .w_full()
            .gap(6.)
            .center_y()
            .child(search.grow(1.).min_w(0.))
            .child(controls)
    } else {
        col()
            .w_full()
            .gap(6.)
            .child(search.w_full())
            .child(controls)
    };
    let mut body = col()
        .w_full()
        .grow(1.)
        .min_h(0.)
        .px(if wide { 28. } else { 12. })
        .py(20.)
        .gap(10.)
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
    for notice in &page.list.notices {
        body = body.child(label(notice, 13., 400., theme.text_secondary).wrap());
    }
    if let Some(error) = &page.error {
        body = body.child(
            label(error, 13., 400., theme.text_secondary)
                .role("status")
                .wrap(),
        );
    }
    if page.pending || (page.error.is_none() && page.list.has_trackers && page.list.rows.is_empty())
    {
        body = body.child(
            label(
                if page.pending {
                    &labels.loading
                } else {
                    &labels.empty
                },
                13.,
                400.,
                theme.text_secondary,
            )
            .role("status"),
        );
    }
    let colors = *theme;
    let rows = col()
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            page.list.rows.len(),
            54.,
            |s: &Desktop, i| s.workspace.issues.list.rows[i].identity.clone().into(),
            move |s: &Desktop, i| {
                let Some(issue) = s.workspace.issues.list.rows.get(i) else {
                    return el();
                };
                let summary = format!(
                    "{} · {} · {} · {}",
                    issue.group, issue.key, issue.context, issue.title
                );
                let details = col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(2.)
                    .child(label(&issue.title, 14., 500., colors.text).truncate())
                    .child(
                        label(
                            &format!("{} · {} · {}", issue.group, issue.key, issue.context),
                            12.,
                            400.,
                            colors.text_secondary,
                        )
                        .truncate(),
                    );
                let (mark, ink) = match issue.status {
                    Status::Open => (
                        &ICON,
                        rune::oklch(if dark { 0.85 } else { 0.60 }, 0.15, 163.),
                    ),
                    Status::Completed => (
                        &COMPLETED,
                        rune::oklch(if dark { 0.81 } else { 0.54 }, 0.19, 293.),
                    ),
                    Status::Closed => (&CLOSED, colors.text_secondary),
                };
                let item = row()
                    .w_full()
                    .h(54.)
                    .px(8.)
                    .center_y()
                    .gap(8.)
                    .radius(6.)
                    .child(icon(mark, 16.).color(ink))
                    .child(details);
                if let Some(url) = issue.url.clone() {
                    item.role("link")
                        .label(&summary)
                        .help(&url)
                        .hover(colors.hover)
                        .focusable(true)
                        .on_press_tx(move |_: &mut Desktop, tx| {
                            let _ = tx.open_url(&url);
                        })
                } else {
                    item
                }
            },
        )
        .virtual_rows_version((page.operation, page.pending, colors.text));
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
    body
}
