use super::{Result, edges::Edges};
use std::time::{Duration, Instant};
use x11rb::{
    connection::Connection,
    protocol::{Event, xinput, xproto::*},
    rust_connection::RustConnection,
};

const MOVE: u32 = 8;
const CANCEL: u32 = 11;

struct Press {
    origin: [i32; 2],
    direction: u32,
    motion: [f64; 2],
    outer: Option<(Window, [i32; 2])>,
    dragged: bool,
}

impl Press {
    fn new(origin: [i32; 2], direction: u32) -> Self {
        Self {
            origin,
            direction,
            motion: [0.; 2],
            outer: None,
            dragged: false,
        }
    }

    fn motion(&mut self, event: &xinput::RawMotionEvent, slop: u32) {
        let mask = event.valuator_mask.first().copied().unwrap_or(0);
        let mut values = event.axisvalues.iter();
        for (axis, displacement) in self.motion.iter_mut().enumerate() {
            if mask & (1 << axis) != 0
                && let Some(delta) = values.next()
            {
                *displacement += f64::from(delta.integral) + f64::from(delta.frac) / 4294967296.;
                self.dragged |= displacement.abs() > f64::from(slop);
            }
        }
    }
}

pub(super) struct Gesture {
    root: Window,
    window: Window,
    moveresize: Atom,
    active: Option<Press>,
    pointer: xinput::DeviceId,
    slop: u32,
    last_click: Option<(Instant, [i32; 2])>,
}

impl Gesture {
    pub(super) fn new(
        conn: &RustConnection,
        root: Window,
        window: Window,
        moveresize: Atom,
        scale: f64,
    ) -> Result<Self> {
        let version = xinput::xi_query_version(conn, 2, 1)?.reply()?;
        if (version.major_version, version.minor_version) < (2, 1) {
            return Err("XInput 2.1 is required for resize handoff".into());
        }
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
        )?
        .check()?;
        let pointer = xinput::xi_get_client_pointer(conn, window)?
            .reply()?
            .deviceid;
        Ok(Self {
            pointer,
            slop: (4. * scale).round().max(1.) as u32,
            root,
            window,
            moveresize,
            active: None,
            last_click: None,
        })
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn clear_click(&mut self) {
        self.last_click = None;
    }

    fn listen(&self, conn: &RustConnection, mask: Option<xinput::XIEventMask>) -> Result<()> {
        let mask = xinput::EventMask {
            deviceid: self.pointer,
            mask: mask.into_iter().collect(),
        };
        xinput::xi_select_events(conn, self.root, &[mask])?.check()?;
        Ok(())
    }

    fn request(&self, conn: &RustConnection, [x, y]: [i32; 2], direction: u32) -> Result<()> {
        conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ClientMessageEvent::new(
                32,
                self.window,
                self.moveresize,
                [x as u32, y as u32, direction, 1, 1],
            ),
        )?;
        Ok(conn.flush()?)
    }

    pub(super) fn handle(
        &mut self,
        conn: &RustConnection,
        edges: &Edges,
        event: Event,
    ) -> Result<bool> {
        let release = match event {
            Event::ButtonPress(e) if e.detail == 1 => {
                let Some(direction) = edges.direction(e.event) else {
                    return Ok(false);
                };
                let origin = [i32::from(e.root_x), i32::from(e.root_y)];
                self.listen(conn, Some(xinput::XIEventMask::RAW_BUTTON_RELEASE))?;
                self.active = Some(Press::new(origin, direction));
                conn.ungrab_pointer(e.time)?;
                self.request(conn, origin, direction)?;
                false
            }
            // The manager's grab can reset Rune's click count. Pair only completed title-bar clicks.
            Event::ClientMessage(e)
                if e.window == self.window
                    && e.type_ == self.moveresize
                    && e.format == 32
                    && e.data.as_data32()[2..4] == [MOVE, 1] =>
            {
                let data = e.data.as_data32();
                self.listen(
                    conn,
                    Some(xinput::XIEventMask::RAW_BUTTON_RELEASE | xinput::XIEventMask::RAW_MOTION),
                )?;
                let mut frame = self.window;
                loop {
                    let parent = conn.query_tree(frame)?.reply()?.parent;
                    if parent == self.root {
                        break;
                    }
                    frame = parent;
                }
                let bounds = conn.get_geometry(frame)?.reply()?;
                let mut press = Press::new([data[0] as i32, data[1] as i32], MOVE);
                press.outer = Some((frame, [i32::from(bounds.x), i32::from(bounds.y)]));
                self.active = Some(press);
                false
            }
            // A pointer warp can move the manager's frame without producing raw device motion.
            Event::ConfigureNotify(e) => {
                if let Some(press) = &mut self.active
                    && let Some((frame, [x, y])) = press.outer
                    && e.window == frame
                {
                    press.dragged |= x.abs_diff(i32::from(e.x)) > self.slop
                        || y.abs_diff(i32::from(e.y)) > self.slop;
                }
                return Ok(false);
            }
            Event::XinputRawMotion(e) => {
                if let Some(press) = &mut self.active
                    && !press.dragged
                {
                    press.motion(&e, self.slop);
                    if press.dragged {
                        self.listen(conn, Some(xinput::XIEventMask::RAW_BUTTON_RELEASE))?;
                    }
                }
                return Ok(false);
            }
            Event::XinputRawButtonRelease(e) if e.detail == 1 && e.deviceid == self.pointer => true,
            _ => return Ok(false),
        };
        if !release {
            // The button may already be up when the subscription starts.
            let pointer = conn.query_pointer(self.root)?.reply()?;
            if let Some(press) = &mut self.active {
                press.dragged |= press.origin[0].abs_diff(i32::from(pointer.root_x)) > self.slop
                    || press.origin[1].abs_diff(i32::from(pointer.root_y)) > self.slop;
            }
            if pointer.mask.contains(KeyButMask::BUTTON1) {
                return Ok(false);
            }
        }
        let Some(press) = self.active.take() else {
            return Ok(false);
        };
        self.listen(conn, None)?;
        // A release delivered to the manager precedes this cancellation in its X11 event queue.
        self.request(conn, press.origin, CANCEL)?;
        let start = press.origin;
        let last = self.last_click.take();
        if press.direction != MOVE || press.dragged {
            return Ok(false);
        }
        if last.is_some_and(|(time, p)| {
            time.elapsed() <= Duration::from_millis(500)
                && start[0].abs_diff(p[0]) <= self.slop
                && start[1].abs_diff(p[1]) <= self.slop
        }) {
            return Ok(true);
        }
        self.last_click = Some((Instant::now(), start));
        Ok(false)
    }
}
