use super::{Desktop, Request, Workspace};
use rune::{El, Icon, Tx, el, icon, tok};
use serde::Deserialize;
use std::{cell::Cell, collections::HashSet};

pub(super) const MAXIMIZE: Icon = Icon {
    name: "maximize-2",
    view: 24.,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M15 3h6v6m0-6-7 7M3 21l7-7m-1 7H3v-6"/>"#,
};

const RESTORE: Icon = Icon {
    name: "minimize-2",
    view: 24.,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m14 10 7-7m-1 7h-6V4M3 21l7-7m-6 0h6v6"/>"#,
};

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct State {
    maximized: HashSet<String>,
    maximize: String,
    restore: String,
    #[serde(skip)]
    pub(super) available: Cell<bool>,
}

impl Workspace {
    pub(super) fn panel_maximized(&self) -> bool {
        self.visible_thread()
            .is_some_and(|t| self.panel_width.presentation.maximized.contains(&t.id))
    }

    pub(super) fn maximize_label(&self) -> &str {
        let labels = &self.panel_width.presentation;
        if self.panel_maximized() {
            &labels.restore
        } else {
            &labels.maximize
        }
    }
}

impl Desktop {
    pub(crate) fn set_panel_maximized(&mut self, maximized: bool) {
        if self.workspace.panel_maximized() == maximized {
            return;
        }
        let Some(watch) = self.workspace.feed else {
            return;
        };
        let Some(thread) = self.workspace.visible_thread().map(|t| t.id.clone()) else {
            return;
        };
        let states = &mut self.workspace.panel_width.presentation.maximized;
        if maximized {
            states.insert(thread.clone());
        } else {
            states.remove(&thread);
        }
        self.write_request(
            None,
            Request::SetPanelMaximized {
                watch,
                thread: &thread,
                maximized,
            },
        );
    }

    pub(super) fn toggle_panel_maximized(&mut self, tx: &mut Tx) {
        if self.workspace.panel_width.presentation.available.get() {
            self.set_panel_maximized(!self.workspace.panel_maximized());
            tx.focus_when_shown("maximize-workspace-panel", true);
        }
    }

    pub(crate) fn dismiss_right_panel(&mut self) {
        self.set_panel_maximized(false);
        self.close_browser();
        self.close_diff();
        self.close_files_panel();
    }

    pub(crate) fn maximize_control(&self, theme: &tok::Theme) -> Option<El> {
        let ws = &self.workspace;
        ws.panel_width.presentation.available.get().then(|| {
            let label = ws.maximize_label();
            el().key("maximize-workspace-panel")
                .role("button")
                .label(label)
                .help(label)
                .focusable(true)
                .size(28., 28.)
                .center()
                .radius(5.)
                .hover(theme.hover)
                .no_window_drag()
                .child(
                    icon(
                        if ws.panel_maximized() {
                            &RESTORE
                        } else {
                            &MAXIMIZE
                        },
                        15.,
                    )
                    .color(theme.text_secondary),
                )
                .on_press_tx(Desktop::toggle_panel_maximized)
        })
    }
}
