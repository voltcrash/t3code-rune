use super::{Desktop, Request, workspace};
use rune::{El, Key, ScrollSpec, Tx, col, el, row, spacer, tok};
use rune_kit::{IconButton, IconButtonVariant, Kit};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Result {
    title: String,
    message: String,
    #[serde(default)]
    available: bool,
    #[serde(default)]
    downloaded: bool,
    #[serde(default)]
    installable: bool,
}

#[derive(Clone, Copy)]
enum Operation {
    Check,
    Download,
    Install,
}

#[derive(Default)]
pub(super) struct Panel {
    pub(super) open: bool,
    pending: Option<Operation>,
    result: Option<Result>,
}

impl Panel {
    pub(super) fn receive(&mut self, result: Result) {
        self.result = Some(result);
        self.pending = None;
    }
}

impl Desktop {
    pub(super) fn check_updates(&mut self, _: &mut Tx) {
        if self.output.is_none() {
            return;
        }
        self.dismiss_palette();
        self.update.open = true;
        if !self.update.result.as_ref().is_some_and(|r| r.downloaded) {
            self.refresh_update_check();
        }
    }

    fn refresh_update_check(&mut self) {
        if self.update.pending.is_none() {
            self.update.result = None;
            self.send_update_action(Request::CheckUpdates, Operation::Check);
        }
    }

    fn send_update_action(&mut self, request: Request, operation: Operation) {
        if self.update.pending.is_none() && self.write_request(None, request) {
            self.update.pending = Some(operation);
        }
    }
}

pub(super) fn overlay(s: &Desktop, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) {
    if !s.update.open {
        return;
    }
    let close = |s: &mut Desktop| s.update.open = false;
    let mut buttons = row().w_full().px(16.).pb(16.).gap(8.).child(spacer());
    let (title, message) = if let Some(operation) = s.update.pending {
        match operation {
            Operation::Download => ("Downloading…", "Downloading and verifying the update."),
            Operation::Install => ("Installing…", "Preparing to restart T3 Code."),
            Operation::Check => ("Checking…", "Checking for updates."),
        }
    } else if let Some(result) = &s.update.result {
        if result.available {
            buttons = buttons.child(workspace::action("Download", theme, true, |s| {
                s.send_update_action(Request::DownloadUpdate, Operation::Download)
            }));
        }
        if result.installable {
            buttons = buttons.child(workspace::action("Restart", theme, true, |s| {
                s.send_update_action(Request::InstallUpdate, Operation::Install)
            }));
        }
        if result.downloaded {
            buttons = buttons.child(workspace::action(
                "Check again",
                theme,
                true,
                Desktop::refresh_update_check,
            ));
            buttons = buttons.child(workspace::action("Show in folder", theme, true, |s| {
                s.write_request(None, Request::ShowUpdate);
            }));
        }
        (result.title.as_str(), result.message.as_str())
    } else {
        ("Updates unavailable", "The desktop connection closed.")
    };
    let kit = Kit::neutral(dark);
    let dismiss = IconButton::new(kit.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .label("Close")
        .build(&kit)
        .size(20., 20.)
        .on_press(close);
    let details = col()
        .key("update-message")
        .label("Update details")
        .focusable(true)
        .focus_ring(1., 0., kit.c.border_focus)
        .w_full()
        .max_h(144.)
        .px(16.)
        .mb(12.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .on_key(|_: &mut Desktop, ev, tx| {
            match ev.key {
                Key::PageUp | Key::Up => tx.scroll_page("update-message", -1),
                Key::PageDown | Key::Down => tx.scroll_page("update-message", 1),
                Key::Home => tx.scroll_to_y("update-message", 0.),
                Key::End => tx.scroll_to_bottom_instant("update-message"),
                _ => return false,
            }
            true
        })
        .child(workspace::label(message, 13., 400., theme.text_secondary).wrap_anywhere());
    let panel = col()
        .key("desktop-update-dialog")
        .role("dialog")
        .label(title)
        .description(message)
        .w_full()
        .max_w(420.)
        .radius(6.)
        .fill(kit.c.layer_01)
        .child(
            row()
                .w_full()
                .padding(16.)
                .items_start()
                .child(
                    workspace::label(title, 15., 500., theme.text)
                        .min_w(0.)
                        .shrink()
                        .wrap(),
                )
                .child(spacer())
                .child(dismiss),
        )
        .child(details)
        .child(buttons.child(workspace::action("Close", theme, true, close)))
        .modal(true, close)
        .token_motion(kit.motion.overlay);
    overlays.push(
        el().key("desktop-update-scrim")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .fill(kit.c.scrim)
            .a11y_hidden(),
    );
    overlays.push(
        el().key("desktop-update-layer")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .padding(16.)
            .center()
            .child(kit.elevate(panel, &kit.elevation.overlay)),
    );
}
