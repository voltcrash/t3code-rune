use crate::{Desktop, Request};
use rune::{App, Key, KeyEvent, Mods, Tx, raw_window_handle::RawWindowHandle};
use serde::Deserialize;

pub(super) fn register(app: App<Desktop>) -> App<Desktop> {
    let owner = app.handle();
    app.on_window(move |window| {
        let xid = window.with_raw_handle(|handle| match handle.as_raw() {
            RawWindowHandle::Xlib(x) => u32::try_from(x.window).ok(),
            RawWindowHandle::Xcb(x) => Some(x.window.get()),
            _ => None,
        });
        if let Ok(Some(xid)) = xid {
            let scale = window.placement().map_or(1., |placement| placement.scale);
            crate::linux_frame::attach(owner.clone(), xid, scale);
            owner.update(move |s, _| {
                s.write_request(None, Request::X11Menu { window: xid });
            });
        }
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Action {
    Settings,
    CheckUpdates,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAsText,
    Delete,
    SelectAll,
    ActualSize,
    ZoomIn,
    ZoomOut,
    Fullscreen,
    Minimize,
    Maximize,
}

pub(super) fn shortcut(s: &mut Desktop, event: &KeyEvent, tx: &mut Tx) -> bool {
    let mods = event.mods;
    if mods.alt || mods.logo || tx.focused() == Some("thread-terminal-grid") {
        return false;
    }
    let action = match event.key {
        Key::Function(11) if !mods.ctrl && !mods.shift => Action::Fullscreen,
        Key::Command('q') if mods.ctrl && !mods.shift => Action::Quit,
        Key::Command('m') if mods.ctrl && !mods.shift => Action::Minimize,
        Key::Command('v') if mods.ctrl && mods.shift => Action::Paste,
        _ => return false,
    };
    if !tx.key_repeat() {
        action.activate(s, tx);
    }
    true
}

impl Action {
    pub(super) fn activate(self, s: &mut Desktop, tx: &mut Tx) {
        let terminal = tx.focused() == Some("thread-terminal-grid");
        let page = tx.focused() == Some("browser-page");
        let (key, shift) = match self {
            Self::CheckUpdates => return s.check_updates(tx),
            Self::Settings => return s.open_settings(tx),
            Self::Quit => return tx.quit(),
            Self::Fullscreen => return tx.toggle_full_screen(),
            Self::Minimize => return tx.minimize(),
            Self::Maximize => return tx.toggle_maximized(),
            Self::Undo | Self::Redo | Self::Cut | Self::Delete if terminal => return,
            Self::Delete => {
                return tx.send_key(KeyEvent {
                    key: Key::Delete,
                    mods: Mods::default(),
                });
            }
            Self::Undo => ('z', false),
            Self::Redo => ('z', true),
            Self::Copy | Self::Cut if page => {
                return s.copy_from_browser_menu(matches!(self, Self::Cut), tx);
            }
            Self::Paste | Self::PasteAsText if page => return s.paste_into_browser(tx),
            Self::Cut => ('x', false),
            Self::Copy => ('c', terminal),
            Self::PasteAsText
                if matches!(tx.focused(), Some("message-input" | "new-thread-prompt")) =>
            {
                ('v', true)
            }
            Self::Paste | Self::PasteAsText if !terminal => return tx.request_paste(),
            Self::Paste | Self::PasteAsText => ('v', true),
            Self::SelectAll => ('a', terminal),
            Self::ActualSize => ('0', false),
            Self::ZoomIn => ('=', false),
            Self::ZoomOut => ('-', false),
        };
        tx.send_key(KeyEvent {
            key: Key::Command(key),
            mods: Mods {
                cmd: true,
                ctrl: true,
                shift,
                ..Mods::default()
            },
        });
    }
}
