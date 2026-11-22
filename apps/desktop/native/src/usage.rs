pub(crate) mod summary;
use crate::{Desktop, Request, workspace};
use rune::{Cx, El, Icon, ScrollSpec, col, el, icon, row, spacer, tok};
use serde::{Deserialize, Serialize};

pub(crate) const CHART: Icon = Icon {
    name: "usage",
    view: 24.,
    body: r#"<path d="M18 20V10M12 20V4M6 20v-6" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub(super) enum Action<'a> {
    Open,
    Close,
    Refresh,
    Summary { selection: u64, days: u32 },
    RefreshSummary,
    Preferences { preferences: summary::Preferences },
    Manage { url: &'a str },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Labels {
    cost: String,
    tokens: String,
    periods: [String; 4],
    totals: String,
    cost_by_type: String,
    tokens_by_type: String,
    processed: String,
    estimated: String,
    savings: String,
    daily_cost: String,
    hourly_cost: String,
    daily_tokens: String,
    hourly_tokens: String,
    share: String,
    day: String,
    hour: String,
    cached: String,
    input: String,
    output: String,
    breakdown: String,
    model: String,
    unpriced: String,
    no_usage: String,
    more: String,
    usage: String,
    limits: String,
    refresh: String,
    reconnect: String,
    disconnected: String,
    close: String,
    manage: String,
    reveal: String,
    hide: String,
    loading: String,
    empty: String,
}

#[derive(Deserialize)]
struct Account {
    name: String,
    email: Option<String>,
    remaining: f32,
    reset: Option<String>,
}

#[derive(Deserialize)]
struct Window {
    name: String,
    remaining: f32,
    accounts: Vec<Account>,
}

#[derive(Deserialize)]
struct Group {
    name: String,
    windows: Vec<Window>,
}

#[derive(Deserialize)]
struct Link {
    label: String,
    message: Option<String>,
    url: String,
}

#[derive(Deserialize)]
struct Limits {
    groups: Vec<Group>,
    notices: Vec<String>,
    links: Vec<Link>,
}

#[derive(Deserialize)]
pub(super) struct Snapshot {
    labels: Labels,
    result: Option<Limits>,
    #[serde(skip)]
    stale: bool,
    refreshing: bool,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct Panel {
    pub(super) open: bool,
    revealed_email: Option<String>,
    pub(crate) summary: summary::Summary,
    pub(super) labels: Option<Labels>,
    pub(super) view: Option<Snapshot>,
}

impl Panel {
    pub(super) fn title(&self) -> &str {
        self.labels.as_ref().map_or("Usage", |labels| &labels.usage)
    }

    pub(super) fn clear_results(&mut self) {
        self.revealed_email = None;
        self.view = None;
        self.summary.clear();
    }

    pub(super) fn receive(&mut self, mut snapshot: Snapshot) {
        if self.summary.metric != summary::Metric::Limits {
            return;
        }
        if snapshot.result.is_none() {
            snapshot.stale = true;
            snapshot.result = self.view.as_mut().and_then(|v| v.result.take());
        }
        self.view = Some(snapshot);
    }
}

impl Desktop {
    pub(super) fn usage_action(&mut self, action: Action<'_>) {
        if let Some(watch) = self.workspace.feed {
            self.write_request(None, Request::Usage { watch, action });
        }
    }

    pub(super) fn start_usage(&mut self) {
        if let Some(view) = &mut self.workspace.usage.view {
            view.stale = true;
        }
        let request = self.workspace.usage.summary.request();
        self.usage_action(request);
    }

    pub(super) fn open_usage(&mut self) {
        if self.workspace.usage.labels.is_none() || self.workspace.feed.is_none() {
            return;
        }
        self.dismiss_palette();
        self.workspace.usage.open = true;
        self.start_usage();
    }

    pub(super) fn close_usage(&mut self) {
        self.usage_action(Action::Close);
        self.workspace.usage.open = false;
        self.workspace.usage.clear_results();
    }
}

pub(super) fn trigger(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let title = &s.workspace.usage.labels.as_ref()?.usage;
    Some(
        el().key("sidebar-usage")
            .role("button")
            .label(title)
            .help(title)
            .size(32., 32.)
            .radius(6.)
            .center()
            .focusable(true)
            .hover(theme.text.alpha(0.06))
            .child(icon(&CHART, 16.).color(theme.text_secondary))
            .on_press(Desktop::open_usage),
    )
}

fn window_row(window: &Window, theme: &tok::Theme, labels: &Labels, revealed: Option<&str>) -> El {
    let value = format!("{}%", window.remaining.round());
    let mut card = col()
        .w_full()
        .gap(8.)
        .padding(12.)
        .radius(10.)
        .border(1., theme.separator)
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    workspace::label(&window.name, 14., 500., theme.text)
                        .min_w(0.)
                        .grow(1.)
                        .truncate(),
                )
                .child(workspace::label(&value, 14., 600., theme.text)),
        )
        .child(
            el().w_full()
                .h(7.)
                .clip_rounded(4.)
                .fill(theme.separator)
                .child(
                    el().w_frac(window.remaining.clamp(0., 100.) / 100.)
                        .h_full()
                        .fill(theme.text.alpha(0.75)),
                ),
        );
    for account in &window.accounts {
        let mut line = row().w_full().gap(8.).center_y().child(
            workspace::label(&account.name, 12., 400., theme.text_secondary)
                .min_w(0.)
                .grow(1.)
                .truncate(),
        );
        if let Some(email) = &account.email {
            let shown = revealed == Some(email.as_str());
            let help = if shown { &labels.hide } else { &labels.reveal };
            let caption = if shown {
                email.as_str()
            } else {
                "••••••"
            };
            let email = email.clone();
            line = line.child(
                el().role("button")
                    .label(help)
                    .help(help)
                    .focusable(true)
                    .max_w(160.)
                    .child(workspace::label(caption, 12., 400., theme.text_secondary).truncate())
                    .on_press(move |s: &mut Desktop| {
                        s.workspace.usage.revealed_email = (!shown).then(|| email.clone());
                    }),
            );
        }
        if let Some(reset) = &account.reset {
            line = line.child(workspace::label(reset, 12., 400., theme.text_secondary));
        }
        if window.accounts.len() > 1 {
            line = line.child(workspace::label(
                &format!("{}%", account.remaining.round()),
                12.,
                500.,
                theme.text,
            ));
        }
        card = card.child(line);
    }
    card
}

pub(super) fn overlay(s: &Desktop, cx: &Cx<Desktop>, theme: &tok::Theme, overlays: &mut Vec<El>) {
    let panel = &s.workspace.usage;
    if !panel.open {
        return;
    }
    let snapshot = panel.view.as_ref();
    let Some(labels) = snapshot.map(|v| &v.labels).or(panel.labels.as_ref()) else {
        return;
    };
    let [width, height] = cx.layout_size("workspace").unwrap_or([1100., 780.]);
    let inset = if width >= 1024. {
        40.
    } else if width >= 640. {
        24.
    } else {
        0.
    };
    let mut body = col()
        .key("usage-result")
        .w_full()
        .max_w(1024.)
        .self_center()
        .min_h(0.)
        .grow(1.)
        .px(if width < 640. { 20. } else { 24. })
        .pt(24.)
        .pb(48.)
        .gap(12.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    let paused = s.workspace.paused;
    if paused {
        body = body.child(
            workspace::label(
                s.error.as_deref().unwrap_or(&labels.disconnected),
                14.,
                400.,
                theme.text,
            )
            .wrap(),
        );
    }
    if panel.summary.metric != summary::Metric::Limits {
        if width < 860. {
            body = body.child(summary::periods(s, labels, theme));
        }
        body = summary::body(body, s, s.dark(cx), labels, theme, width >= 1024.);
    } else {
        if let Some(error) = snapshot.and_then(|s| s.error.as_ref()) {
            body = body.child(workspace::label(error, 13., 500., theme.text).wrap());
        }
        if let Some(view) = snapshot.and_then(|s| s.result.as_ref()) {
            if view.groups.is_empty() && view.notices.is_empty() && view.links.is_empty() {
                body = body
                    .child(workspace::label(&labels.empty, 14., 400., theme.text_secondary).wrap());
            }
            for group in &view.groups {
                body = body.child(workspace::label(&group.name, 16., 600., theme.text));
                for window in &group.windows {
                    body = body.child(window_row(
                        window,
                        theme,
                        labels,
                        panel.revealed_email.as_deref(),
                    ));
                }
            }
            for notice in &view.notices {
                body = body.child(workspace::label(notice, 14., 400., theme.text_secondary).wrap());
            }
            for link in &view.links {
                let url = link.url.clone();
                body = body.child(
                    row()
                        .w_full()
                        .center_y()
                        .child(
                            workspace::label(&link.label, 14., 500., theme.text)
                                .min_w(0.)
                                .grow(1.)
                                .truncate(),
                        )
                        .child(workspace::action(
                            &labels.manage,
                            theme,
                            !paused && snapshot.is_some_and(|v| !v.stale),
                            move |s| s.usage_action(Action::Manage { url: &url }),
                        )),
                );
                if let Some(message) = &link.message {
                    body = body
                        .child(workspace::label(message, 13., 400., theme.text_secondary).wrap());
                }
            }
        } else if !paused {
            body = body.child(workspace::label(
                &labels.loading,
                14.,
                400.,
                theme.text_secondary,
            ));
        }
    }
    let kit = s.contrast.kit(s.dark(cx));
    let close = rune_kit::IconButton::new(kit.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label(&labels.close)
        .build(&kit)
        .key("usage-close")
        .autofocus()
        .on_press(Desktop::close_usage);
    let can_refresh = if paused {
        !s.pending && s.workspace.refresh_request.is_none()
    } else if panel.summary.metric == summary::Metric::Limits {
        snapshot.is_some_and(|v| v.result.is_some() && !v.stale && !v.refreshing)
    } else {
        panel.summary.ready()
    };
    let refresh = rune_kit::IconButton::new(&workspace::palette::REFRESH)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label(if paused {
            &labels.reconnect
        } else {
            &labels.refresh
        })
        .build(&kit)
        .key("usage-refresh")
        .size(26., 26.)
        .disabled(!can_refresh);
    let refresh = if can_refresh {
        refresh.on_press(|s: &mut Desktop| {
            if s.workspace.paused {
                return s.send(Request::Refresh);
            }
            s.usage_action(
                if s.workspace.usage.summary.metric == summary::Metric::Limits {
                    Action::Refresh
                } else {
                    Action::RefreshSummary
                },
            )
        })
    } else {
        refresh
    };
    let mut header = row()
        .w_full()
        .h(46.)
        .px(16.)
        .center_y()
        .gap(8.)
        .child(workspace::label(&labels.usage, 14., 500., theme.text))
        .child(workspace::label("/", 14., 400., theme.text_secondary))
        .child(
            workspace::label(
                s.environment.as_deref().unwrap_or(""),
                14.,
                500.,
                theme.text,
            )
            .min_w(0.)
            .truncate(),
        )
        .child(spacer());
    if panel.summary.metric != summary::Metric::Limits && width >= 860. {
        header = header.child(summary::periods(s, labels, theme));
    }
    let header = header
        .child(summary::controls(s, labels, theme))
        .child(refresh)
        .child(close.size(26., 26.));
    let panel = col()
        .key("usage-panel")
        .role("dialog")
        .label(&labels.usage)
        .w((width - 2. * inset).clamp(1., 1280.))
        .h((height - 2. * inset).clamp(1., 960.))
        .fill(theme.bg)
        .radius(if inset > 0. { 16. } else { 0. })
        .shadow(16., 32., rune::Color::BLACK.alpha(0.24))
        .border(1., theme.separator)
        .child(header)
        .child(el().w_full().h(1.).fill(theme.separator))
        .child(body)
        .modal(true, Desktop::close_usage)
        .modal_click_outside(true);
    overlays.push(
        el().absolute(0., 0.)
            .w_full()
            .h_full()
            .center()
            .backdrop_blur(3.)
            .fill(theme.bg.alpha(0.62))
            .child(panel),
    );
}
