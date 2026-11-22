use super::{Desktop, Request, workspace};
use rune::{El, Icon, Key, ScrollSpec, Tx, col, el, icon, row, spacer, tok};
use rune_kit::{IconButton, IconButtonVariant};
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

#[derive(Deserialize)]
pub(super) struct Labels {
    check: String,
    checking: String,
    available: String,
    downloading: String,
    downloaded: String,
    restart: String,
    installing: String,
}

#[derive(Default)]
pub(super) struct Panel {
    pub(super) open: bool,
    pub(super) labels: Option<Labels>,
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

const DOWNLOAD: Icon = Icon {
    name: "download-desktop-update",
    view: 24.,
    body: r#"<path d="M12 3v12m-4-4 4 4 4-4M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) fn sidebar(s: &Desktop, theme: &tok::Theme) -> Option<El> {
    let panel = &s.update;
    let labels = panel.labels.as_ref()?;
    let result = panel.result.as_ref();
    let downloaded = result.is_some_and(|r| r.downloaded);
    let available = result.is_some_and(|r| r.available);
    let refresh = &workspace::palette::REFRESH;
    let (caption, glyph) = match panel.pending {
        Some(Operation::Check) => (&labels.checking, refresh),
        Some(Operation::Download) => (&labels.downloading, &DOWNLOAD),
        Some(Operation::Install) => (&labels.installing, refresh),
        None if result.is_some_and(|r| r.installable) => (&labels.restart, refresh),
        None if downloaded => (&labels.downloaded, refresh),
        None if available => (&labels.available, &DOWNLOAD),
        None => (&labels.check, refresh),
    };
    let active = panel.pending.is_some() || downloaded || available;
    let mut mark = el().size(16., 16.).child(icon(glyph, 16.).color(if active {
        theme.text
    } else {
        theme.text_secondary
    }));
    if downloaded {
        mark = mark.child(
            el().pin(None, None, Some(-3.), Some(-3.))
                .size(10., 10.)
                .radius(5.)
                .fill(theme.text)
                .center()
                .child(icon(&rune_kit::glyphs::CHECK, 8.).color(theme.bg)),
        );
    }
    let mut trigger = el()
        .key("sidebar-update")
        .role("button")
        .label(caption)
        .help(caption)
        .focusable(true)
        .size(32., 32.)
        .radius(16.)
        .center()
        .hover(theme.separator)
        .child(mark)
        .on_press_tx(|s: &mut Desktop, tx| {
            if s.update.result.as_ref().is_some_and(|r| r.available) && s.update.pending.is_none() {
                s.update.open = true;
                s.send_update_action(Request::DownloadUpdate, Operation::Download);
            } else {
                s.check_updates(tx);
            }
        });
    if active {
        trigger = trigger.fill(theme.text.alpha(0.05));
    }
    Some(trigger)
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
    let kit = s.contrast.kit(dark);
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
