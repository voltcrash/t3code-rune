use crate::{Desktop, Request};
use rune::{Cursor, El, Phase, el};

const DEFAULT_WIDTH: f32 = 256.;
const RAIL: &str = "sidebar-resize";

#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub open: bool,
    pub width: Option<u32>,
    dragging: Option<f32>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            open: true,
            width: None,
            dragging: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    pub min: f32,
    pub max: f32,
}

impl Bounds {
    pub fn new(viewport: f32, leading: f32) -> Self {
        let min = (leading + 174.).ceil().max(208.);
        Self {
            min,
            max: (viewport.floor() - 640.).max(min),
        }
    }

    pub fn shown(self, layout: Layout) -> f32 {
        layout
            .dragging
            .or(layout.width.map(|w| w as f32))
            .unwrap_or(DEFAULT_WIDTH)
            .clamp(self.min, self.max)
    }
}

impl Layout {
    pub fn cancel_unfocused_drag(&mut self, tx: &rune::Tx) -> bool {
        if self.dragging.is_some() && tx.focused() != Some(RAIL) {
            self.end_drag();
            return true;
        }
        false
    }

    pub fn end_drag(&mut self) {
        self.dragging = None;
    }
}

impl Desktop {
    fn save_sidebar_width(&mut self, width: Option<u32>) {
        self.sidebar_layout.width = width;
        self.sidebar_layout.end_drag();
        self.write_request(None, Request::SetSidebarWidth { width });
    }
}

pub(crate) fn rail(shown: f32, bounds: Bounds) -> El {
    el().key(RAIL)
        .role("slider")
        .label("Resize sidebar")
        .help("Drag to resize sidebar")
        .focusable(true)
        .range_value(shown.into(), bounds.min.into(), bounds.max.into())
        .range_step(16.)
        .on_step(move |s: &mut Desktop, steps, _| {
            let next = (shown + steps as f32 * 16.).clamp(bounds.min, bounds.max);
            if next != shown {
                s.save_sidebar_width(Some(next as u32));
            }
        })
        .cursor(Cursor::ResizeColumn)
        .no_window_drag()
        .absolute(shown - 4., 0.)
        .w(9.)
        .h_full()
        .on_double_press(|s: &mut Desktop| s.save_sidebar_width(None))
        .on_drag(move |s: &mut Desktop, drag, tx| match drag.phase {
            Phase::Down => {
                drag.data = shown;
                s.sidebar_layout.dragging = Some(shown);
                tx.focus(RAIL, false);
            }
            Phase::Move if s.sidebar_layout.dragging.is_some() && tx.focused() == Some(RAIL) => {
                s.sidebar_layout.dragging =
                    Some((drag.data + drag.dx).round().clamp(bounds.min, bounds.max));
                tx.instant();
            }
            Phase::Move => {}
            Phase::Up => {
                if let Some(width) = s.sidebar_layout.dragging.take()
                    && tx.focused() == Some(RAIL)
                    && !drag.cancelled()
                    && width != drag.data
                {
                    s.save_sidebar_width(Some(width as u32));
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, Env, Runtime};

    #[test]
    fn losing_the_divider_during_a_drag_restores_the_committed_width() {
        fn view(s: &Desktop, _: &mut Cx<Desktop>) -> El {
            let bounds = Bounds::new(1100., 0.);
            let mut root = el()
                .w_full()
                .h_full()
                .child(el().key("other").size(20., 20.).focusable(true));
            if s.sidebar_layout.open {
                root = root.child(rail(bounds.shown(s.sidebar_layout), bounds));
            }
            if s.settings_open {
                root = root.child(
                    el().key("dialog")
                        .size(100., 100.)
                        .focusable(true)
                        .modal(true, |_: &mut Desktop| {}),
                );
            }
            root
        }
        for interruption in ["modal", "unmount", "refocus"] {
            let mut rt = Runtime::new(Desktop::new(), view, (1100., 700.), Env::default());
            rt.poll = Some(|s, _, tx| s.sidebar_layout.cancel_unfocused_drag(tx));
            rt.frame(0.);
            rt.pointer_down(256., 350.);
            rt.frame(0.);
            rt.pointer_move(330., 350.);
            rt.frame(0.);
            let bounds = Bounds::new(1100., 0.);
            assert_eq!(bounds.shown(rt.state.sidebar_layout), 330.);
            rt.update(|s, tx| {
                if interruption == "modal" {
                    s.settings_open = true;
                    tx.focus("dialog", true);
                } else if interruption == "unmount" {
                    s.sidebar_layout.open = false;
                } else {
                    tx.focus("other", true);
                }
            });
            rt.frame(0.);
            if interruption == "refocus" {
                rt.frame(0.);
                rt.update(|_, tx| tx.focus(RAIL, true));
                rt.frame(0.);
                rt.pointer_move(310., 350.);
                rt.frame(0.);
            }
            rt.pointer_up(310., 350.);
            rt.frame(0.);
            assert_eq!(bounds.shown(rt.state.sidebar_layout), DEFAULT_WIDTH);
            assert_eq!(rt.state.sidebar_layout.width, None);
        }
    }
}
