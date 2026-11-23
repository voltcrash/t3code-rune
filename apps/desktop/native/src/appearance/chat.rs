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
    composer: String,
    advanced: String,
    slash_skills: String,
    slash_description: String,
    slash_reset: String,
    pub(super) close: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    Appearance,
    Chat,
}

impl SettingsPage {
    pub(super) fn title(self, labels: Option<&SettingsLabels>) -> &str {
        match self {
            Self::Appearance => labels.map_or("Appearance", |l| &l.appearance),
            Self::Chat => labels.map_or("Chat", |l| &l.chat),
        }
    }

    pub(super) fn button(self, s: &Desktop, theme: &tok::Theme) -> El {
        let active = self == s.settings_page;
        let name = self.title(s.settings_labels.as_ref());
        let (key, icon) = match self {
            Self::Appearance => ("settings-appearance", &PALETTE),
            Self::Chat => ("settings-chat", &CHAT),
        };
        let button = row()
            .key(key)
            .role("button")
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
                    s.settings_page = self;
                }
                tx.focus(key, true);
            });
        if active {
            button.fill(theme.separator)
        } else {
            button
        }
    }
}

impl Desktop {
    fn show_slash_skills(&mut self, enabled: bool, tx: &mut Tx) {
        if self.saving_client_settings || self.slash_skills == enabled {
            return;
        }
        self.saving_client_settings = self.write_request(None, Request::SetSlashSkills { enabled });
        tx.focus_when_shown("settings-slash-skills", true);
    }
}

pub(super) fn composer(
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
        .clip_rounded(12.)
        .border(1., theme.separator)
        .child(disclosure);
    if s.composer_advanced {
        let checked = s.slash_skills;
        let disabled = s.saving_client_settings;
        let thumb = if narrow { 20. } else { 16. };
        let mut toggle = el()
            .key("settings-slash-skills")
            .role("switch")
            .label(&labels.slash_skills)
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
            toggle =
                toggle.on_press_tx(move |s: &mut Desktop, tx| s.show_slash_skills(!checked, tx));
        }
        let mut title = row()
            .center_y()
            .gap(4.)
            .child(label(&labels.slash_skills, 14., 500., theme.text).wrap());
        if !checked {
            let mut reset =
                reset_button("reset-slash-skills", &labels.slash_reset, theme).disabled(disabled);
            if !disabled {
                reset = reset.on_press_tx(|s: &mut Desktop, tx| s.show_slash_skills(true, tx));
            }
            title = title.child(reset);
        }
        let copy = col()
            .min_w(0.)
            .grow(1.)
            .gap(3.)
            .child(title)
            .child(label(&labels.slash_description, 12., 400., theme.text_secondary).wrap());
        let layout = if narrow { col() } else { row().center_y() };
        group = group
            .child(el().w_full().h(1.).fill(theme.separator))
            .child(
                layout
                    .w_full()
                    .padding(if narrow { 12. } else { 16. })
                    .gap(12.)
                    .child(copy)
                    .child(toggle),
            );
    }
    col()
        .w_full()
        .gap(8.)
        .child(label(&labels.composer, 16., 600., theme.text).pl(12.))
        .child(group)
}
