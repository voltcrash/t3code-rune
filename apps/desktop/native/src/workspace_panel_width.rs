use super::{Desktop, Request};
use rune::{Cursor, El, Key, Phase, Tx, el};
use serde::Deserialize;
use std::collections::HashMap;

const MIN: f32 = 360.;
const DEFAULT: f32 = 540.;
const RAIL: &str = "right-panel-resize";

#[derive(Clone, Copy)]
struct Drag {
    watch: u64,
    width: f32,
    start_max: f32,
}

#[derive(Default, Deserialize)]
pub(crate) struct Widths {
    #[serde(default)]
    widths: HashMap<String, u32>,
    #[serde(default)]
    resize: String,
    #[serde(skip)]
    drag: Option<Drag>,
}

impl Widths {
    pub(super) fn shown(&self, s: &Desktop, max: f32) -> f32 {
        self.drag
            .filter(|d| s.workspace.terminal_watch() == Some(d.watch))
            .map(|d| d.width)
            .or_else(|| {
                s.workspace
                    .visible_thread()
                    .and_then(|t| self.widths.get(&t.id))
                    .map(|n| *n as f32)
            })
            .unwrap_or(DEFAULT)
            .clamp(MIN, max)
    }
}

pub(super) fn sidebar_cap(viewport: f32) -> f32 {
    viewport.floor() - 2. * MIN - 1.
}

pub(super) fn max_width(viewport: f32, available: f32) -> f32 {
    (viewport * 0.7)
        .floor()
        .min(available.floor() - MIN - 1.)
        .max(MIN)
}

impl Desktop {
    pub(crate) fn cancel_panel_resize(&mut self, tx: &Tx) -> bool {
        let drag = self.workspace.panel_width.drag;
        if drag.is_some_and(|d| {
            self.workspace.terminal_watch() != Some(d.watch) || tx.focused() != Some(RAIL)
        }) {
            self.workspace.panel_width.drag = None;
            return true;
        }
        false
    }

    fn save_panel_width(&mut self, watch: u64, width: f32) {
        if self.workspace.terminal_watch() != Some(watch) {
            return;
        }
        let Some(thread) = self.workspace.visible_thread().map(|t| t.id.clone()) else {
            return;
        };
        let Some(feed) = self.workspace.feed else {
            return;
        };
        let width = width as u32;
        self.workspace
            .panel_width
            .widths
            .insert(thread.clone(), width);
        self.write_request(
            None,
            Request::SetPanelWidth {
                watch: feed,
                thread: &thread,
                width,
            },
        );
    }
}

pub(super) fn rail(s: &Desktop, shown: f32, max: f32, edge: f32) -> Option<El> {
    let watch = s.workspace.terminal_watch()?;
    let resize = move |s: &mut Desktop, width: f32| {
        let width = width.clamp(MIN, max);
        if s.workspace.panel_width.drag.is_none() && width != shown {
            s.save_panel_width(watch, width);
        }
    };
    Some(
        el().key(RAIL)
            .role("slider")
            .label(&s.workspace.panel_width.resize)
            .focusable(true)
            .range_value(shown.into(), MIN.into(), max.into())
            .range_step(16.)
            .cursor(Cursor::ResizeColumn)
            .no_window_drag()
            .absolute(edge - 4., 0.)
            .w(9.)
            .h_full()
            .on_key(move |s: &mut Desktop, ev, _| {
                if ev.mods != rune::Mods::default() {
                    return false;
                }
                let delta = match ev.key {
                    Key::Left => 16.,
                    Key::Right => -16.,
                    _ => return false,
                };
                resize(s, shown + delta);
                true
            })
            .on_step(move |s: &mut Desktop, steps, _| resize(s, shown + steps as f32 * 16.))
            .on_step_limit(move |s: &mut Desktop, end, _| resize(s, if end { max } else { MIN }))
            .on_drag(move |s: &mut Desktop, drag, tx| match drag.phase {
                Phase::Down => {
                    drag.data = shown;
                    s.workspace.panel_width.drag = Some(Drag {
                        watch,
                        width: shown,
                        start_max: max,
                    });
                    tx.focus(RAIL, false);
                }
                Phase::Move => {
                    if let Some(d) = &mut s.workspace.panel_width.drag
                        && d.watch == watch
                        && tx.focused() == Some(RAIL)
                    {
                        d.width = (drag.data - drag.dx).round().clamp(MIN, d.start_max);
                        tx.instant();
                    }
                }
                Phase::Up => {
                    if let Some(d) = s.workspace.panel_width.drag.take()
                        && d.watch == watch
                        && !drag.cancelled()
                        && tx.focused() == Some(RAIL)
                        && d.width != drag.data
                    {
                        s.save_panel_width(watch, d.width);
                    }
                }
            }),
    )
}
