use super::{Action, Labels};
use crate::{Desktop, workspace};
use rune::{Color, El, LineCap, Path, Shape, canvas, col, el, row, spacer, tok};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Metric {
    Cost,
    Tokens,
    #[default]
    Limits,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub(crate) struct Preferences {
    metric: Metric,
    days: u32,
}

#[derive(Deserialize)]
struct Amount {
    cost: String,
    tokens: String,
}

impl Amount {
    fn display(&self, metric: Metric) -> &str {
        match metric {
            Metric::Cost => &self.cost,
            _ => &self.tokens,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Provider {
    driver: String,
    sessions: String,
    cost_share: String,
    token_share: String,
    name: String,
    #[serde(flatten)]
    amount: Amount,
}

#[derive(Deserialize)]
struct Period {
    name: String,
    cost: f64,
    tokens: f64,
    display: Amount,
}

#[derive(Deserialize)]
struct Tick {
    y: f32,
    label: String,
}

#[derive(Deserialize)]
struct Line {
    provider: String,
    start: f32,
    curve: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
struct Chart {
    ticks: Vec<Tick>,
    lines: Vec<Line>,
}

#[derive(Deserialize)]
struct Charts {
    cost: Chart,
    tokens: Chart,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Model {
    name: String,
    provider: String,
    #[serde(flatten)]
    amount: Amount,
    count: u64,
    price: f64,
    cost_share: String,
    token_share: String,
}

#[derive(Deserialize)]
struct Category {
    label: String,
    cost: f64,
    tokens: f64,
    ink: f32,
    display: Amount,
}

#[derive(Deserialize)]
struct Totals {
    categories: Vec<Category>,
    total: Amount,
    sessions: String,
    savings: String,
    charts: Charts,
    cached: String,
    input: String,
    output: String,
    unpriced: bool,
    providers: Vec<Provider>,
    periods: Vec<Period>,
    models: Vec<Model>,
    notices: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct Reply {
    selection: u64,
    loading: bool,
    #[serde(default)]
    restricted: bool,
    data: Option<Totals>,
    error: Option<String>,
}

pub(crate) struct Summary {
    pub(super) metric: Metric,
    days: u32,
    serial: u64,
    limit: usize,
    time_rows: bool,
    reply: Option<Reply>,
}

impl Default for Summary {
    fn default() -> Self {
        Self {
            metric: Metric::Limits,
            days: 30,
            serial: 0,
            limit: 20,
            time_rows: false,
            reply: None,
        }
    }
}

impl Summary {
    pub(crate) fn restore(&mut self, preferences: Option<Preferences>) {
        if let Some(p) = preferences {
            self.metric = p.metric;
            self.days = p.days;
        }
    }

    fn preferences(&self) -> Action<'static> {
        Action::Preferences {
            preferences: Preferences {
                metric: self.metric,
                days: self.days,
            },
        }
    }

    pub(super) fn request(&mut self) -> Action<'static> {
        self.reply = None;
        self.serial += 1;
        self.limit = 20;
        if self.metric == Metric::Limits {
            Action::Open
        } else {
            Action::Summary {
                selection: self.serial,
                days: self.days,
            }
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.reply
            .as_ref()
            .is_some_and(|r| !r.loading && !r.restricted)
    }

    pub(crate) fn receive(&mut self, mut reply: Reply) {
        if self.metric == Metric::Limits || reply.selection != self.serial {
            return;
        }
        if let Some(data) = &mut reply.data {
            if self.metric == Metric::Tokens {
                data.models.sort_by_key(|m| std::cmp::Reverse(m.count));
            } else {
                data.models.sort_by(|a, b| b.price.total_cmp(&a.price));
            }
        }
        self.reply = Some(reply);
    }
}

impl Desktop {
    pub(crate) fn select_usage_metric(&mut self, metric: Metric) {
        let summary = &mut self.workspace.usage.summary;
        if summary.metric == metric {
            return;
        }
        let previous = summary.metric;
        summary.metric = metric;
        let preferences = summary.preferences();
        if metric == Metric::Limits || previous == Metric::Limits {
            let request = summary.request();
            self.usage_action(request);
        } else if let Some(reply) = summary.reply.take() {
            summary.receive(reply);
        }
        self.usage_action(preferences);
    }

    pub(crate) fn select_usage_period(&mut self, days: u32) {
        let summary = &mut self.workspace.usage.summary;
        if summary.metric == Metric::Limits || days == summary.days {
            return;
        }
        summary.days = days;
        let preferences = summary.preferences();
        let request = summary.request();
        self.usage_action(request);
        self.usage_action(preferences);
    }
}

fn option(
    title: &str,
    selected: bool,
    theme: &tok::Theme,
    run: impl Fn(&mut Desktop) + 'static,
) -> El {
    let mut button = el()
        .role("button")
        .label(title)
        .focusable(true)
        .h(26.)
        .px(9.)
        .radius(5.)
        .center()
        .hover(theme.separator)
        .on_press(run)
        .child(workspace::label(
            title,
            12.,
            if selected { 550. } else { 400. },
            theme.text,
        ));
    if selected {
        button = button.fill(theme.bg).border(1., theme.separator);
    }
    button
}

pub(super) fn controls(s: &Desktop, labels: &Labels, theme: &tok::Theme) -> El {
    let metric = s.workspace.usage.summary.metric;
    let mut group = row()
        .gap(1.)
        .radius(6.)
        .padding(2.)
        .fill(theme.text.alpha(0.055));
    for (item, title) in [
        (Metric::Cost, &labels.cost),
        (Metric::Tokens, &labels.tokens),
        (Metric::Limits, &labels.limits),
    ] {
        group = group.child(option(title, item == metric, theme, move |s| {
            s.select_usage_metric(item)
        }));
    }
    group
}

pub(super) fn periods(s: &Desktop, labels: &Labels, theme: &tok::Theme) -> El {
    let mut group = row()
        .gap(1.)
        .padding(2.)
        .radius(6.)
        .fill(theme.text.alpha(0.055));
    for (days, title) in [1, 7, 30, 90].into_iter().zip(&labels.periods) {
        group = group.child(option(
            title,
            days == s.workspace.usage.summary.days,
            theme,
            move |s| s.select_usage_period(days),
        ));
    }
    group
}

fn statistic(title: &str, value: &str, theme: &tok::Theme) -> El {
    col()
        .min_w(104.)
        .grow(1.)
        .gap(4.)
        .child(workspace::label(title, 12., 400., theme.text_secondary).truncate())
        .child(workspace::label(value, 16., 500., theme.text))
}

fn color(provider: &str, theme: &tok::Theme) -> Color {
    let value = match provider {
        "claude" => "#d97757",
        "cursor" => "#8b8b8b",
        "opencode" => "#5b9bbd",
        "antigravity" => "#8c7bd1",
        "pi" => "#4d9f8a",
        "muse" => "#0866ff",
        "grok" => return theme.text.alpha(0.72),
        _ => return theme.text,
    };
    rune::color::hex(value).unwrap_or(theme.text)
}

fn chart(data: &Totals, metric: Metric, theme: &tok::Theme, title: &str) -> El {
    let chart = if metric == Metric::Cost {
        &data.charts.cost
    } else {
        &data.charts.tokens
    };
    let mut shape = Shape::new().view_box(960., 260.);
    let mut axis = el().w(56.).h(224.);
    for tick in &chart.ticks {
        shape = shape.line(
            [0., tick.y],
            [960., tick.y],
            1.,
            LineCap::Butt,
            theme.separator,
        );
        axis = axis.child(
            row()
                .absolute(0., tick.y * 224. / 260. - 6.)
                .w(56.)
                .justify_end()
                .child(workspace::label(
                    &tick.label,
                    10.,
                    400.,
                    theme.text_secondary,
                )),
        );
    }
    for line in &chart.lines {
        let step = 960. / line.curve.len().max(1) as f32;
        let mut path = Path::new().move_to([0., line.start]);
        for (index, [c1, c2, end]) in line.curve.iter().enumerate() {
            let x = index as f32 * step;
            path = path.cubic_to(
                [x + step / 3., *c1],
                [x + 2. * step / 3., *c2],
                [x + step, *end],
            );
        }
        let ink = color(&line.provider, theme);
        let area = path
            .clone()
            .line_to([960., 260.])
            .line_to([0., 260.])
            .close();
        shape =
            shape
                .fill_path(&area, ink.alpha(0.12))
                .stroke_path(&path, 1.5, LineCap::Round, ink);
    }
    let mut plot = canvas(shape).w_full().h(224.);
    let mut hover = row().absolute(0., 0.).w_full().h_full();
    for period in &data.periods {
        hover = hover.child(el().w(0.).grow(1.).h_full().help(format!(
            "{} · {}",
            period.name,
            period.display.display(metric)
        )));
    }
    plot = plot.child(hover);
    let mut dates = row().w_full().pl(64.);
    if let (Some(first), Some(middle), Some(last)) = (
        data.periods.first(),
        data.periods.get(data.periods.len() / 2),
        data.periods.last(),
    ) {
        for (idx, period) in [first, middle, last].into_iter().enumerate() {
            if idx > 0 {
                dates = dates.child(spacer());
            }
            dates = dates.child(workspace::label(
                &period.name,
                10.,
                400.,
                theme.text_secondary,
            ));
        }
    }
    col()
        .grow(1.)
        .min_w(0.)
        .gap(10.)
        .child(workspace::label(title, 14., 500., theme.text))
        .child(
            row()
                .w_full()
                .gap(8.)
                .child(axis)
                .child(el().w(0.).grow(1.).child(plot)),
        )
        .child(dates)
}

fn model_bar(model: &Model, first: Option<&Model>, metric: Metric) -> f32 {
    let value = |m: &Model| {
        if metric == Metric::Cost {
            m.price
        } else {
            m.count as f64
        }
    };
    let peak = first.map(value).unwrap_or_default();
    if peak <= 0. || value(model) <= 0. {
        0.
    } else {
        (192. * value(model) / peak).clamp(8., 192.) as f32
    }
}

fn data_row(
    name: El,
    cost: &str,
    share: &str,
    tokens: &str,
    index: &str,
    theme: &tok::Theme,
) -> El {
    row()
        .w_full()
        .gap(8.)
        .center_y()
        .py(8.)
        .child(workspace::label(index, 12., 400., theme.text_secondary).w(20.))
        .child(name.min_w(0.).grow(1.))
        .child(
            row()
                .w(60.)
                .justify_end()
                .child(workspace::label(cost, 13., 500., theme.text)),
        )
        .child(row().w(52.).justify_end().child(workspace::label(
            share,
            12.,
            400.,
            theme.text_secondary,
        )))
        .child(row().w(56.).justify_end().child(workspace::label(
            tokens,
            12.,
            400.,
            theme.text_secondary,
        )))
}

pub(super) fn body(
    mut body: El,
    desktop: &Desktop,
    dark: bool,
    labels: &Labels,
    theme: &tok::Theme,
    wide: bool,
) -> El {
    let summary = &desktop.workspace.usage.summary;
    let provider_mark = |provider: &str, size| {
        let driver = if provider == "claude" {
            "claudeAgent"
        } else {
            provider
        };
        let colors = crate::sidebar_providers::Colors {
            rest: theme.text,
            hovered: theme.text,
        };
        desktop
            .workspace
            .providers
            .driver_mark(driver, false, dark, colors, size)
            .unwrap_or_else(|| {
                el().size(size, size).center().child(workspace::label(
                    &provider.chars().take(1).collect::<String>().to_uppercase(),
                    10.,
                    500.,
                    theme.text,
                ))
            })
    };
    let row_name = |name: &str| workspace::label(name, 13., 500., theme.text).truncate();
    let reply = summary.reply.as_ref();
    if let Some(error) = reply.and_then(|r| r.error.as_deref()) {
        body = body.child(workspace::label(error, 14., 400., theme.text).wrap());
    }
    let Some(data) = reply.and_then(|r| r.data.as_ref()) else {
        return if !desktop.workspace.paused && reply.is_none_or(|r| r.loading) {
            body.child(workspace::label(
                &labels.loading,
                14.,
                400.,
                theme.text_secondary,
            ))
        } else {
            body
        };
    };
    let metric = summary.metric;
    let mut overview = col().gap(8.).child(workspace::label(
        data.total.display(metric),
        36.,
        600.,
        theme.text,
    ));
    let subtitle = if metric == Metric::Cost {
        format!("{} · {}", data.sessions, labels.estimated)
    } else {
        data.sessions.clone()
    };
    overview = overview.child(workspace::label(&subtitle, 12., 400., theme.text_secondary));
    if data.unpriced && metric == Metric::Cost {
        overview = overview
            .child(workspace::label(&labels.unpriced, 12., 400., theme.text_secondary).wrap());
    }
    for p in &data.providers {
        overview = overview.child(
            col()
                .w_full()
                .gap(4.)
                .pt(10.)
                .child(
                    row()
                        .w_full()
                        .gap(6.)
                        .center_y()
                        .child(el().size(8., 8.).radius(4.).fill(color(&p.driver, theme)))
                        .child(provider_mark(&p.driver, 16.))
                        .child(workspace::label(&p.name, 14., 500., theme.text).truncate())
                        .child(
                            workspace::label(&p.sessions, 11., 400., theme.text_secondary)
                                .truncate(),
                        )
                        .child(spacer())
                        .child(workspace::label(
                            p.amount.display(metric),
                            14.,
                            500.,
                            theme.text,
                        )),
                )
                .child(workspace::label(
                    if metric == Metric::Cost {
                        &p.cost_share
                    } else {
                        &p.token_share
                    },
                    12.,
                    400.,
                    theme.text_secondary,
                )),
        );
    }
    let heading = match (metric, summary.days == 1) {
        (Metric::Cost, true) => &labels.hourly_cost,
        (Metric::Cost, false) => &labels.daily_cost,
        (_, true) => &labels.hourly_tokens,
        (_, false) => &labels.daily_tokens,
    };
    let plot = chart(data, metric, theme, heading);
    body = if wide {
        body.child(row().w_full().gap(24.).child(overview.w(288.)).child(plot))
    } else {
        body.child(overview.w_full()).child(plot.w_full())
    };
    for notice in &data.notices {
        body = body.child(workspace::label(notice, 13., 400., theme.text_secondary).wrap());
    }
    let mut stats = row().w_full().gap(12.).flex_wrap();
    for (label, value) in [
        (&labels.processed, &data.total.tokens),
        (&labels.cached, &data.cached),
        (&labels.input, &data.input),
        (&labels.output, &data.output),
        (&labels.savings, &data.savings),
    ] {
        stats = stats.child(statistic(label, value, theme));
    }
    body = body.child(
        col()
            .gap(10.)
            .pt(12.)
            .child(workspace::label(&labels.totals, 14., 500., theme.text))
            .child(stats),
    );
    let mut bars = row().w_full().h(8.).radius(4.).clip_soft(0.).gap(2.);
    let mut legend = row().w_full().flex_wrap().gap(9.);
    let mut populated = false;
    for c in &data.categories {
        let value = if metric == Metric::Cost {
            c.cost
        } else {
            c.tokens
        };
        if value <= 0. {
            continue;
        }
        populated = true;
        let ink = theme.text.alpha(c.ink);
        bars = bars.child(el().w(0.).min_w(1.).grow(value as f32).h_full().fill(ink));
        legend = legend.child(
            row()
                .center_y()
                .gap(5.)
                .child(el().size(8., 8.).radius(2.).fill(ink))
                .child(workspace::label(&c.label, 12., 400., theme.text_secondary))
                .child(workspace::label(
                    c.display.display(metric),
                    12.,
                    400.,
                    theme.text,
                )),
        );
    }
    if populated {
        let name = if metric == Metric::Cost {
            &labels.cost_by_type
        } else {
            &labels.tokens_by_type
        };
        let mut section = col().w_full().pt(12.).gap(10.);
        if wide {
            section = section.max_w(466.);
        }
        body = body.child(
            section
                .child(workspace::label(name, 14., 500., theme.text))
                .child(bars)
                .child(legend),
        );
    }
    let time_label = if summary.days == 1 {
        &labels.hour
    } else {
        &labels.day
    };
    let mut choices = row()
        .gap(1.)
        .radius(6.)
        .padding(2.)
        .fill(theme.text.alpha(0.055));
    for (title, time) in [(&labels.model, false), (time_label, true)] {
        choices = choices.child(option(title, time == summary.time_rows, theme, move |s| {
            let summary = &mut s.workspace.usage.summary;
            summary.time_rows = time;
            summary.limit = 20;
        }));
    }
    body = body.child(
        row()
            .w_full()
            .pt(12.)
            .center_y()
            .child(workspace::label(&labels.breakdown, 14., 500., theme.text))
            .child(spacer())
            .child(choices),
    );
    let mut table = col().w_full().child(data_row(
        row_name(if summary.time_rows {
            time_label
        } else {
            &labels.model
        }),
        &labels.cost,
        &labels.share,
        &labels.tokens,
        "#",
        theme,
    ));
    let line = || el().w_full().h(1.).fill(theme.separator);
    let count = if summary.time_rows {
        let mut count = 0;
        let grand: f64 = data
            .periods
            .iter()
            .map(|p| {
                if metric == Metric::Cost {
                    p.cost
                } else {
                    p.tokens
                }
            })
            .sum();
        for p in data
            .periods
            .iter()
            .rev()
            .filter(|p| p.tokens > 0. || p.cost > 0.)
        {
            count += 1;
            if count <= summary.limit {
                let value = if metric == Metric::Cost {
                    p.cost
                } else {
                    p.tokens
                };
                let share = if grand > 0. {
                    format!("{:.1}%", value / grand * 100.)
                } else {
                    String::new()
                };
                table = table.child(line()).child(data_row(
                    row_name(&p.name),
                    &p.display.cost,
                    &share,
                    &p.display.tokens,
                    &count.to_string(),
                    theme,
                ));
            }
        }
        count
    } else {
        for (index, m) in data.models.iter().take(summary.limit).enumerate() {
            let share = if metric == Metric::Cost {
                &m.cost_share
            } else {
                &m.token_share
            };
            table = table.child(line()).child(
                data_row(
                    col()
                        .gap(6.)
                        .child(
                            row()
                                .gap(6.)
                                .center_y()
                                .child(provider_mark(&m.provider, 14.))
                                .child(row_name(&m.name)),
                        )
                        .child(
                            el().w(model_bar(m, data.models.first(), metric))
                                .h(2.)
                                .radius(1.)
                                .fill(color(&m.provider, theme)),
                        ),
                    &m.amount.cost,
                    share,
                    &m.amount.tokens,
                    &(index + 1).to_string(),
                    theme,
                )
                .help(&m.provider),
            );
        }
        data.models.len()
    };
    if count == 0 {
        table = table.child(
            workspace::label(&labels.no_usage, 14., 400., theme.text_secondary).self_center(),
        );
    }
    body = body.child(table);
    if count > summary.limit {
        body = body.child(row().child(spacer()).child(workspace::action(
            &labels.more,
            theme,
            true,
            |s| s.workspace.usage.summary.limit += 20,
        )));
    }
    body
}
