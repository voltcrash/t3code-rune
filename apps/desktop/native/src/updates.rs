use super::{Desktop, Request, workspace};
use rune::{El, Tx, row, spacer, tok};
use rune_kit::{Dialog, Kit};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Result {
    title: String,
    message: String,
    #[serde(default)]
    available: bool,
    #[serde(default)]
    downloaded: bool,
}

#[derive(Default)]
pub(super) struct Panel {
    pub(super) open: bool,
    busy: bool,
    downloading: bool,
    result: Option<Result>,
}

impl Panel {
    pub(super) fn receive(&mut self, result: Result) {
        self.result = Some(result);
        self.busy = false;
        self.downloading = false;
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
        if !self.update.busy {
            self.update.result = None;
            self.update.busy = self.write_request(None, Request::CheckUpdates);
        }
    }

    fn download_update(&mut self) {
        if !self.update.busy {
            self.update.busy = self.write_request(None, Request::DownloadUpdate);
            self.update.downloading = self.update.busy;
        }
    }
}

pub(super) fn overlay(s: &Desktop, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) {
    if !s.update.open {
        return;
    }
    let close = |s: &mut Desktop| s.update.open = false;
    let mut buttons = row().w_full().px(16.).pb(16.).gap(8.).child(spacer());
    let (title, message) = if s.update.busy {
        if s.update.downloading {
            ("Downloading…", "Downloading and verifying the update.")
        } else {
            ("Checking…", "Checking for updates.")
        }
    } else if let Some(result) = &s.update.result {
        if result.available {
            buttons = buttons.child(workspace::action(
                "Download",
                theme,
                true,
                Desktop::download_update,
            ));
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
    Dialog::new("desktop-update", title)
        .description(message)
        .open(true)
        .width(420.)
        .fit()
        .child(buttons.child(workspace::action("Close", theme, true, close)))
        .on_close(close)
        .build(&Kit::neutral(dark), overlays);
}
