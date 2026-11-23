use super::{Desktop, El, Panel, button, json, tok};
use rune::{KeyEvent, Tx};
use rune_kit::{Kit, Menu, MenuItem, Placement};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Labels {
    title: String,
    reload: String,
    zoom: String,
    zoom_in: String,
    zoom_out: String,
    reset: String,
    appearance: String,
    system: String,
    light: String,
    dark: String,
    cookies: String,
    cache: String,
}

const MORE: rune::Icon = rune::Icon {
    name: "browser-more",
    view: 24.,
    body: r#"<g fill="currentColor"><circle cx="12" cy="5" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="12" cy="19" r="1.8"/></g>"#,
};

impl Desktop {
    pub(crate) fn zoom_browser(&mut self, setting: &str, tx: &Tx) -> bool {
        if !matches!(tx.focused(), Some("browser-page" | "browser-address")) {
            return false;
        }
        let Some(panel) = &self.workspace.browser.panel else {
            return false;
        };
        let Some(tab) = &panel.page.selected else {
            return false;
        };
        if panel.page.can_write && !panel.page.busy && panel.page.dialog.is_none() {
            self.browser_request(json!({"action":"adjust","tab":tab,"setting":setting}));
        }
        true
    }

    pub(super) fn browser_page_shortcut(
        &mut self,
        event: &KeyEvent,
        tx: &mut Tx,
        down: bool,
    ) -> bool {
        if let Some(direction) = crate::window_zoom::Direction::from_key(event, tx) {
            if down {
                return self.zoom_browser(direction.browser_setting(), tx);
            }
            return true;
        }
        if event.mods.shift || event.mods.alt {
            return false;
        }
        if event.is_command('l') {
            if down {
                if let Some(panel) = &mut self.workspace.browser.panel {
                    panel.address.set_selection(0, panel.address.text.len());
                }
                tx.focus("browser-address", true);
            }
            return true;
        }
        if event.is_command('r') {
            if down {
                self.browser_input(json!({"type":"reload"}));
            }
            return true;
        }
        false
    }
}

pub(super) fn view(
    panel: &Panel,
    labels: &Labels,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
) -> El {
    let tab = panel.page.tab();
    let open = panel.menu_open && tab.is_some();
    let trigger = button(
        &labels.title,
        rune::icon(&MORE, 15.).color(theme.text_secondary),
        tab.is_some(),
        theme,
        |s, _| {
            if let Some(panel) = &mut s.workspace.browser.panel {
                panel.menu_open = !panel.menu_open;
            }
        },
    );
    let mut menu = Menu::new("browser-more")
        .open(open)
        .placement(Placement::BottomEnd)
        .trigger(trigger)
        .on_close(|s: &mut Desktop| {
            if let Some(panel) = &mut s.workspace.browser.panel {
                panel.menu_open = false;
            }
        });
    if open && let Some(tab) = tab {
        let item = |label: &str, setting: &'static str, enabled: bool| {
            let tab = tab.id.clone();
            MenuItem::new(label)
                .disabled(!panel.page.can_write || panel.page.busy || !enabled)
                .on_select(move |s: &mut Desktop| {
                    if let Some(panel) = &mut s.workspace.browser.panel {
                        panel.menu_open = false;
                        if panel.page.tab().is_some_and(|current| current.id == tab) {
                            s.browser_request(
                                json!({"action":"adjust","tab":tab,"setting":setting}),
                            );
                        }
                    }
                })
        };
        menu = menu
            .item(item(&labels.reload, "hardReload", true))
            .separator()
            .group_label(format!("{} · {}%", labels.zoom, tab.zoom))
            .item(item(&labels.zoom_in, "zoomIn", tab.can_zoom_in))
            .item(item(&labels.zoom_out, "zoomOut", tab.can_zoom_out))
            .item(item(&labels.reset, "resetZoom", tab.zoom != 100))
            .separator()
            .group_label(&labels.appearance);
        for (label, scheme) in [
            (&labels.system, "system"),
            (&labels.light, "light"),
            (&labels.dark, "dark"),
        ] {
            menu = menu.item(item(label, scheme, true).checked(tab.scheme == scheme));
        }
        menu = menu
            .separator()
            .item(item(&labels.cookies, "cookies", true))
            .item(item(&labels.cache, "cache", true));
    }
    menu.build(kit, overlays)
}
