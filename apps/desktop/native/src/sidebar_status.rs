use super::Thread;
use rune::{Color, El, Icon, el, icon, row, tok};
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum RowStatus {
    Working,
    Goal,
    Preparing,
    Waiting,
    Approval,
    Input,
    Request,
    Limited,
    Failed,
    Done,
    #[default]
    #[serde(other)]
    Ready,
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn relative_age(seconds: i64) -> String {
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

fn working_duration(seconds: i64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
    }
}

impl RowStatus {
    fn timed(self) -> bool {
        matches!(
            self,
            Self::Working | Self::Goal | Self::Preparing | Self::Waiting
        )
    }

    fn badge(self, dark: bool, theme: &tok::Theme) -> Option<(&'static str, &'static Icon, Color)> {
        let pair = |light, night| if dark { night } else { light };
        let blue = pair(
            rune::oklch(0.488, 0.243, 264.376),
            rune::oklch(0.707, 0.165, 254.624),
        );
        let amber = pair(
            rune::oklch(0.555, 0.163, 48.998),
            rune::oklch(0.828, 0.189, 84.429),
        );
        Some(match self {
            Self::Ready => return None,
            Self::Preparing => ("Setting up", &WORKING, blue),
            Self::Working => ("Working", &WORKING, blue),
            Self::Goal => ("Goal", &WORKING, blue),
            Self::Waiting => ("Waiting", &WAITING, theme.text_secondary),
            Self::Approval => ("Approval", &APPROVAL, amber),
            Self::Input => (
                "Input",
                &INPUT,
                pair(
                    rune::oklch(0.511, 0.262, 276.966),
                    rune::oklch(0.785, 0.115, 274.713),
                ),
            ),
            Self::Request => (
                "Request",
                &REQUEST,
                pair(
                    rune::oklch(0.591, 0.293, 322.896),
                    rune::oklch(0.833, 0.145, 321.434),
                ),
            ),
            Self::Limited => ("Limited", &FAILED, amber),
            Self::Failed => (
                "Failed",
                &FAILED,
                pair(
                    rune::oklch(0.505, 0.213, 27.518),
                    rune::oklch(0.704, 0.191, 22.216),
                ),
            ),
            Self::Done => (
                "Done",
                &DONE,
                pair(
                    rune::oklch(0.508, 0.118, 165.612),
                    rune::oklch(0.765, 0.177, 163.223),
                ),
            ),
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Indicator {
    status: RowStatus,
    status_at: Option<i64>,
}

impl Thread {
    pub(super) fn indicator(&self, shelf: bool) -> Indicator {
        if shelf {
            Indicator {
                status: RowStatus::Ready,
                status_at: (self.sort_at > 0).then_some(self.sort_at),
            }
        } else {
            Indicator {
                status: self.status,
                status_at: self.status_at,
            }
        }
    }
}

impl Indicator {
    fn local_start(&self, offset: i64) -> Option<i64> {
        self.status_at
            .map(|at| if self.status.timed() { at - offset } else { at })
    }
    fn tick_size(&self) -> i64 {
        if self.status.timed() { 1000 } else { 60_000 }
    }

    fn elapsed_ms(&self, now: i64, at: i64) -> i64 {
        let tick = self.tick_size();
        (now.div_euclid(tick) * tick - at).max(0)
    }

    fn next_label_change(&self, now: i64, offset: i64) -> Option<i64> {
        let at = self.local_start(offset)?;
        if !self.status.timed() && self.status != RowStatus::Ready {
            return None;
        }
        let age = self.elapsed_ms(now, at);
        let unit = match age {
            0..60_000 if self.status.timed() => 1000,
            _ if self.status.timed() || age < 3_600_000 => 60_000,
            3_600_000..86_400_000 => 3_600_000,
            _ => 86_400_000,
        };
        let next = at + (age / unit + 1) * unit;
        let tick = self.tick_size();
        Some((next + tick - 1).div_euclid(tick) * tick)
    }
}

pub(super) fn view(
    indicator: Indicator,
    dark: bool,
    theme: &tok::Theme,
    clock: &Clock,
) -> (El, Option<&'static str>) {
    let elapsed = indicator
        .local_start(clock.offset)
        .map(|t| indicator.elapsed_ms(clock.now, t) / 1000);
    let Some((name, glyph, color)) = indicator.status.badge(dark, theme) else {
        return (
            elapsed.map_or_else(el, |seconds| {
                super::label(&relative_age(seconds), 12., 400., theme.text_secondary)
            }),
            None,
        );
    };
    let mut text = name.to_owned();
    if indicator.status.timed()
        && let Some(seconds) = elapsed
    {
        text.push(' ');
        text.push_str(&working_duration(seconds));
    }
    (
        row()
            .center_y()
            .gap(4.)
            .child(icon(glyph, 16.).color(color))
            .child(super::label(&text, 12., 500., color)),
        Some(name),
    )
}

pub(super) struct Clock {
    pub mounted: Cell<bool>,
    pub viewport: rune::Live<[f32; 4]>,
    rows: RefCell<HashMap<String, Weak<rune::Live<[f32; 4]>>>>,
    now: i64,
    offset: i64,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            mounted: Default::default(),
            viewport: rune::Live::new([0.; 4]),
            rows: Default::default(),
            now: unix_millis(),
            offset: 0,
        }
    }
}

fn intersects(row: [f32; 4], viewport: [f32; 4]) -> bool {
    row[3] > 0.
        && viewport[3] > 0.
        && row[1] < viewport[1] + viewport[3]
        && row[1] + row[3] > viewport[1]
}

impl Clock {
    pub(super) fn track_row(&self, thread: &Thread, indicator: Indicator, row: El) -> El {
        if indicator.next_label_change(self.now, self.offset).is_none() {
            return row;
        }
        let mut rows = self.rows.borrow_mut();
        let bounds = rows
            .get(&thread.id)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let bounds = Rc::new(rune::Live::new([0.; 4]));
                rows.insert(thread.id.clone(), Rc::downgrade(&bounds));
                bounds
            });
        row.drawn_rect_into(&bounds)
            .on_visible(move |s: &mut super::Desktop, on, tx| {
                if on && intersects(bounds.get(), s.workspace.row_clock.viewport.get()) {
                    tx.wake_after(std::time::Duration::ZERO);
                }
                tx.live_only();
            })
    }
}
impl super::Workspace {
    pub(crate) fn sidebar_viewport(&self, list: El) -> El {
        list.drawn_rect_into(&self.row_clock.viewport)
            .on_drawn_layout_tx(|s: &mut super::Desktop, _, tx| {
                if s.workspace.providers.viewport_changed() {
                    s.workspace.refresh_provider_rows();
                } else {
                    tx.live_only();
                }
            })
    }

    pub(crate) fn sidebar_intersects(&self, bounds: [f32; 4]) -> bool {
        let viewport = self.row_clock.viewport.get();
        intersects(bounds, viewport)
            && bounds[2] > 0.
            && viewport[2] > 0.
            && bounds[0] < viewport[0] + viewport[2]
            && bounds[0] + bounds[2] > viewport[0]
    }

    pub(super) fn can_advance_to(&self, thread: &Thread) -> bool {
        !thread.settled
            && thread
                .snoozed_until
                .is_none_or(|until| until <= unix_millis().saturating_add(self.row_clock.offset))
    }

    pub(crate) fn set_clock_offset(&mut self, offset: i64) {
        self.row_clock.offset = offset;
        self.row_clock.now = unix_millis();
    }
    pub(crate) fn poll_sidebar_clock(&mut self, tx: &mut rune::Tx) -> bool {
        if !self.row_clock.mounted.get() {
            return false;
        }
        let now = unix_millis();
        let (changed, deadline) = self.sidebar_clock_deadline(now);
        if let Some(due) = deadline {
            tx.wake_after(std::time::Duration::from_millis((due - now).max(1) as u64));
        }
        if changed {
            self.row_clock.now = now;
        }
        changed
    }

    fn sidebar_clock_deadline(&self, now: i64) -> (bool, Option<i64>) {
        let mut deadline = None;
        let mut changed = false;
        let inbox = self.search.text.trim().is_empty();
        let viewport = self.row_clock.viewport.get();
        self.row_clock.rows.borrow_mut().retain(|id, weak| {
            let Some(bounds) = weak.upgrade() else {
                return false;
            };
            if intersects(bounds.get(), viewport)
                && let Some(&index) = self.by_thread.get(id)
            {
                let thread = &self.threads[index];
                let indicator = thread.indicator(thread.settled && inbox);
                if let Some(next) = indicator.next_label_change(now, self.row_clock.offset) {
                    deadline = Some(deadline.map_or(next, |earlier: i64| earlier.min(next)));
                    changed |= now < self.row_clock.now
                        || indicator
                            .next_label_change(self.row_clock.now, self.row_clock.offset)
                            .is_some_and(|due| due <= now);
                }
            }
            true
        });
        (changed, deadline)
    }
}

const WORKING: Icon = Icon {
    name: "sidebar-working",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.1 2.182a10 10 0 0 1 3.8 0" /><path d="M13.9 21.818a10 10 0 0 1-3.8 0" /><path d="M17.609 3.721a10 10 0 0 1 2.69 2.7" /><path d="M2.182 13.9a10 10 0 0 1 0-3.8" /><path d="M20.279 17.609a10 10 0 0 1-2.7 2.69" /><path d="M21.818 10.1a10 10 0 0 1 0 3.8" /><path d="M3.721 6.391a10 10 0 0 1 2.7-2.69" /><path d="M6.391 20.279a10 10 0 0 1-2.69-2.7" /></g>"#,
};
const WAITING: Icon = Icon {
    name: "sidebar-waiting",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.1 2.18a9.93 9.93 0 0 1 3.8 0" /><path d="M17.6 3.71a9.95 9.95 0 0 1 2.69 2.7" /><path d="M21.82 10.1a9.93 9.93 0 0 1 0 3.8" /><path d="M20.29 17.6a9.95 9.95 0 0 1-2.7 2.69" /><path d="M13.9 21.82a9.94 9.94 0 0 1-3.8 0" /><path d="M6.4 20.29a9.95 9.95 0 0 1-2.69-2.7" /><path d="M2.18 13.9a9.93 9.93 0 0 1 0-3.8" /><path d="M3.71 6.4a9.95 9.95 0 0 1 2.7-2.69" /><circle cx="12" cy="12" r="1" /></g>"#,
};
const INPUT: Icon = Icon {
    name: "sidebar-input",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></g>"#,
};
const REQUEST: Icon = Icon {
    name: "sidebar-request",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8" r="5" /><path d="M20 21a8 8 0 0 0-16 0" /></g>"#,
};
const APPROVAL: Icon = Icon {
    name: "sidebar-approval",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" /><path d="M9.1 9a3 3 0 0 1 5.82 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></g>"#,
};
const FAILED: Icon = Icon {
    name: "sidebar-failed",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><line x1="12" x2="12" y1="8" y2="12" /><line x1="12" x2="12.01" y1="16" y2="16" /></g>"#,
};
const DONE: Icon = Icon {
    name: "sidebar-done",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><path d="m9 12 2 2 4-4" /></g>"#,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_between_active_and_settled_rows_ticks_only_visible_labels() {
        use crate::Desktop;
        use rune::{Cx, Env, Runtime, ScrollSpec, col};

        fn view(s: &Desktop, _: &mut Cx<Desktop>) -> El {
            let ws = &s.workspace;
            let row = |id: &str| {
                let thread = &ws.threads[ws.by_thread[id]];
                ws.row_clock.track_row(
                    thread,
                    thread.indicator(thread.settled),
                    el().key(id).w_full().h(50.),
                )
            };
            col()
                .key("viewport")
                .w_full()
                .h_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .drawn_rect_into(&ws.row_clock.viewport)
                .child(row("active"))
                .child(el().w_full().h(270.))
                .child(row("settled"))
        }

        let mut app = Desktop::new();
        app.workspace.replace(
            vec![],
            ["active", "settled"]
                .map(|id| {
                    serde_json::from_value(serde_json::json!({
                        "id":id,"project":"p","title":id,"model":"test","status_at":60_000,
                        "status":"working", "settled":id=="settled", "sort_at":60_000
                    }))
                    .unwrap()
                })
                .into(),
        );
        let mut rt = Runtime::new(app, view, (200., 200.), Env::default());
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(
            rt.state.workspace.sidebar_clock_deadline(61_000).1,
            Some(62_000)
        );
        rt.update(|_, tx| tx.scroll_to_bottom("viewport"));
        let mut saw_gap = false;
        let mut saw_settled = false;
        for _ in 0..120 {
            rt.frame(1. / 120.);
            let elements = rt.elements();
            let visible = |id| {
                elements.iter().any(|el| {
                    el.key.as_deref() == Some(id) && intersects(el.rect, [0., 0., 200., 200.])
                })
            };
            let expected = if visible("active") {
                Some(62_000)
            } else if visible("settled") {
                Some(120_000)
            } else {
                None
            };
            assert_eq!(
                rt.state.workspace.sidebar_clock_deadline(61_000).1,
                expected
            );
            saw_gap |= expected.is_none();
            saw_settled |= expected == Some(120_000);
        }
        assert!(saw_gap && saw_settled);
    }
}
