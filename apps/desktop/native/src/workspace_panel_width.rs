use super::{Desktop, Request};
use rune::{Cursor, El, Phase, Tx, el};
use serde::Deserialize;
use std::collections::HashMap;

const MIN: f32 = 360.;
const DEFAULT: f32 = 540.;
const RAIL: &str = "right-panel-resize";

#[derive(Default, Deserialize)]
pub(crate) struct Widths {
    #[serde(default)]
    widths: HashMap<String, u32>,
    #[serde(default)]
    resize: String,
    #[serde(skip)]
    drag: Option<(u64, f32)>,
}

impl Widths {
    pub(super) fn shown(&self, s: &Desktop, max: f32) -> f32 {
        self.drag
            .filter(|(watch, _)| s.workspace.terminal_watch() == Some(*watch))
            .map(|(_, width)| width)
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

pub(super) fn max_width(viewport: f32, available: f32) -> f32 {
    (viewport * 0.7)
        .floor()
        .min(available.floor() - MIN)
        .max(MIN)
}

impl Desktop {
    pub(crate) fn cancel_panel_resize(&mut self, tx: &Tx) -> bool {
        let drag = self.workspace.panel_width.drag;
        if drag.is_some_and(|(watch, _)| {
            self.workspace.terminal_watch() != Some(watch) || tx.focused() != Some(RAIL)
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
            .on_step(move |s: &mut Desktop, steps, _| {
                let width = (shown + steps as f32 * 16.).clamp(MIN, max);
                if s.workspace.panel_width.drag.is_none() && width != shown {
                    s.save_panel_width(watch, width);
                }
            })
            .on_drag(move |s: &mut Desktop, drag, tx| match drag.phase {
                Phase::Down => {
                    drag.data = shown;
                    s.workspace.panel_width.drag = Some((watch, shown));
                    tx.focus(RAIL, false);
                }
                Phase::Move
                    if s.workspace
                        .panel_width
                        .drag
                        .is_some_and(|(w, _)| w == watch)
                        && tx.focused() == Some(RAIL) =>
                {
                    s.workspace.panel_width.drag =
                        Some((watch, (drag.data - drag.dx).round().clamp(MIN, max)));
                    tx.instant();
                }
                Phase::Move => {}
                Phase::Up => {
                    if let Some((w, width)) = s.workspace.panel_width.drag.take()
                        && w == watch
                        && !drag.cancelled()
                        && tx.focused() == Some(RAIL)
                        && width != drag.data
                    {
                        s.save_panel_width(watch, width.clamp(MIN, max));
                    }
                }
            }),
    )
}
