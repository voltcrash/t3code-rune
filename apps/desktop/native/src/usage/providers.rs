use super::{Labels, summary};
use crate::{Desktop, workspace};
use rune::{El, icon, row, tok};
use rune_kit::{Menu, MenuItem, glyphs};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Choice {
    id: String,
    name: String,
}

impl Desktop {
    fn toggle_usage_provider(&mut self, provider: Option<&str>) {
        let panel = &mut self.workspace.usage;
        let hidden = &mut panel.summary.hidden;
        match provider {
            Some(id) => {
                if !hidden.remove(id) {
                    hidden.insert(id.to_owned());
                }
            }
            None if !hidden.is_empty() => hidden.clear(),
            None => {
                let Some(labels) = &panel.labels else { return };
                hidden.extend(labels.providers.iter().map(|p| p.id.clone()));
            }
        }
        panel.clear_results();
        let preferences = panel.summary.preferences();
        self.usage_action(preferences);
    }
}

pub(super) fn menu(
    s: &Desktop,
    labels: &Labels,
    theme: &tok::Theme,
    dark: bool,
    overlays: &mut Vec<El>,
) -> El {
    let panel = &s.workspace.usage;
    let hidden = &panel.summary.hidden;
    let mut visible = labels.providers.iter().filter(|p| !hidden.contains(&p.id));
    let count = visible.clone().count();
    let label = match count {
        0 => &labels.no_providers,
        1 => &visible.next().unwrap().name,
        n if n == labels.providers.len() => &labels.all_providers,
        n => &labels.provider_counts[n - 1],
    };
    let trigger = row()
        .min_w(0.)
        .shrink()
        .h(26.)
        .px(4.)
        .gap(4.)
        .center_y()
        .role("button")
        .label(label)
        .focusable(true)
        .radius(5.)
        .hover(theme.separator)
        .child(
            workspace::label(label, 14., 500., theme.text)
                .min_w(0.)
                .truncate(),
        )
        .child(icon(&glyphs::CHEVRON_DOWN, 14.).color(theme.text_secondary))
        .on_press(|s: &mut Desktop| {
            s.workspace.usage.providers_open = !s.workspace.usage.providers_open
        });
    let mut menu = Menu::new("usage-providers")
        .trigger(trigger)
        .open(panel.providers_open)
        .on_close(|s: &mut Desktop| s.workspace.usage.providers_open = false);
    if panel.providers_open {
        menu = menu
            .item(
                MenuItem::new(&labels.all_providers)
                    .checked(hidden.is_empty())
                    .on_select(|s: &mut Desktop| s.toggle_usage_provider(None)),
            )
            .separator();
        for choice in &labels.providers {
            let id = choice.id.clone();
            menu = menu.item(
                MenuItem::new(&choice.name)
                    .checked(!hidden.contains(&id))
                    .lead(summary::provider_mark(s, dark, theme, &id, 14.))
                    .on_select(move |s: &mut Desktop| s.toggle_usage_provider(Some(&id))),
            );
        }
    }
    menu.build(&s.contrast.kit(dark), overlays)
}
