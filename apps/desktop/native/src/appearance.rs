use super::{Desktop, Request, workspace::label};
use rune::{Color, Cx, El, Icon, Tx, col, el, icon, row, spacer, tok};
use rune_kit::glyphs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::System => ("System", "Follow the system appearance"),
            Self::Light => ("Light", "Use light mode"),
            Self::Dark => ("Dark", "Use dark mode"),
        }
    }
}

impl Desktop {
    pub(crate) fn dark(&self, cx: &Cx<Desktop>) -> bool {
        match self.appearance {
            Mode::System => cx.system_dark(),
            Mode::Light => false,
            Mode::Dark => true,
        }
    }

    pub(crate) fn open_settings(&mut self, tx: &mut Tx) {
        if self.output.is_none() || self.settings_open {
            return;
        }
        self.dismiss_palette();
        self.models.close();
        self.permission_menu = None;
        self.close_sidebar_menus();
        self.settings_open = true;
        self.appearance_error = None;
        tx.focus("settings-panel", true);
    }

    fn close_settings(&mut self, tx: &mut Tx) {
        self.settings_open = false;
        tx.focus("settings-trigger", true);
    }
}

pub(crate) const GEAR: Icon = Icon {
    name: "settings",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9.67 4.1a2.35 2.35 0 0 1 4.66 0 2.35 2.35 0 0 0 3.32 1.65 2.35 2.35 0 0 1 2.9 3.6 2.35 2.35 0 0 0 0 3.3 2.35 2.35 0 0 1-2.9 3.6 2.35 2.35 0 0 0-3.32 1.65 2.35 2.35 0 0 1-4.66 0 2.35 2.35 0 0 0-3.32-1.65 2.35 2.35 0 0 1-2.9-3.6 2.35 2.35 0 0 0 0-3.3 2.35 2.35 0 0 1 2.9-3.6A2.35 2.35 0 0 0 9.67 4.1Z" transform="translate(0 1)"/><circle cx="12" cy="12" r="3"/></g>"#,
};

pub(crate) fn trigger(theme: &tok::Theme) -> El {
    el().key("settings-trigger")
        .role("button")
        .label("Settings")
        .help("Settings")
        .focusable(true)
        .size(28., 28.)
        .radius(6.)
        .center()
        .hover(theme.separator)
        .child(icon(&GEAR, 16.).color(theme.text_secondary))
        .on_press_tx(Desktop::open_settings)
}

fn color(hex: &str) -> Color {
    rune::color::hex(hex).expect("valid theme color")
}

fn preview_pane(width: f32, height: f32, dark: bool) -> El {
    let canvas = if dark { Color::BLACK } else { Color::WHITE };
    let sidebar = color(if dark { "#0f0f10" } else { "#fafafa" });
    let card = color(if dark { "#121212" } else { "#ffffff" });
    let message = color(if dark { "#27272a" } else { "#e4e4e7" });
    let line = color("#7f7f7f").alpha(0.25);
    let rect = |x: f32, y: f32, w: f32, h: f32, r: f32, c: Color| {
        el().absolute(x * width, y * height)
            .size(w * width, h * height)
            .radius(r)
            .fill(c)
    };
    let mut pane = el()
        .size(width, height)
        .fill(canvas)
        .child(rect(0., 0., 0.22, 1., 0., sidebar))
        .child(rect(0.03, 0.08, 0.16, 0.08, 4., card).border(1., line))
        .child(rect(0.03, 0.22, 0.16, 0.07, 4., message))
        .child(rect(0.03, 0.32, 0.16, 0.07, 4., message.alpha(0.7)))
        .child(rect(0.03, 0.42, 0.16, 0.07, 4., message.alpha(0.5)))
        .child(rect(0.48, 0.11, 0.24, 0.09, 6., message))
        .child(rect(0.27, 0.28, 0.34, 0.05, 3., line))
        .child(rect(0.27, 0.38, 0.26, 0.05, 3., line))
        .child(rect(0.26, 0.77, 0.68, 0.15, 5., card).border(1., line))
        .child(rect(0.28, 0.83, 0.22, 0.035, 4., line))
        .child(rect(0.75, 0.08, 0.2, 0.46, 6., card).border(1., line));
    let blue = color("#3b82f6");
    pane = pane.child(
        el().absolute(width * 0.94 - height * 0.09 - 4., height * 0.8)
            .size(height * 0.09, height * 0.09)
            .radius(height)
            .fill(blue),
    );
    for (i, dot) in [color("#34d399"), blue, color("#fbbf24")]
        .into_iter()
        .enumerate()
    {
        let y = 0.15 + i as f32 * 0.138;
        pane = pane
            .child(rect(0.78, y, 0.014, 0.022, 3., dot))
            .child(rect(0.81, y, 0.09, 0.025, 2., line));
    }
    pane
}

fn tile(mode: Mode, selected: bool, width: f32, theme: &tok::Theme) -> El {
    let (name, description) = mode.labels();
    let inner = (width - 18.).max(1.);
    let height = if inner < 100. { 88. } else { 140. };
    let preview = if mode == Mode::System {
        let half = inner / 2.;
        row()
            .child(
                el().w(half - 1.)
                    .h(height)
                    .clip_rounded(0.)
                    .child(preview_pane(inner, height, false)),
            )
            .child(el().w(2.).h(height).fill(theme.separator))
            .child(
                el().w(half - 1.)
                    .h(height)
                    .clip_rounded(0.)
                    .child(preview_pane(inner, height, true).absolute(-half - 1., 0.)),
            )
    } else {
        preview_pane(inner, height, mode == Mode::Dark)
    };
    col()
        .key(description)
        .role("radio")
        .label(description)
        .checked(selected)
        .focus_selected(selected)
        .focusable(true)
        .w(width)
        .padding(8.)
        .gap(6.)
        .items_center()
        .radius(12.)
        .border(
            if selected { 2. } else { 1. },
            if selected {
                theme.text_secondary
            } else {
                theme.separator
            },
        )
        .fill(theme.text.alpha(if selected { 0.05 } else { 0.02 }))
        .hover(theme.separator)
        .child(
            el().w(inner)
                .h(height)
                .clip_rounded(8.)
                .border(1., theme.separator)
                .child(preview)
                .a11y_hidden(),
        )
        .child(label(
            name,
            12.,
            500.,
            if selected {
                theme.text
            } else {
                theme.text_secondary
            },
        ))
        .on_press(move |s: &mut Desktop| {
            s.write_request(None, Request::SetAppearance { mode });
        })
}

pub(crate) fn overlay(s: &Desktop, cx: &Cx<Desktop>, theme: &tok::Theme, overlays: &mut Vec<El>) {
    if !s.settings_open {
        return;
    }
    let [w, h] = cx
        .layout_size("workspace")
        .or_else(|| cx.layout_size("connection"))
        .unwrap_or([1100., 780.]);
    let margin: f32 = if w >= 1024. {
        40.
    } else if w >= 640. {
        24.
    } else {
        0.
    };
    let top = margin.max(40_f32);
    let width = (w - 2. * margin).min(1280.);
    let height = (h - top - margin).min(960.);
    let nav = if w >= 720. { 192. } else { 0. };
    let content = (width - nav - 50.).clamp(90., 960.);
    let close = el()
        .key("settings-close")
        .role("button")
        .label("Close settings")
        .help("Close (Esc)")
        .focusable(true)
        .size(32., 32.)
        .center()
        .radius(6.)
        .hover(theme.separator)
        .child(icon(&glyphs::CLOSE, 16.).color(theme.text_secondary))
        .on_press_tx(Desktop::close_settings);
    let header = row()
        .w_full()
        .h(48.)
        .pl(20.)
        .pr(8.)
        .center_y()
        .gap(8.)
        .child(label("Settings", 14., 500., theme.text_secondary))
        .child(label("/", 14., 400., theme.text_secondary))
        .child(label("Appearance", 14., 500., theme.text))
        .child(spacer())
        .child(close);
    let mut body = col()
        .w(content)
        .max_w(960.)
        .gap(12.)
        .pt(24.)
        .child(label("Color scheme", 16., 600., theme.text).pl(12.))
        .child(
            row()
                .role("radiogroup")
                .focus_group()
                .label("Appearance mode")
                .w_full()
                .gap(12.)
                .children(
                    [Mode::System, Mode::Light, Mode::Dark]
                        .into_iter()
                        .map(|mode| tile(mode, s.appearance == mode, (content - 24.) / 3., theme)),
                ),
        );
    if let Some(err) = &s.appearance_error {
        body = body.child(label(err, 13., 400., theme.text).wrap());
    }
    let mut panel = row()
        .key("settings-panel")
        .role("dialog")
        .label("Settings")
        .focusable(true)
        .no_window_drag()
        .size(width, height)
        .clip_rounded(if margin > 0. { 16. } else { 0. })
        .border(1., theme.separator)
        .fill(theme.bg);
    if nav > 0. {
        panel = panel.child(
            col()
                .w(nav)
                .h_full()
                .padding(12.)
                .gap(12.)
                .fill(theme.text.alpha(0.025))
                .child(label("Settings", 15., 600., theme.text).padding(8.))
                .child(
                    label("Appearance", 14., 500., theme.text)
                        .padding(8.)
                        .radius(6.)
                        .fill(theme.separator),
                ),
        );
    }
    panel = panel
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .h_full()
                .child(header)
                .child(el().w_full().h(1.).fill(theme.separator))
                .child(col().w_full().items_center().child(body)),
        )
        .modal_tx(true, Desktop::close_settings)
        .modal_click_outside(true);
    overlays.push(
        el().absolute(0., 0.)
            .w_full()
            .h_full()
            .backdrop_blur(3.)
            .fill(Color::BLACK.alpha(0.4))
            .a11y_hidden(),
    );
    overlays.push(
        col()
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .pt(top)
            .pb(margin)
            .center()
            .child(panel),
    );
}
