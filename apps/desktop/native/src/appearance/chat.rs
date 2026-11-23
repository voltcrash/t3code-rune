use super::{Desktop, Request, label, reset_button};
use rune::{El, Tx, col, el, row, tok};
use serde::Deserialize;

const PALETTE: rune::Icon = rune::Icon {
    name: "palette",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/></g>"#,
};

const CHAT: rune::Icon = rune::Icon {
    name: "message-square-text",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z"/><path d="M7 11h10"/><path d="M7 15h6"/><path d="M7 7h8"/></g>"#,
};

#[derive(Deserialize)]
pub(crate) struct SettingsLabels {
    pub(super) settings: String,
    pub(super) appearance: String,
    pub(super) chat: String,
    pub(crate) voice: super::voice::Labels,
    composer: String,
    advanced: String,
    slash_skills: String,
    slash_description: String,
    slash_reset: String,
    screen_reader: String,
    window_title: String,
    title_description: String,
    title_reset: String,
    pub(super) close: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    Appearance,
    Chat,
    Voice,
}

impl SettingsPage {
    pub(super) fn title(self, labels: Option<&SettingsLabels>) -> &str {
        match self {
            Self::Appearance => labels.map_or("Appearance", |l| &l.appearance),
            Self::Voice => labels.map_or("Voice", |l| &l.voice.title),
            Self::Chat => labels.map_or("Chat", |l| &l.chat),
        }
    }

    pub(super) fn tabs(s: &Desktop, theme: &tok::Theme, compact: bool) -> El {
        let layout = if compact { row().px(12.) } else { col() };
        let tabs = layout
            .key("settings-tabs")
            .role("tablist")
            .focus_group()
            .label(
                s.settings_labels
                    .as_ref()
                    .map_or("Settings", |l| l.settings.as_str()),
            )
            .w_full()
            .gap(4.)
            .child(Self::Appearance.tab(s, theme));
        if s.settings_labels.is_some() {
            tabs.child(Self::Chat.tab(s, theme))
                .child(Self::Voice.tab(s, theme))
        } else {
            tabs
        }
    }

    fn tab(self, s: &Desktop, theme: &tok::Theme) -> El {
        let active = self == s.settings_page;
        let name = self.title(s.settings_labels.as_ref());
        let (key, icon) = match self {
            Self::Appearance => ("settings-appearance", &PALETTE),
            Self::Chat => ("settings-chat", &CHAT),
            Self::Voice => ("settings-voice", &super::voice::MIC),
        };
        let tab = row()
            .key(key)
            .role("tab")
            .label(name)
            .focus_selected(active)
            .focusable(true)
            .center_y()
            .h(32.)
            .px(10.)
            .gap(8.)
            .child(rune::icon(icon, 16.).color(if active {
                theme.text
            } else {
                theme.text_secondary
            }))
            .radius(6.)
            .hover(theme.separator)
            .child(label(name, 14., if active { 500. } else { 400. }, theme.text).truncate())
            .on_press_tx(move |s: &mut Desktop, tx| {
                if s.settings_page != self {
                    s.font_families.close();
                    s.chat_width_menu = false;
                    if self == Self::Voice || s.settings_page == Self::Voice {
                        s.show_voice_models(self == Self::Voice);
                    }
                    s.settings_page = self;
                }
                tx.focus(key, true);
            });
        if active {
            tab.fill(theme.separator)
        } else {
            tab
        }
    }
}

#[derive(Clone, Copy)]
enum ChatSwitch {
    SlashSkills,
    WindowTitle,
}

impl ChatSwitch {
    fn key(self) -> &'static str {
        match self {
            Self::SlashSkills => "settings-slash-skills",
            Self::WindowTitle => "settings-window-title",
        }
    }

    fn set(self, s: &mut Desktop, enabled: bool, tx: &mut Tx) {
        if s.saving_client_settings {
            return;
        }
        let request = match self {
            Self::SlashSkills => Request::SetSlashSkills { enabled },
            Self::WindowTitle => Request::SetReaderTitle { enabled },
        };
        s.saving_client_settings = s.write_request(None, request);
        tx.focus_when_shown(self.key(), true);
    }

    fn row(self, s: &Desktop, labels: &SettingsLabels, narrow: bool, theme: &tok::Theme) -> El {
        let key = self.key();
        let (checked, default, name, description, reset_label, reset_key) = match self {
            Self::SlashSkills => (
                s.slash_skills,
                true,
                &labels.slash_skills,
                &labels.slash_description,
                &labels.slash_reset,
                "reset-slash-skills",
            ),
            Self::WindowTitle => (
                s.reader_title,
                false,
                &labels.window_title,
                &labels.title_description,
                &labels.title_reset,
                "reset-window-title",
            ),
        };
        let disabled = s.saving_client_settings;
        let thumb = if narrow { 20. } else { 16. };
        let mut toggle = el()
            .key(key)
            .role("switch")
            .label(name)
            .checked(checked)
            .focusable(!disabled)
            .disabled(disabled)
            .size(2. * thumb - 2., thumb + 2.)
            .radius(thumb)
            .fill(if checked { theme.text } else { theme.separator })
            .child(
                el().absolute(if checked { thumb - 2. } else { 2. }, 2.)
                    .size(thumb - 2., thumb - 2.)
                    .radius(thumb)
                    .fill(theme.bg),
            );
        if !disabled {
            toggle = toggle.on_press_tx(move |s: &mut Desktop, tx| self.set(s, !checked, tx));
        }
        let mut title = row()
            .center_y()
            .gap(4.)
            .child(label(name, 14., 500., theme.text).wrap());
        if checked != default {
            let mut reset = reset_button(reset_key, reset_label, theme).disabled(disabled);
            if !disabled {
                reset = reset.on_press_tx(move |s: &mut Desktop, tx| self.set(s, default, tx));
            }
            title = title.child(reset);
        }
        let copy = col()
            .min_w(0.)
            .grow(1.)
            .gap(3.)
            .child(title)
            .child(label(description, 12., 400., theme.text_secondary).wrap());
        let layout = if narrow { col() } else { row().center_y() };
        layout
            .w_full()
            .padding(if narrow { 12. } else { 16. })
            .gap(12.)
            .child(copy)
            .child(toggle)
    }
}

pub(super) fn settings(
    s: &Desktop,
    labels: &SettingsLabels,
    narrow: bool,
    theme: &tok::Theme,
) -> El {
    let disclosure = row()
        .key("advanced-composer")
        .role("button")
        .label(&labels.advanced)
        .focusable(true)
        .expanded(s.composer_advanced)
        .w_full()
        .h(40.)
        .px(if narrow { 12. } else { 16. })
        .gap(6.)
        .center_y()
        .hover(theme.separator)
        .child(crate::disclosure_chevron(s.composer_advanced, 14.).color(theme.text_secondary))
        .child(label(&labels.advanced, 12., 500., theme.text_secondary))
        .on_press(|s: &mut Desktop| s.composer_advanced = !s.composer_advanced);
    let mut group = col()
        .w_full()
        .radius(12.)
        .clip_rounded(12.)
        .border(1., theme.separator)
        .child(disclosure);
    if s.composer_advanced {
        group = group
            .child(el().w_full().h(1.).fill(theme.separator))
            .child(ChatSwitch::SlashSkills.row(s, labels, narrow, theme));
    }
    let section = |name: &str, child: El| {
        col()
            .w_full()
            .gap(8.)
            .child(label(name, 16., 600., theme.text).pl(12.))
            .child(child)
    };
    let title = ChatSwitch::WindowTitle
        .row(s, labels, narrow, theme)
        .radius(12.)
        .border(1., theme.separator);
    col()
        .w_full()
        .gap(24.)
        .child(section(&labels.composer, group))
        .child(section(&labels.screen_reader, title))
}
