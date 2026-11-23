use super::{Desktop, Request};
#[cfg(target_os = "macos")]
use rune::{App, MenuItem};
use rune::{Key, KeyEvent, PhysicalKeyCode, Tx};

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    In,
    Out,
    Actual,
}

#[derive(Clone, Copy)]
pub(crate) struct WindowZoom {
    pub(crate) factor: f64,
    edited: bool,
}

impl Default for WindowZoom {
    fn default() -> Self {
        Self {
            factor: 1.,
            edited: false,
        }
    }
}

impl WindowZoom {
    pub(crate) fn restore(&mut self, factor: f64) {
        if !self.edited && (0.25..=5.).contains(&factor) {
            self.factor = factor;
        }
    }
}

impl Desktop {
    fn zoom_window(&mut self, direction: Direction) {
        let step = 1.2_f64.sqrt();
        let factor = match direction {
            Direction::In => self.zoom.factor * step,
            Direction::Out => self.zoom.factor / step,
            Direction::Actual => 1.,
        }
        .clamp(0.25, 5.);
        if self.zoom.edited && self.zoom.factor == factor {
            return;
        }
        self.zoom = WindowZoom {
            factor,
            edited: true,
        };
        self.write_request(None, Request::SetWindowZoom { factor });
    }

    fn apply_zoom(&mut self, direction: Direction, tx: &Tx) {
        if !self.zoom_browser(direction.browser_setting(), tx) {
            self.zoom_window(direction);
        }
    }

    pub(crate) fn zoom_key(&mut self, event: &KeyEvent, tx: &Tx) -> bool {
        let Some(direction) = Direction::from_key(event, tx) else {
            return false;
        };
        self.apply_zoom(direction, tx);
        true
    }
}

impl Direction {
    pub(crate) fn browser_setting(self) -> &'static str {
        match self {
            Self::In => "zoomIn",
            Self::Out => "zoomOut",
            Self::Actual => "resetZoom",
        }
    }
    pub(crate) fn from_key(event: &KeyEvent, tx: &Tx) -> Option<Self> {
        if event.key == Key::Insert
            || !event.mods.cmd
            || event.mods.alt
            || (cfg!(target_os = "macos") && event.mods.ctrl)
            || (!cfg!(target_os = "macos") && event.mods.logo)
        {
            return None;
        }
        let typed = match &event.key {
            Key::Command(c) => Some(*c),
            Key::Text(s) if s.len() == 1 => s.chars().next(),
            _ => None,
        };
        let direction = match (typed, tx.physical_key(), event.mods.shift) {
            (Some('+'), _, _)
            | (Some('='), _, false)
            | (_, Some(PhysicalKeyCode::NumpadAdd), false) => Direction::In,
            (Some('='), _, true) if cfg!(target_os = "macos") => Direction::In,
            (Some('-'), _, false) | (_, Some(PhysicalKeyCode::NumpadSubtract), false) => {
                Direction::Out
            }
            (Some('0'), _, false) | (_, Some(PhysicalKeyCode::Numpad0), false) => Direction::Actual,
            _ => return None,
        };
        Some(direction)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn view_menu(app: App<Desktop>) -> App<Desktop> {
    let item = |id, title, shortcut: &str, action: fn(&mut Desktop, &mut Tx)| {
        MenuItem::new(title, action)
            .id(id)
            .key_equivalent(shortcut.parse().expect("valid window zoom shortcut"))
            .menu("View")
    };
    app.menu_item(item("actual-size", "Actual Size", "mod+0", |s, tx| {
        s.apply_zoom(Direction::Actual, tx)
    }))
    .menu_item(item("zoom-in", "Zoom In", "mod+=", |s, tx| {
        s.apply_zoom(Direction::In, tx)
    }))
    .menu_item(item("zoom-out", "Zoom Out", "mod+-", |s, tx| {
        s.apply_zoom(Direction::Out, tx)
    }))
}
