#[path = "microphone_test.rs"]
mod microphone_test;

use super::{Desktop, Request, label};
use rune::{El, TextEdit, TextStyle, col, el, row, text_input, tok};
use serde::Deserialize;

pub(crate) const MIC: rune::Icon = rune::Icon {
    name: "mic",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19v3M5 10v2a7 7 0 0 0 14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/></g>"#,
};

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub(super) title: String,
    microphone: Box<microphone_test::Labels>,
    pub(crate) dictation: crate::dictation::Labels,
    models: String,
    search: String,
    empty: String,
    download: String,
    select: String,
    selected: String,
    cancel: String,
    remove: String,
    refresh: String,
    loading: String,
    disconnected: String,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    name: String,
    size: String,
    state: ModelState,
    active: bool,
    progress: Option<String>,
    remove_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ModelState {
    Downloadable,
    Downloading,
    Verifying,
    Installed,
}

#[derive(Deserialize)]
pub(crate) struct Catalog {
    models: Vec<Model>,
    message: Option<String>,
    busy: bool,
    can_cancel: bool,
    manage: bool,
}

#[derive(Default)]
pub(crate) struct VoiceSettings {
    pub(crate) catalog: Option<Catalog>,
    query: TextEdit,
    microphone: microphone_test::Test,
}

impl Desktop {
    pub(crate) fn show_voice_models(&mut self, open: bool) {
        if !open {
            self.voice_settings.microphone = Default::default();
        }
        if let Some(watch) = self.workspace.feed {
            self.write_request(None, Request::VoiceModels { watch, open });
        }
    }
}

fn button(key: &str, caption: &str, description: &str, enabled: bool, theme: &tok::Theme) -> El {
    let mut button = el()
        .key(key)
        .role("button")
        .label(description)
        .help(description)
        .px(8.)
        .h(28.)
        .center()
        .radius(6.)
        .border(1., theme.separator)
        .child(label(caption, 12., 500., theme.text).truncate());
    if enabled {
        button = button.focusable(true).hover(theme.separator);
    } else {
        button = button.disabled(true).opacity(0.6);
    }
    button
}

impl Model {
    fn action(
        &self,
        watch: u64,
        action: &'static str,
        caption: &str,
        description: &str,
        enabled: bool,
        theme: &tok::Theme,
    ) -> El {
        let button = button(
            &format!("voice-{action}-{}", self.id),
            caption,
            description,
            enabled,
            theme,
        );
        if !enabled {
            return button;
        }
        let id = self.id.clone();
        button.on_press(move |s: &mut Desktop| {
            if s.workspace.feed == Some(watch) {
                s.write_request(
                    None,
                    Request::ChangeVoiceModel {
                        watch,
                        action,
                        model: &id,
                    },
                );
            }
        })
    }

    fn card(
        &self,
        watch: u64,
        catalog: &Catalog,
        labels: &Labels,
        narrow: bool,
        theme: &tok::Theme,
    ) -> El {
        let enabled = catalog.manage && !catalog.busy;
        let named = |action: &str| format!("{action} · {}", self.name);
        let mut actions = row().gap(6.).center_y().flex_wrap();
        let primary = match self.state {
            ModelState::Downloading | ModelState::Verifying => self.action(
                watch,
                "cancel",
                &labels.cancel,
                &named(&labels.cancel),
                catalog.manage && catalog.can_cancel,
                theme,
            ),
            ModelState::Downloadable => self.action(
                watch,
                "download",
                &labels.download,
                &named(&labels.download),
                enabled,
                theme,
            ),
            ModelState::Installed if !self.active => self.action(
                watch,
                "select",
                &labels.select,
                &named(&labels.select),
                enabled,
                theme,
            ),
            ModelState::Installed => button(
                &format!("voice-selected-{}", self.id),
                &labels.selected,
                &named(&labels.selected),
                false,
                theme,
            ),
        };
        actions = actions.child(primary);
        if matches!(self.state, ModelState::Installed) {
            actions = actions.child(self.action(
                watch,
                "remove",
                &labels.remove,
                &self.remove_label,
                enabled,
                theme,
            ));
        }
        let mut details = col()
            .min_w(0.)
            .gap(3.)
            .child(label(&self.name, 14., 500., theme.text).wrap())
            .child(label(&self.size, 12., 400., theme.text_secondary));
        if let Some(progress) = &self.progress {
            details = details.child(label(progress, 12., 400., theme.text_secondary).wrap());
        }
        let layout = if narrow { col() } else { row().center_y() };
        if !narrow {
            details = details.grow(1.);
        }
        layout
            .w_full()
            .px(12.)
            .py(9.)
            .gap(8.)
            .child(details)
            .child(actions)
    }
}

pub(super) fn view(s: &Desktop, labels: &Labels, narrow: bool, theme: &tok::Theme) -> El {
    let state = &s.voice_settings;
    let mut section = col().w_full().gap(10.).child(microphone_test::view(
        &state.microphone,
        &labels.microphone,
        theme,
    ));
    if let Some(watch) = s.workspace.feed {
        let choice = &s.dictation.choice;
        let name = if choice.retry_revoke {
            &labels.dictation.retry_revoke
        } else if choice.enabled {
            &choice.destination
        } else {
            &labels.dictation.off
        };
        let next = !choice.enabled && !choice.retry_revoke;
        let description = format!("{} · {}", labels.dictation.destination, choice.destination);
        let allowed = choice.available || choice.enabled || choice.retry_revoke;
        let mut control = if choice.retry_revoke {
            button("dictation-choice", name, name, allowed, theme)
        } else {
            button("dictation-choice", name, &description, allowed, theme)
                .role("switch")
                .checked(choice.enabled)
        };
        if allowed {
            control = control.on_press(move |s: &mut Desktop| {
                if s.workspace.feed == Some(watch) {
                    s.write_request(
                        None,
                        Request::SetDictation {
                            watch,
                            enabled: next,
                        },
                    );
                }
            });
        }
        section = section
            .child(label(&labels.dictation.destination, 16., 600., theme.text))
            .child(label(&choice.destination, 13., 400., theme.text_secondary).wrap())
            .child(control);
    }
    section = section.child(label(&labels.models, 16., 600., theme.text));
    let Some(watch) = s.workspace.feed else {
        return section.child(label(&labels.disconnected, 13., 400., theme.text_secondary).wrap());
    };
    let search = text_input(&state.query, TextStyle::new(13., 400.))
        .single_line()
        .key("speech-model-search")
        .label(&labels.search)
        .placeholder(&labels.search, theme.text_secondary)
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .px(9.)
        .color(theme.text)
        .border(1., theme.separator)
        .radius(6.)
        .on_edit(|s: &mut Desktop| &mut s.voice_settings.query);
    section = section.child(
        row().w_full().center_y().gap(8.).child(search).child(
            button(
                "refresh-voice-models",
                &labels.refresh,
                &labels.refresh,
                true,
                theme,
            )
            .on_press(|s: &mut Desktop| s.show_voice_models(true)),
        ),
    );
    let Some(catalog) = &state.catalog else {
        return section.child(label(&labels.loading, 13., 400., theme.text_secondary));
    };
    if let Some(message) = &catalog.message {
        section = section.child(label(message, 13., 400., theme.text).wrap());
    }
    let query = state.query.text.trim().to_lowercase();
    let mut list = col().w_full().border(1., theme.separator).radius(10.);
    let mut count = 0;
    for model in catalog
        .models
        .iter()
        .filter(|model| query.is_empty() || model.name.to_lowercase().contains(&query))
    {
        if count > 0 {
            list = list.child(el().w_full().h(1.).fill(theme.separator));
        }
        count += 1;
        list = list.child(model.card(watch, catalog, labels, narrow, theme));
    }
    if count > 0 {
        section.child(list)
    } else if catalog.message.is_none() {
        section.child(label(&labels.empty, 13., 400., theme.text_secondary))
    } else {
        section
    }
}
