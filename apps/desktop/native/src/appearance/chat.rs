use super::{Desktop, Request, label, reset_button};
use rune::{Color, El, Tx, col, el, row, tok};
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
    pub(super) archive: super::archived::Labels,
    pub(super) general: super::general::Labels,
    pub(crate) voice: super::voice::Labels,
    composer: String,
    advanced: String,
    pub(super) slash_skills: String,
    slash_description: String,
    slash_reset: String,
    screen_reader: String,
    pub(super) window_title: String,
    title_description: String,
    title_reset: String,
    pub(super) close: String,
    pub(super) search: String,
    pub(super) search_label: String,
    pub(super) no_results: String,
    pub(super) clear_search: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    General,
    Appearance,
    Chat,
    Voice,
    Archive,
}

impl SettingsPage {
    pub(super) fn title(self, labels: Option<&SettingsLabels>) -> &str {
        match self {
            Self::General => labels.map_or("General", |l| &l.general.title),
            Self::Appearance => labels.map_or("Appearance", |l| &l.appearance),
            Self::Voice => labels.map_or("Voice", |l| &l.voice.title),
            Self::Chat => labels.map_or("Chat", |l| &l.chat),
            Self::Archive => labels.map_or("Archive", |l| &l.archive.title),
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
            .gap(if compact { 4. } else { 16. });
        if s.settings_labels.is_none() {
            return tabs.child(Self::Appearance.tab(s, theme));
        }
        // The web nav's groups; these two have hidden headings.
        let group = |pages: &[Self]| {
            let layout = if compact { row() } else { col() };
            layout
                .gap(4.)
                .children(pages.iter().map(|page| page.tab(s, theme)))
        };
        tabs.child(group(&[
            Self::General,
            Self::Appearance,
            Self::Chat,
            Self::Voice,
        ]))
        .child(group(&[Self::Archive]))
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Chat => "settings-chat",
            Self::Voice => "settings-voice",
            Self::Archive => "settings-archive",
        }
    }

    pub(super) fn select(self, s: &mut Desktop) {
        if s.settings_page != self {
            s.font_families.close();
            s.chat_width_menu = false;
            if self == Self::Voice || s.settings_page == Self::Voice {
                s.show_voice_models(self == Self::Voice);
            }
            s.settings_page = self;
            if self == Self::Archive {
                s.load_archived();
            }
        }
    }

    fn tab(self, s: &Desktop, theme: &tok::Theme) -> El {
        let active = self == s.settings_page;
        let name = self.title(s.settings_labels.as_ref());
        let key = self.key();
        let icon = match self {
            Self::General => &super::general::SETTINGS,
            Self::Appearance => &PALETTE,
            Self::Chat => &CHAT,
            Self::Voice => &super::voice::MIC,
            Self::Archive => &super::archived::ARCHIVE,
        };
        let color = if active {
            theme.text
        } else {
            theme.text_secondary.alpha(0.8)
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
            .child(rune::icon(icon, 16.).color(color))
            .radius(8.)
            .hover(theme.text.alpha(0.03))
            .child(label(name, 14., 500., color).truncate())
            .on_press_tx(move |s: &mut Desktop, tx| {
                self.select(s);
                tx.focus(key, true);
            });
        if active {
            tab.fill(theme.text.alpha(0.03))
        } else {
            tab
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Switch {
    SlashSkills,
    WindowTitle,
    ConfirmUnpin,
    ConfirmArchive,
    ConfirmDelete,
}

impl Switch {
    fn key(self) -> &'static str {
        match self {
            Self::SlashSkills => "settings-slash-skills",
            Self::WindowTitle => "settings-window-title",
            Self::ConfirmUnpin => "settings-confirm-unpin",
            Self::ConfirmArchive => "settings-confirm-archive",
            Self::ConfirmDelete => "settings-confirm-delete",
        }
    }

    fn set(self, s: &mut Desktop, enabled: bool, tx: &mut Tx) {
        if s.saving_client_settings {
            return;
        }
        let request = match self {
            Self::SlashSkills => Request::SetSlashSkills { enabled },
            Self::WindowTitle => Request::SetReaderTitle { enabled },
            Self::ConfirmUnpin => Request::SetConfirmation {
                setting: "unpin",
                enabled,
            },
            Self::ConfirmArchive => Request::SetConfirmation {
                setting: "archive",
                enabled,
            },
            Self::ConfirmDelete => Request::SetConfirmation {
                setting: "delete",
                enabled,
            },
        };
        s.saving_client_settings = s.write_request(None, request);
        tx.focus_when_shown(self.key(), true);
    }

    pub(super) fn row(
        self,
        s: &Desktop,
        labels: &SettingsLabels,
        narrow: bool,
        theme: &tok::Theme,
    ) -> El {
        let general = &labels.general;
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
            Self::ConfirmUnpin => (
                s.confirm_thread_unpin,
                false,
                &general.unpin,
                &general.unpin_description,
                &general.unpin_reset,
                "reset-confirm-unpin",
            ),
            Self::ConfirmArchive => (
                s.confirm_thread_archive,
                false,
                &general.archive,
                &general.archive_description,
                &general.archive_reset,
                "reset-confirm-archive",
            ),
            Self::ConfirmDelete => (
                s.confirm_thread_delete,
                true,
                &general.delete,
                &general.delete_description,
                &general.delete_reset,
                "reset-confirm-delete",
            ),
        };
        let disabled = s.saving_client_settings;
        let dark = theme.text.r > 0.5;
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
            .fill(match (checked, dark) {
                (true, true) => super::color("#346bf1"),
                (true, false) => super::color("#1b4ed8"),
                (false, true) => Color::WHITE.alpha(0.11),
                (false, false) => super::color("#d4d4d8"),
            })
            .child(
                el().absolute(if checked { thumb - 2. } else { 2. }, 2.)
                    .size(thumb - 2., thumb - 2.)
                    .radius(thumb)
                    .fill(match (checked, dark) {
                        (false, true) => super::color("#a3a3a3"),
                        _ => theme.bg,
                    }),
            );
        if disabled {
            toggle = toggle.opacity(0.64);
        }
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
            .child(Switch::SlashSkills.row(s, labels, narrow, theme));
    }
    let section = |name: &str, child: El| {
        col()
            .w_full()
            .gap(10.)
            .child(label(name, 16., 600., theme.text).pl(16.))
            .child(child)
    };
    let title = Switch::WindowTitle
        .row(s, labels, narrow, theme)
        .radius(12.)
        .border(1., theme.separator);
    col()
        .w_full()
        .gap(32.)
        .child(section(&labels.composer, group))
        .child(section(&labels.screen_reader, title))
}
