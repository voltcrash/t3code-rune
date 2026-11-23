use super::{Desktop, El, Panel, Tx, device, json, tok};
use rune::{Cursor, Key, Phase, el};
use std::time::{Duration, Instant};

const RAIL: f32 = 10.;
const KEYS_DELAY: Duration = Duration::from_millis(150);
const HANDLES: [(&str, i8, i8); 5] = [
    ("browser-resize-left", -1, 0),
    ("browser-resize-right", 1, 0),
    ("browser-resize-bottom", 0, 1),
    ("browser-resize-bottom-left", -1, 1),
    ("browser-resize-bottom-right", 1, 1),
];

#[derive(Clone, Copy)]
struct Layout {
    rect: [f32; 4],
    area: [f64; 2],
    scale: f64,
}

fn layout(area: [f32; 2], size: [u32; 2], zoom: f64) -> Layout {
    let [w, h] = [(area[0] - RAIL * 2.).max(1.), (area[1] - RAIL).max(1.)];
    let scale = zoom
        .min(f64::from(w) / f64::from(size[0]))
        .min(f64::from(h) / f64::from(size[1]));
    let [page_w, page_h] = size.map(|n| (f64::from(n) * scale) as f32);
    Layout {
        rect: [
            RAIL + ((w - page_w) / 2.).round(),
            ((h - page_h) / 2.).round(),
            page_w,
            page_h,
        ],
        area: [f64::from(w), f64::from(h)],
        scale,
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Drag,
    Keys(Instant),
    Sent,
}

#[derive(Clone, Copy)]
pub(super) struct Resize {
    edge: usize,
    mode: Mode,
    source: [u32; 2],
    start: [u32; 2],
    next: [u32; 2],
    layout: Layout,
}

impl Resize {
    pub(super) fn sent(self) -> bool {
        matches!(self.mode, Mode::Sent)
    }

    fn new(
        p: &Panel,
        edge: usize,
        mode: Mode,
        window_zoom: f64,
        visible: Option<[u32; 2]>,
    ) -> Option<Self> {
        let tab = p.page.tab()?;
        let size = tab.device.as_ref()?;
        let source = [size.width, size.height];
        let start = visible.unwrap_or(source);
        Some(Self {
            edge,
            mode,
            source,
            start,
            next: start,
            layout: layout(p.size, start, f64::from(tab.zoom) / 100. / window_zoom),
        })
    }

    fn drag(&mut self, delta: [f32; 2], limits: &device::Limits, ratio: Option<f64>) {
        let (_, x, y) = HANDLES[self.edge];
        let mut desired = self.start.map(f64::from);
        for (i, sign) in [x, y].into_iter().enumerate() {
            if sign == 0 {
                continue;
            }
            let scale = self.layout.scale;
            let available = self.layout.area[i];
            let start = desired[i] * scale;
            let travel = f64::from(delta[i]) * f64::from(sign);
            let end = if start < available {
                (available + start) / 2.
            } else {
                start
            };
            let target = end + travel;
            desired[i] = (if target <= available {
                target * 2. - available
            } else {
                target
            }) / scale;
        }
        let delta = [0, 1].map(|i| (desired[i] - f64::from(self.start[i])).abs());
        let width = if ratio.is_some() {
            x != 0
                && (y == 0
                    || delta[0] / f64::from(self.start[0]) >= delta[1] / f64::from(self.start[1]))
        } else {
            delta[0] >= delta[1]
        };
        self.next = limits.size(desired, ratio, width);
    }
}

impl Panel {
    pub(super) fn display_resize(&mut self, resize: Option<Resize>) {
        let size = resize.map(|r| r.next).or_else(|| {
            self.page
                .tab()?
                .device
                .as_ref()
                .map(|d| [d.width, d.height])
        });
        if let Some(size) = size {
            self.device.display(size);
        }
        self.resize = resize;
    }
}

impl Desktop {
    fn begin_browser_drag(&mut self, edge: usize, tx: &mut Tx) {
        let browser = &mut self.workspace.browser;
        let (Some(p), Some(labels)) = (&mut browser.panel, &browser.labels) else {
            return;
        };
        if !p.page.can_write || p.page.busy || p.resize.is_some_and(Resize::sent) {
            return;
        }
        let visible = p
            .device
            .take_size(&labels.device.limits)
            .or_else(|| p.resize.map(|r| r.next));
        p.display_resize(Resize::new(p, edge, Mode::Drag, self.zoom.factor, visible));
        tx.focus(HANDLES[edge].0, false);
    }

    fn browser_resize_key(&mut self, edge: usize, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        let browser = &mut self.workspace.browser;
        let (Some(p), Some(labels)) = (&mut browser.panel, &browser.labels) else {
            return false;
        };
        if ev.key == Key::Escape && p.resize.is_some_and(|r| !matches!(r.mode, Mode::Sent)) {
            self.cancel_browser_resize();
            tx.instant();
            return true;
        }
        if !p.page.can_write
            || p.page.busy
            || p.resize.is_some_and(|r| !matches!(r.mode, Mode::Keys(_)))
        {
            return false;
        }
        let (_, x, y) = HANDLES[edge];
        let (i, dir) = match ev.key {
            Key::Left if x != 0 => (0, -x),
            Key::Right if x != 0 => (0, x),
            Key::Up if y != 0 => (1, -y),
            Key::Down if y != 0 => (1, y),
            _ => return false,
        };
        let mode = Mode::Keys(Instant::now() + KEYS_DELAY);
        let Some(mut resize) = p
            .resize
            .or_else(|| Resize::new(p, edge, mode, self.zoom.factor, None))
        else {
            return false;
        };
        resize.mode = mode;
        resize.edge = edge;
        let mut desired = resize.next.map(f64::from);
        desired[i] += f64::from(dir) * if ev.mods.shift { 50. } else { 10. };
        resize.next = labels
            .device
            .limits
            .size(desired, p.device.aspect_ratio, i == 0);
        p.display_resize(Some(resize));
        tx.wake_after(KEYS_DELAY);
        true
    }

    pub(super) fn sync_browser_resize(&mut self, tx: &mut Tx) -> bool {
        let Some(p) = &mut self.workspace.browser.panel else {
            return false;
        };
        let Some(r) = p.resize else { return false };
        match r.mode {
            Mode::Keys(at) => {
                if let Some(remaining) = at.checked_duration_since(Instant::now()) {
                    tx.wake_after(remaining);
                    return false;
                }
                self.commit_browser_resize();
                true
            }
            Mode::Drag if tx.focused() != Some(HANDLES[r.edge].0) => {
                self.cancel_browser_resize();
                true
            }
            _ => false,
        }
    }

    fn cancel_browser_resize(&mut self) {
        let Some(p) = &mut self.workspace.browser.panel else {
            return;
        };
        if let Some(r) = &mut p.resize
            && matches!(r.mode, Mode::Drag)
            && r.start != r.source
        {
            r.next = r.start;
            self.commit_browser_resize();
        } else {
            p.display_resize(None);
        }
    }

    fn commit_browser_resize(&mut self) {
        let Some(p) = &mut self.workspace.browser.panel else {
            return;
        };
        let Some(mut r) = p.resize else { return };
        p.display_resize(None);
        let Some(tab) = p.page.tab() else { return };
        let Some(source) = &tab.device else { return };
        if r.next == r.source
            || !p.page.can_write
            || p.page.busy
            || [source.width, source.height] != r.source
        {
            return;
        }
        r.mode = Mode::Sent;
        let [width, height] = r.next;
        let request = json!({"action":"viewport","tab":tab.id,"from":{"width":source.width,"height":source.height,"presetId":source.preset_id},"change":{"kind":"edit",
            "edit":{"width":width,"height":height,"aspectRatio":null,"axis":"width"}}});
        p.display_resize(Some(r));
        self.browser_request(request);
    }
}

pub(super) fn canvas(
    s: &Desktop,
    mut page: El,
    p: &Panel,
    labels: &device::Labels,
    theme: &tok::Theme,
) -> El {
    let container =
        el().w_full()
            .grow(1.)
            .min_h(0.)
            .clip_rounded(0.)
            .on_layout(|s: &mut Desktop, r| {
                if let Some(p) = &mut s.workspace.browser.panel {
                    p.size = [r[2], r[3]];
                    s.resize_browser();
                }
            });
    let Some(tab) = p.page.tab() else {
        return container.child(page.w_full().h_full());
    };
    let Some(source) = &tab.device else {
        return container.child(page.w_full().h_full());
    };
    let size = p.resize.map_or([source.width, source.height], |r| r.next);
    let [left, top, width, height] =
        layout(p.size, size, f64::from(tab.zoom) / 100. / s.zoom.factor).rect;
    page = page.absolute(left, top).size(width, height);
    let mut area = container.fill(theme.bg).child(page);
    if !p.page.can_write || p.page.dialog.is_some() {
        return area;
    }
    for (edge, (key, x, y)) in HANDLES.iter().copied().enumerate() {
        let corner = x != 0 && y != 0;
        let px = match x {
            -1 => left - RAIL,
            1 => left + width,
            _ => left,
        };
        let py = if y == 0 { top } else { top + height };
        let (w, h) = (
            if x == 0 { width } else { RAIL },
            if y == 0 { height } else { RAIL },
        );
        let ink = if p.resize.is_some_and(|r| r.edge == edge) {
            theme.text
        } else {
            theme.text_secondary
        };
        let stroke = |w, h| el().size(w, h).fill(ink).radius(0.5);
        let grip = if corner {
            let angle = if x > 0 {
                -std::f32::consts::FRAC_PI_4
            } else {
                std::f32::consts::FRAC_PI_4
            };
            el().size(12., 12.)
                .child(stroke(12., 1.).absolute(0., 8.).rotate(angle))
                .child(
                    stroke(8., 1.)
                        .absolute(if x > 0 { 5. } else { -1. }, 11.)
                        .rotate(angle),
                )
        } else if x == 0 {
            rune::col()
                .gap(1.)
                .child(stroke(24., 1.))
                .child(stroke(24., 1.))
        } else {
            rune::row()
                .gap(1.)
                .child(stroke(1., 24.))
                .child(stroke(1., 24.))
        };
        let handle = el()
            .key(key)
            .role("button")
            .label(&labels.handles[edge])
            .help(&labels.handles[edge])
            .absolute(px, py)
            .size(w, h)
            .center()
            .child(grip);
        let handle = handle
            .focusable(true)
            .cursor(if corner {
                Cursor::Crosshair
            } else if x == 0 {
                Cursor::ResizeRow
            } else {
                Cursor::ResizeColumn
            })
            .hover(theme.hover)
            .on_key(move |s: &mut Desktop, ev, tx| s.browser_resize_key(edge, ev, tx))
            .on_drag(move |s: &mut Desktop, d, tx| {
                if d.phase == Phase::Down {
                    s.begin_browser_drag(edge, tx);
                    return;
                }
                let browser = &mut s.workspace.browser;
                let (Some(p), Some(labels)) = (&mut browser.panel, &browser.labels) else {
                    return;
                };
                let Some(mut r) = p.resize else { return };
                if !matches!(r.mode, Mode::Drag) || r.edge != edge {
                    return;
                }
                if d.cancelled() || tx.focused() != Some(key) {
                    s.cancel_browser_resize();
                    return;
                }
                r.drag([d.dx, d.dy], &labels.device.limits, p.device.aspect_ratio);
                p.display_resize(Some(r));
                tx.instant();
                if d.phase == Phase::Up {
                    s.commit_browser_resize();
                }
            });
        area = area.child(handle);
    }
    area
}
