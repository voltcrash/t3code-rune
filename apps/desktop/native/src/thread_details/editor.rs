use super::{ContextAction, Desktop, Request, label};
use rune::{El, Icon, Tx, el, icon, row, tok};
use rune_kit::{Menu, MenuItem, glyphs};
use serde::Deserialize;

#[path = "editor_icons.rs"]
mod artwork;

#[derive(Deserialize)]
struct Editor {
    id: String,
    label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalog {
    editors: Vec<Editor>,
    preferred: Option<String>,
    can_open: bool,
    pub(super) can_choose: bool,
    open: String,
    choose: String,
    unavailable: Option<String>,
    hint: Option<String>,
}

pub(crate) const OPEN: Icon = Icon {
    name: "open-editor",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-7M14 3h7v7m0-7-10 10"/></g>"#,
};

fn editor_icon(id: &str) -> &'static Icon {
    match id {
        "file-manager" if cfg!(target_os = "macos") => artwork::icon("finder"),
        "file-manager" if cfg!(target_os = "windows") => artwork::icon("explorer"),
        id => artwork::icon(id),
    }
}

impl Desktop {
    fn editor_root(&self) -> Option<&str> {
        let (draft, id) = self.details_target()?;
        if draft {
            return self.workspace.project(id).map(|p| p.path.as_str());
        }
        self.thread_workspace_root()
    }

    pub(crate) fn can_open_editor(&self) -> bool {
        self.editor_root().is_some()
            && self
                .workspace
                .details
                .editor
                .as_ref()
                .is_some_and(|c| c.can_open && c.preferred.is_some())
    }

    pub(crate) fn open_editor(&mut self) {
        if !self.can_open_editor() {
            return;
        }
        let Some(watch) = self.workspace.feed else {
            return;
        };
        let Some(cwd) = self.editor_root().map(str::to_owned) else {
            return;
        };
        let Some(editor) = self
            .workspace
            .details
            .editor
            .as_ref()
            .and_then(|c| c.preferred.clone())
        else {
            return;
        };
        self.write_request(
            None,
            Request::OpenEditor {
                watch,
                editor: &editor,
                cwd: &cwd,
            },
        );
    }

    pub(crate) fn update_editors(&mut self, watch: u64, catalog: Catalog) {
        if self.workspace.feed != Some(watch) {
            return;
        }
        self.workspace.details.editor = Some(catalog);
        if self.palette.is_some() {
            self.filter_palette(true);
        }
    }

    fn choose_editor(&mut self, id: &str, tx: &mut Tx) {
        let Some(menu) = self.workspace.details.menu.take() else {
            return;
        };
        if self.details_target() != Some((menu.target.0, menu.target.1.as_str())) {
            return;
        }
        if let Some(watch) = self.workspace.feed
            && self
                .workspace
                .details
                .editor
                .as_ref()
                .is_some_and(|c| c.can_choose && c.can_open && c.editors.iter().any(|e| e.id == id))
        {
            self.write_request(None, Request::ChooseEditor { watch, editor: id });
        }
        tx.focus_when_shown("choose-editor", true);
    }
}

impl Catalog {
    pub(crate) fn open_label(&self) -> &str {
        &self.open
    }

    pub(super) fn row(&self, s: &Desktop, theme: &tok::Theme) -> El {
        let branded = self.preferred.as_ref().is_some_and(|id| {
            id != "file-manager" || cfg!(any(target_os = "macos", target_os = "windows"))
        });
        let primary = row()
            .key("open-editor")
            .role("button")
            .focusable(true)
            .label(&self.open)
            .help(&self.open)
            .grow(1.)
            .min_w(0.)
            .h(32.)
            .px(10.)
            .gap(10.)
            .center_y()
            .radius(8.)
            .hover(theme.hover)
            .child(
                icon(self.preferred.as_deref().map_or(&OPEN, editor_icon), 16.).color(if branded {
                    theme.text
                } else {
                    theme.text_secondary
                }),
            )
            .child(
                label(&self.open, 13., 500., theme.text.alpha(0.8))
                    .truncate()
                    .shrink(),
            )
            .on_press(Desktop::open_editor);
        let available = s.can_open_editor();
        let choose = el()
            .key("choose-editor")
            .role("button")
            .focusable(true)
            .label(&self.choose)
            .help(&self.choose)
            .size(32., 32.)
            .center()
            .radius(8.)
            .hover(theme.hover)
            .disabled(!self.can_choose)
            .child(icon(&glyphs::CHEVRON_DOWN, 16.).color(theme.text_secondary))
            .on_press(|s: &mut Desktop| s.open_details_menu(ContextAction::Editor));
        row()
            .key("details-editor-trigger")
            .w_full()
            .h(32.)
            .center_y()
            .child(if available {
                primary
            } else {
                primary.disabled(true).opacity(0.5)
            })
            .child(choose)
    }

    pub(super) fn menu(&self) -> Menu {
        let mut menu = Menu::new("details-editor")
            .open(true)
            .min_width(262.)
            .on_close(|s: &mut Desktop| s.workspace.details.menu = None);
        if let Some(reason) = &self.unavailable {
            return menu.item(MenuItem::new(reason).disabled(true));
        }
        for editor in &self.editors {
            let id = editor.id.clone();
            let mut item = MenuItem::new(&editor.label)
                .icon(editor_icon(&id))
                .disabled(!self.can_open);
            if self.preferred.as_ref() == Some(&id) {
                item = item.shortcut(if cfg!(target_os = "macos") {
                    "⌘O"
                } else {
                    "Ctrl+O"
                });
            }
            menu =
                menu.item(item.on_select_tx(move |s: &mut Desktop, tx| s.choose_editor(&id, tx)));
        }
        if let Some(hint) = &self.hint {
            menu = menu.item(MenuItem::new(hint).disabled(true));
        }
        menu
    }
}
