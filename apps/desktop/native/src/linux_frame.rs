use crate::Desktop;
use rune::{AppHandle, El, Icon, Tx, el, icon, row};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

mod edges;
mod gesture;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

x11rb::atom_manager! {
    Atoms: AtomCookies {
        _MOTIF_WM_HINTS, _NET_SUPPORTED, _NET_WM_MOVERESIZE, _NET_FRAME_EXTENTS, _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN, _NET_WM_STATE_MAXIMIZED_HORZ, _NET_WM_STATE_MAXIMIZED_VERT,
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Frame {
    window: Window,
    custom: bool,
    full_screen: bool,
    maximized: bool,
}

impl Frame {
    pub(super) fn visible(self) -> bool {
        self.custom && !self.full_screen
    }

    pub(super) fn trailing(self, zoom: f64) -> f32 {
        if self.visible() {
            112. / zoom as f32
        } else {
            0.
        }
    }
}

pub(super) fn attach(app: AppHandle<Desktop>, window: Window, scale: f64) {
    app.update(move |s, _| {
        s.frame = Frame {
            window,
            ..Frame::default()
        }
    });
    std::thread::spawn(move || {
        if let Err(error) = watch(&app, window, scale) {
            eprintln!("Could not use the X11 app title bar: {error}");
        }
        app.update(move |s, _| {
            if s.frame.window == window {
                s.frame.custom = false;
            }
        });
    });
}

fn atoms(conn: &RustConnection, window: Window, property: Atom) -> Result<Vec<Atom>> {
    let reply = conn
        .get_property(false, window, property, AtomEnum::ATOM, 0, u32::MAX)?
        .reply()?;
    Ok(reply.value32().into_iter().flatten().collect())
}

fn watch(app: &AppHandle<Desktop>, window: Window, scale: f64) -> Result<()> {
    let (conn, _) = x11rb::connect(None)?;
    let a = Atoms::new(&conn)?.reply()?;
    conn.change_window_attributes(
        window,
        &ChangeWindowAttributesAux::new()
            .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
    )?
    .check()?;
    let geometry = conn.get_geometry(window)?.reply()?;
    let root = geometry.root;
    let supported = atoms(&conn, root, a._NET_SUPPORTED)?;
    if ![a._NET_WM_MOVERESIZE, a._NET_FRAME_EXTENTS]
        .iter()
        .all(|atom| supported.contains(atom))
    {
        return Ok(());
    }

    let mut gesture = gesture::Gesture::new(&conn, root, window, a._NET_WM_MOVERESIZE, scale)?;
    let edges = edges::Edges::new(&conn, window, root, scale)?;
    let motif = a._MOTIF_WM_HINTS;
    let old = conn
        .get_property(false, window, motif, motif, 0, 5)?
        .reply()?;
    let original: Vec<_> = old.value32().into_iter().flatten().collect();
    let mut hints = original.clone();
    hints.resize(5, 0);
    hints[0] |= 2; // MWM_HINTS_DECORATIONS
    hints[2] = 0;
    conn.change_property32(PropMode::REPLACE, window, motif, motif, &hints)?
        .check()?;

    let mut run = || -> Result<()> {
        let mut size = (geometry.width, geometry.height);
        let mut previous = None;
        let mut state_changed = true;
        loop {
            if state_changed {
                let state = atoms(&conn, window, a._NET_WM_STATE)?;
                let extents = conn
                    .get_property(
                        false,
                        window,
                        a._NET_FRAME_EXTENTS,
                        AtomEnum::CARDINAL,
                        0,
                        4,
                    )?
                    .reply()?;
                let frame = Frame {
                    window,
                    custom: extents.value32().is_some_and(|values| values.eq([0; 4])),
                    full_screen: state.contains(&a._NET_WM_STATE_FULLSCREEN),
                    maximized: state.contains(&a._NET_WM_STATE_MAXIMIZED_HORZ)
                        && state.contains(&a._NET_WM_STATE_MAXIMIZED_VERT),
                };
                if previous != Some(frame) {
                    gesture.clear_click();
                    edges.show(&conn, frame.visible() && !frame.maximized, size)?;
                    app.update(move |s, _| {
                        if s.frame.window == window {
                            s.frame = frame;
                        }
                    });
                    previous = Some(frame);
                }
            }
            let event = conn.wait_for_event()?;
            state_changed = matches!(&event, Event::PropertyNotify(e) if e.atom == a._NET_WM_STATE || e.atom == a._NET_FRAME_EXTENTS);
            if matches!(&event, Event::DestroyNotify(e) if e.window == window) {
                return Ok(());
            }
            if let Event::ConfigureNotify(e) = &event
                && e.window == window
                && (e.width, e.height) != size
            {
                size = (e.width, e.height);
                edges.arrange(&conn, size)?;
            }
            if (previous.is_some_and(Frame::visible) || gesture.is_active())
                && gesture.handle(&conn, &edges, event)?
            {
                app.update(move |s, tx| {
                    if s.frame.window == window && s.frame.visible() {
                        tx.toggle_maximized();
                    }
                });
            }
        }
    };
    let result = run();
    if result.is_err() {
        if original.is_empty() {
            conn.delete_property(window, motif)?.ignore_error();
        } else {
            conn.change_property32(PropMode::REPLACE, window, motif, motif, &original)?
                .ignore_error();
        }
        conn.flush()?;
    }
    result
}

const fn glyph(name: &'static str, body: &'static str) -> Icon {
    Icon {
        name,
        view: 12.,
        body,
    }
}
const MINIMIZE: Icon = glyph("minimize", r#"<path d="M2 8h8v2H2z" fill="currentColor"/>"#);
const MAXIMIZE: Icon = glyph(
    "maximize",
    r#"<path d="M3 3h6v6H3z" fill="none" stroke="currentColor" stroke-width="2"/>"#,
);
const RESTORE: Icon = glyph(
    "restore",
    r#"<path d="M4 2h6v6M2 4h6v6H2z" fill="none" stroke="currentColor" stroke-width="1.5"/>"#,
);
const CLOSE: Icon = glyph(
    "close",
    r#"<path d="m3 3 6 6m0-6-6 6" fill="none" stroke="currentColor" stroke-width="2"/>"#,
);

pub(super) fn view(s: &Desktop, content: El, dark: bool) -> El {
    let root = el().w_full().h_full().child(content);
    if !s.frame.visible() {
        return root;
    }
    let unit = 1. / s.zoom.factor as f32;
    let ink = rune::color::hex(if dark { "#f8fafc" } else { "#1f2937" }).unwrap();
    let button = |id: &str, label: &str, symbol: &'static Icon, activate: fn(&mut Tx)| {
        el().key(id)
            .label(label)
            .role("button")
            .focusable(false)
            .no_window_drag()
            .size(24. * unit, 24. * unit)
            .radius(12. * unit)
            .center()
            .fill(ink.alpha(0.10))
            .hover(ink.alpha(0.10))
            .press(ink.alpha(0.18))
            .child(icon(symbol, 12. * unit).color(ink))
            .on_press_tx(move |_: &mut Desktop, tx| activate(tx))
    };
    let (name, symbol) = if s.frame.maximized {
        ("Restore", &RESTORE)
    } else {
        ("Maximize", &MAXIMIZE)
    };
    let controls = row()
        .pin(None, Some(8. * unit), Some(10. * unit), None)
        .gap(13. * unit)
        .child(button(
            "window-minimize",
            "Minimize",
            &MINIMIZE,
            Tx::minimize,
        ))
        .child(button(
            "window-maximize",
            name,
            symbol,
            Tx::toggle_maximized,
        ))
        .child(button("window-close", "Close", &CLOSE, Tx::close_window));
    root.child(controls)
}
