use super::{Desktop, Request, workspace::label};
use rune::{Color, Cx, El, Icon, Tx, col, el, icon, row, spacer, tok};
mod archived;
pub(crate) use archived::{Archived, Group as ArchivedGroup};
mod chat;
mod search;
pub(crate) mod voice;
pub(crate) use chat::{SettingsLabels, SettingsPage};
mod chat_width;
mod contrast;
pub(crate) use contrast::Contrast;
mod fonts;
pub(crate) use chat_width::ChatWidth;
pub(super) use fonts::{FontFamilies, FontSlot};
use rune_kit::glyphs;
use serde::{Deserialize, Serialize};

pub(crate) const SANS: &str = if cfg!(target_os = "macos") {
    "System Font, Inter, system-ui, sans-serif"
} else {
    "Inter, Segoe UI, system-ui, sans-serif"
};
pub(crate) const MONO: &str =
    "ui-monospace, SF Mono, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace";

pub(crate) fn apply_fonts(tx: &mut Tx, sans: &str, code: &str, smoothing: bool) {
    let stack = |saved: &str, fallback: &str| {
        let saved = saved.trim();
        if saved.is_empty() {
            fallback.to_owned()
        } else {
            format!("{}, {fallback}", fonts::saved_css(saved))
        }
    };
    tx.set_font_families(stack(sans, SANS), stack(code, MONO));
    if cfg!(target_os = "macos") {
        // Electron's checked setting uses CSS antialiased text, without CoreText dilation.
        tx.set_font_smoothing(if smoothing {
            rune::FontSmoothing::Off
        } else {
            rune::FontSmoothing::On
        });
    }
}

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
            Mode::System => self.system_dark.unwrap_or_else(|| cx.system_dark()),
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
        self.font_families.close();
        self.chat_width_menu = false;
        self.settings_open = true;
        if self.settings_page == SettingsPage::Voice {
            self.show_voice_models(true);
        }
        if self.settings_page == SettingsPage::Archive {
            self.load_archived();
        }
        self.typography_advanced = false;
        self.composer_advanced = false;
        self.settings_error = None;
        tx.focus("settings-panel", true);
    }

    fn close_settings(&mut self, tx: &mut Tx) {
        if self.settings_page == SettingsPage::Voice {
            self.show_voice_models(false);
        }
        self.settings_open = false;
        self.archived.deleting = None;
        self.clear_settings_search();
        tx.focus("settings-trigger", true);
    }

    fn set_font_smoothing(&mut self, smoothing: bool) {
        if !self.saving_client_settings
            && self.font_smoothing.is_some_and(|value| value != smoothing)
        {
            self.saving_client_settings =
                self.write_request(None, Request::SetFontSmoothing { smoothing });
        }
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

fn reset_button(key: &str, caption: &str, theme: &tok::Theme) -> El {
    const UNDO: Icon = Icon {
        name: "undo-2",
        view: 24.,
        body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12"/></g>"#,
    };
    el().key(key)
        .role("button")
        .label(caption)
        .help("Reset to default")
        .focusable(true)
        .size(22., 22.)
        .radius(4.)
        .center()
        .hover(theme.separator)
        .child(icon(&UNDO, 12.).color(theme.text_secondary))
}

fn typography(
    s: &Desktop,
    narrow: bool,
    theme: &tok::Theme,
    dark: bool,
    popups: &mut Vec<El>,
) -> El {
    let mut group = col()
        .w_full()
        .radius(12.)
        .border(1., theme.separator)
        .child(fonts::row(s, FontSlot::Sans, narrow, theme, dark, popups))
        .child(el().w_full().h(1.).fill(theme.separator))
        .child(fonts::row(s, FontSlot::Code, narrow, theme, dark, popups));
    if cfg!(target_os = "macos") {
        group = group
            .child(el().w_full().h(1.).fill(theme.separator))
            .child(
                row()
                    .key("advanced-typography")
                    .role("button")
                    .label("Advanced")
                    .focusable(true)
                    .expanded(s.typography_advanced)
                    .w_full()
                    .h(40.)
                    .px(if narrow { 12. } else { 16. })
                    .gap(6.)
                    .center_y()
                    .radius(12.)
                    .hover(theme.separator)
                    .child(
                        crate::disclosure_chevron(s.typography_advanced, 14.)
                            .color(theme.text_secondary),
                    )
                    .child(label("Advanced", 12., 500., theme.text_secondary))
                    .on_press(|s: &mut Desktop| s.typography_advanced = !s.typography_advanced),
            );
    }
    if cfg!(target_os = "macos") && s.typography_advanced {
        let checked = s.font_smoothing.unwrap_or(true);
        let enabled = s.font_smoothing.is_some() && !s.saving_client_settings;
        let thumb = if narrow { 20. } else { 16. };
        let toggle = el()
            .key("font-smoothing")
            .role("switch")
            .label("Font smoothing")
            .checked(checked)
            .focusable(true)
            .disabled(!enabled)
            .size(2. * thumb - 2., thumb + 2.)
            .radius(thumb)
            .fill(if checked { theme.text } else { theme.separator })
            .child(
                el().absolute(if checked { thumb - 2. } else { 2. }, 2.)
                    .size(thumb - 2., thumb - 2.)
                    .radius(thumb)
                    .fill(theme.bg),
            )
            .on_press(move |s: &mut Desktop| s.set_font_smoothing(!checked));
        let mut title =
            row()
                .center_y()
                .gap(4.)
                .child(label("Font smoothing", 14., 500., theme.text));
        if !checked {
            title = title.child(
                reset_button(
                    "reset-font-smoothing",
                    "Reset font smoothing to default",
                    theme,
                )
                .disabled(!enabled)
                .on_press_tx(|s: &mut Desktop, tx| {
                    s.set_font_smoothing(true);
                    tx.focus("font-smoothing", true);
                }),
            );
        }
        let description = "Use thinner grayscale text smoothing instead of the macOS default.";
        let copy = col()
            .min_w(0.)
            .grow(1.)
            .gap(3.)
            .child(title)
            .child(label(description, 12., 400., theme.text_secondary).wrap());
        let setting = if narrow {
            col().gap(12.)
        } else {
            row().center_y().gap(16.)
        };
        group = group
            .child(el().w_full().h(1.).fill(theme.separator))
            .child(
                setting
                    .w_full()
                    .padding(if narrow { 12. } else { 16. })
                    .child(copy)
                    .child(toggle),
            );
    }
    col()
        .w_full()
        .gap(10.)
        .child(label("Typography", 16., 600., theme.text).pl(16.))
        .child(group)
}

fn appearance_body(
    s: &Desktop,
    cx: &Cx<Desktop>,
    content: f32,
    narrow: bool,
    theme: &tok::Theme,
    popups: &mut Vec<El>,
) -> El {
    let scheme = col()
        .w_full()
        .gap(10.)
        .child(label("Color scheme", 16., 600., theme.text).pl(16.))
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
    let mut body = col().w_full().gap(32.).child(scheme);
    let interface = col()
        .w_full()
        .radius(12.)
        .border(1., theme.separator)
        .child(contrast::setting(s, narrow, theme, s.dark(cx)))
        .child(el().w_full().h(1.).fill(theme.separator))
        .child(chat_width::setting(s, narrow, theme, s.dark(cx), popups));
    body = body.child(
        col()
            .w_full()
            .gap(10.)
            .child(label("Interface", 16., 600., theme.text).pl(16.))
            .child(interface),
    );
    body = body.child(typography(s, narrow, theme, s.dark(cx), popups));
    body
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
    let nav = if w >= 768. { 240. } else { 0. };
    // The web page container: max-w-4xl with 24pt sides, so at most 848pt of content.
    let content = (width - nav - 48.).clamp(90., 848.);
    let labels = s.settings_labels.as_ref();
    let settings_title = labels.map_or("Settings", |l| l.settings.as_str());
    let close = el()
        .key("settings-close")
        .role("button")
        .label(labels.map_or("Close settings", |l| l.close.as_str()))
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
        .child(label(settings_title, 14., 500., theme.text_secondary))
        .child(label("/", 14., 400., theme.text_secondary))
        .child(label(s.settings_page.title(labels), 14., 500., theme.text))
        .child(spacer())
        .child(close);
    let mut popups = Vec::new();
    let mut body = match (s.settings_page, labels) {
        (SettingsPage::Voice, Some(labels)) => voice::view(s, &labels.voice, w < 640., theme),
        (SettingsPage::Chat, Some(labels)) => chat::settings(s, labels, w < 640., theme),
        (SettingsPage::Archive, Some(labels)) => {
            archived::view(s, &labels.archive, w < 640., theme)
        }
        _ => appearance_body(s, cx, content, w < 640., theme, &mut popups),
    }
    .w(content)
    .max_w(848.)
    .pt(24.);
    if let Some(err) = &s.settings_error {
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
        let mut links = col()
            .w(nav)
            .h_full()
            .padding(8.)
            .gap(8.)
            .fill(theme.text.alpha(0.025));
        if let Some(labels) = labels {
            links = links.child(search::field(s, labels, theme)).child(
                search::results(s, labels, theme)
                    .unwrap_or_else(|| SettingsPage::tabs(s, theme, false)),
            );
        } else {
            links = links.pt(48.).child(SettingsPage::tabs(s, theme, false));
        }
        panel = panel.child(links);
    }
    let mut column = col().grow(1.).min_w(0.).h_full().child(header);
    if nav == 0. && labels.is_some() {
        column = column.child(SettingsPage::tabs(s, theme, true));
    }
    panel = panel
        .child(
            column
                .child(el().w_full().h(1.).fill(theme.separator))
                .child(
                    col()
                        .key(match s.settings_page {
                            SettingsPage::Appearance => "appearance-scroll",
                            SettingsPage::Chat => "chat-settings-scroll",
                            SettingsPage::Voice => "voice-settings-scroll",
                            SettingsPage::Archive => "archived-settings-scroll",
                        })
                        .w_full()
                        .grow(1.)
                        .min_h(0.)
                        .scroll(rune::ScrollSpec {
                            stick_bottom: false,
                        })
                        .items_center()
                        .child(body.pb(48.)),
                ),
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
    overlays.extend(popups);
    archived::delete_dialog(s, theme, overlays);
}
