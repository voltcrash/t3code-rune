use super::Result;
use x11rb::{
    connection::Connection, cursor, protocol::xproto::*, resource_manager,
    rust_connection::RustConnection,
};

// The order is the EWMH _NET_WM_MOVERESIZE direction (clockwise from top-left).
const CURSORS: [&str; 8] = [
    "top_left_corner",
    "top_side",
    "top_right_corner",
    "right_side",
    "bottom_right_corner",
    "bottom_side",
    "bottom_left_corner",
    "left_side",
];

pub(super) struct Edges {
    windows: [Window; 8],
    width: u32,
}

impl Edges {
    pub(super) fn new(
        conn: &RustConnection,
        parent: Window,
        root: Window,
        scale: f64,
    ) -> Result<Self> {
        let screen = conn
            .setup()
            .roots
            .iter()
            .position(|s| s.root == root)
            .ok_or("unknown screen")?;
        let resources = resource_manager::new_from_default(conn)?;
        let cursors = cursor::Handle::new(conn, screen, &resources)?.reply()?;
        let mut windows = [0; 8];
        for (window, name) in windows.iter_mut().zip(CURSORS) {
            *window = conn.generate_id()?;
            let cursor = cursors.load_cursor(conn, name)?;
            conn.create_window(
                0,
                *window,
                parent,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new()
                    .cursor(cursor)
                    .event_mask(EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE),
            )?
            .check()?;
        }
        Ok(Self {
            windows,
            width: (scale * 5.).round().max(1.) as u32,
        })
    }

    pub(super) fn direction(&self, window: Window) -> Option<u32> {
        self.windows
            .iter()
            .position(|id| *id == window)
            .map(|n| n as u32)
    }

    pub(super) fn arrange(&self, conn: &RustConnection, size: (u16, u16)) -> Result<()> {
        let (w, h) = (u32::from(size.0), u32::from(size.1));
        let b = self.width.min(w / 2).min(h / 2).max(1);
        let corner = (b * 2).min(w / 2).min(h / 2);
        let across = w.saturating_sub(2 * corner).max(1);
        let down = h.saturating_sub(2 * corner).max(1);
        let regions = [
            (0, 0, corner, corner),
            (corner, 0, across, b),
            (w - corner, 0, corner, corner),
            (w - b, corner, b, down),
            (w - corner, h - corner, corner, corner),
            (corner, h - b, across, b),
            (0, h - corner, corner, corner),
            (0, corner, b, down),
        ];
        for (window, (x, y, width, height)) in self.windows.into_iter().zip(regions) {
            conn.configure_window(
                window,
                &ConfigureWindowAux::new()
                    .x(x as i32)
                    .y(y as i32)
                    .width(width.max(1))
                    .height(height.max(1)),
            )?;
        }
        Ok(conn.flush()?)
    }

    pub(super) fn show(
        &self,
        conn: &RustConnection,
        enabled: bool,
        size: (u16, u16),
    ) -> Result<()> {
        if enabled {
            self.arrange(conn, size)?;
        }
        for window in self.windows {
            if enabled {
                conn.map_window(window)?;
            } else {
                conn.unmap_window(window)?;
            }
        }
        Ok(conn.flush()?)
    }
}
