use crate::{Desktop, Request};
use rune::{Color, El, Icon, Key, KeyEvent, TextStyle, Tx, col, el, icon, text, tok};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const ICON: Icon = Icon {
    name: "terminal",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m6 9 3 3-3 3m6 0h4"/></g>"#,
};
const CW: f32 = 7.23;
const CH: f32 = 16.;

#[derive(Serialize)]
#[serde(
    tag = "action",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Input {
    Open {
        cols: u16,
        rows: u16,
        dark: bool,
    },
    Resize {
        cols: u16,
        rows: u16,
        dark: bool,
    },
    Close,
    Handled,
    Select {
        phase: &'static str,
        x: i32,
        y: i32,
        scroll: i32,
        clicks: u32,
        cancelled: bool,
    },
    SelectAll,
    Copy {
        request: u64,
        interrupt_if_empty: bool,
    },
    Paste {
        text: String,
    },
    Scroll {
        lines: i32,
    },
    Key {
        key: String,
        code: String,
        shift_key: bool,
        ctrl_key: bool,
        alt_key: bool,
        meta_key: bool,
        repeat: bool,
        printable: bool,
    },
}
#[derive(Deserialize)]
struct Run {
    x: u16,
    width: u16,
    text: String,
    fg: u32,
    bg: u32,
    style: u8,
}
#[derive(Deserialize)]
pub(crate) struct Frame {
    rows: usize,
    background: u32,
    status: String,
    error: Option<String>,
    cursor: Option<[u32; 4]>,
    patches: Vec<(usize, Vec<Run>)>,
}
pub(crate) struct Panel {
    watch: u64,
    operation: String,
    requested_copy: u64,
    applied_copy: u64,
    grid: Option<(u16, u16, bool)>,
    background: Option<u32>,
    cursor: Option<[u32; 4]>,
    lines: Vec<Vec<Run>>,
    status: String,
    error: Option<String>,
}
fn color(value: u32) -> Color {
    let channel = |bits: u32| rune::color::srgb_decode(((value >> bits) & 255) as f32 / 255.);
    Color {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.,
    }
}

impl Desktop {
    pub(crate) fn terminal_available(&self) -> bool {
        self.terminal_read
            && (!self.pending || self.terminal.is_some())
            && self.new_threads.active.is_none()
            && !self.workspace.show_projects
            && self.workspace.terminal_watch().is_some()
    }
    pub(crate) fn toggle_terminal(&mut self, tx: &mut Tx) {
        if self.terminal.is_some() {
            self.close_terminal();
            return;
        }
        if !self.terminal_available() {
            return;
        }
        let Some(watch) = self.workspace.terminal_watch() else {
            return;
        };
        self.terminal = Some(Panel {
            watch,
            operation: Uuid::new_v4().to_string(),
            grid: None,
            requested_copy: 0,
            applied_copy: 0,
            background: None,
            cursor: None,
            lines: vec![],
            status: "connecting".into(),
            error: None,
        });
        tx.focus("thread-terminal-grid", true);
    }
    pub(crate) fn close_terminal(&mut self) {
        if let Some(panel) = self.terminal.take() {
            self.write_request(
                None,
                Request::Terminal {
                    watch: panel.watch,
                    operation: panel.operation,
                    input: Input::Close,
                },
            );
        }
    }
    pub(crate) fn close_stale_terminal(&mut self) {
        if self
            .terminal
            .as_ref()
            .is_some_and(|panel| Some(panel.watch) != self.workspace.terminal_watch())
        {
            self.close_terminal();
        }
    }
    fn terminal_input(&mut self, input: Input) {
        if let Some(panel) = &self.terminal {
            self.write_request(
                None,
                Request::Terminal {
                    watch: panel.watch,
                    operation: panel.operation.clone(),
                    input,
                },
            );
        }
    }
    fn copy_terminal_selection(&mut self, interrupt_if_empty: bool) {
        if let Some(panel) = &mut self.terminal {
            panel.requested_copy += 1;
            let request = panel.requested_copy;
            self.terminal_input(Input::Copy {
                request,
                interrupt_if_empty,
            });
        }
    }
    pub(crate) fn receive_terminal_copy(
        &mut self,
        watch: u64,
        operation: &str,
        request: u64,
        text: String,
        tx: &mut Tx,
    ) {
        if let Some(panel) = &mut self.terminal
            && panel.watch == watch
            && panel.operation == operation
            && request > panel.applied_copy
            && request <= panel.requested_copy
            && !text.is_empty()
        {
            panel.applied_copy = request;
            if tx.focused() == Some("thread-terminal-grid") {
                tx.copy(text);
            }
        }
    }
    fn terminal_layout(&mut self, w: f32, h: f32, dark: bool) {
        let cols = (((w - 8.) / CW).floor() as u16).clamp(1, 1000);
        let rows = (((h - 8.) / CH).floor() as u16).clamp(1, 500);
        let Some(panel) = &mut self.terminal else {
            return;
        };
        let previous = panel.grid.replace((cols, rows, dark));
        if previous == Some((cols, rows, dark)) {
            return;
        }
        self.terminal_input(if previous.is_none() {
            Input::Open { cols, rows, dark }
        } else {
            Input::Resize { cols, rows, dark }
        });
    }
    pub(crate) fn terminal_frame(&mut self, watch: u64, operation: &str, frame: Frame) {
        let Some(panel) = &mut self.terminal else {
            return;
        };
        if panel.watch != watch || panel.operation != operation {
            return;
        }
        panel.status = frame.status;
        panel.error = frame.error;
        panel.background = Some(frame.background);
        panel.lines.resize_with(frame.rows.min(500), Vec::new);
        for (index, patch) in frame.patches {
            if let Some(line) = panel.lines.get_mut(index) {
                *line = patch;
            }
        }
        panel.cursor = frame.cursor;
        self.terminal_input(Input::Handled);
    }
}
fn input(s: &mut Desktop, e: &KeyEvent, tx: &mut Tx) -> bool {
    let shortcut = !e.mods.alt && (cfg!(target_os = "macos") != e.mods.shift);
    if shortcut && e.is_command('a') {
        s.terminal_input(Input::SelectAll);
        return true;
    }
    let copy_key = e.is_command('c');
    let copy_insert = !cfg!(target_os = "macos")
        && e.key == Key::Insert
        && e.mods.ctrl
        && !e.mods.shift
        && !e.mods.logo;
    if copy_key || copy_insert {
        s.copy_terminal_selection(
            copy_key && !cfg!(target_os = "macos") && !e.mods.shift && !e.mods.alt,
        );
        return true;
    }
    if !s.terminal_write || s.terminal.as_ref().is_none_or(|p| p.status != "running") {
        return false;
    }
    if e.is_command('v') && !e.mods.alt && (cfg!(target_os = "macos") || e.mods.shift) {
        let operation = s.terminal.as_ref().map(|p| p.operation.clone());
        tx.read_clipboard(move |s: &mut Desktop, data, _| {
            if s.terminal.as_ref().map(|p| &p.operation) == operation.as_ref()
                && !data.text().is_empty()
            {
                s.terminal_input(Input::Paste {
                    text: data.text().into(),
                });
            }
        });
        return true;
    }
    if e.mods.logo || (cfg!(target_os = "macos") && e.mods.cmd) {
        return false;
    }
    let key = match &e.key {
        Key::Text(value) => value.clone(),
        Key::Command(c) => c.to_string(),
        Key::Function(n) => format!("F{n}"),
        Key::Left => "ArrowLeft".into(),
        Key::Right => "ArrowRight".into(),
        Key::Up => "ArrowUp".into(),
        Key::Down => "ArrowDown".into(),
        Key::Enter
        | Key::Escape
        | Key::Tab
        | Key::Backspace
        | Key::Delete
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown
        | Key::Insert => format!("{:?}", e.key),
        _ => return false,
    };
    let code = tx
        .physical_key()
        .map(|k| format!("{k:?}"))
        .unwrap_or_else(|| key.clone());
    s.terminal_input(Input::Key {
        key,
        code,
        ctrl_key: e.mods.ctrl || (!cfg!(target_os = "macos") && e.mods.cmd),
        shift_key: e.mods.shift,
        alt_key: e.mods.alt,
        meta_key: e.mods.logo,
        repeat: tx.key_repeat(),
        printable: matches!(e.key, Key::Text(_)),
    });
    true
}
pub(crate) fn button(theme: &tok::Theme) -> El {
    el().key("toggle-terminal")
        .label("Toggle terminal drawer")
        .help("Toggle terminal drawer")
        .role("button")
        .focusable(true)
        .no_window_drag()
        .size(28., 28.)
        .radius(6.)
        .center()
        .hover(theme.text.alpha(0.08))
        .child(icon(&ICON, 16.).color(theme.text_secondary))
        .on_press_tx(Desktop::toggle_terminal)
}
pub(crate) fn view(s: &Desktop, theme: &tok::Theme, dark: bool, height: f32) -> Option<El> {
    let panel = s.terminal.as_ref()?;
    if !s.terminal_available() {
        return None;
    }
    let mut grid = el()
        .key("thread-terminal-grid")
        .label("Terminal")
        .role("group")
        .w_full()
        .grow(1.)
        .min_h(0.)
        .clip_rounded(0.)
        .focusable(true)
        .fill(panel.background.map(color).unwrap_or(theme.bg))
        .child(
            el().key(if dark {
                "terminal-dark-layout"
            } else {
                "terminal-light-layout"
            })
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .a11y_hidden()
            .on_layout(move |s: &mut Desktop, rect| s.terminal_layout(rect[2], rect[3], dark)),
        )
        .cursor(rune::Cursor::Text)
        .on_drag(|s: &mut Desktop, d, tx| {
            let phase = match d.phase {
                rune::Phase::Down => {
                    tx.focus("thread-terminal-grid", true);
                    "down"
                }
                rune::Phase::Move => "move",
                rune::Phase::Up => "up",
            };
            let distance = if d.y < 4. {
                d.y - 4.
            } else {
                (d.y - d.h + 4.).max(0.)
            };
            let scroll = if distance == 0. {
                0
            } else {
                (distance.signum() + (distance / 50.).clamp(-1., 1.) * 14.).round() as i32
            };
            s.terminal_input(Input::Select {
                phase,
                x: ((d.x - 4.) / CW).floor() as i32,
                y: ((d.y - 4.) / CH).floor() as i32,
                scroll,
                clicks: d.clicks,
                cancelled: d.cancelled(),
            });
            tx.live_only();
        })
        .on_key(input)
        .on_wheel(|s: &mut Desktop, e, _| {
            let lines = (-e.dy / CH).round() as i32;
            if lines != 0 {
                s.terminal_input(Input::Scroll { lines });
            }
            true
        });
    for (y, line) in panel.lines.iter().enumerate() {
        for run in line {
            let mut font = TextStyle::new(12., if run.style & 1 != 0 { 700. } else { 400. })
                .mono()
                .lh(CH);
            if run.style & 2 != 0 {
                font = font.italic();
            }
            let width = run.width as f32 * CW;
            let mut piece = el()
                .absolute(4. + run.x as f32 * CW, 4. + y as f32 * CH)
                .size(width, CH)
                .fill(color(run.bg))
                .child(text(&run.text).font(font).color(color(run.fg)));
            for (flag, offset) in [(4, 14.), (8, 8.), (16, 1.)] {
                if run.style & flag != 0 {
                    piece = piece.child(
                        el().absolute(0., offset)
                            .size(width, 1.)
                            .fill(color(run.fg)),
                    );
                }
            }
            grid = grid.child(piece);
        }
    }
    if let Some([x, y, c, style]) = panel.cursor {
        let (w, h, dy) = match style {
            0 => (2., CH, 0.),
            2 => (CW, 2., CH - 2.),
            _ => (CW, CH, 0.),
        };
        grid = grid.child(
            el().absolute(4. + x as f32 * CW, 4. + y as f32 * CH + dy)
                .size(w, h)
                .fill(color(c).alpha(0.65)),
        );
    }
    let thread = &s.workspace.conversation.as_ref()?.id;
    let pref = s
        .workspace
        .terminal_heights
        .get(thread)
        .copied()
        .unwrap_or(280.);
    let max = (height * 0.75).max(180.).min((height - 160.).max(0.));
    let clamp = move |value: f32| value.round().max(180.).min(max);
    let shown = clamp(pref);
    let resize = el()
        .key("terminal-resize")
        .role("slider")
        .label("Terminal height")
        .focusable(true)
        .range_value(
            f64::from(shown),
            f64::from(180_f32.min(max)),
            f64::from(max),
        )
        .range_step(16.)
        .on_step(move |s: &mut Desktop, steps, _| {
            if let Some(pane) = &s.workspace.conversation {
                s.workspace
                    .terminal_heights
                    .insert(pane.id.clone(), clamp(shown + steps as f32 * CH));
            }
        })
        .cursor(rune::Cursor::ResizeRow)
        .no_window_drag()
        .w_full()
        .h(4.)
        .child(el().w_full().h(1.).fill(theme.separator))
        .on_drag(move |s: &mut Desktop, drag, tx| {
            let Some(pane) = &s.workspace.conversation else {
                return;
            };
            match drag.phase {
                rune::Phase::Down => drag.data = shown,
                rune::Phase::Move => {
                    s.workspace
                        .terminal_heights
                        .insert(pane.id.clone(), clamp(drag.data - drag.dy));
                    tx.instant();
                }
                rune::Phase::Up if drag.cancelled() => {
                    s.workspace
                        .terminal_heights
                        .insert(pane.id.clone(), drag.data);
                }
                _ => {}
            }
        });
    let mut drawer = col().key("thread-terminal").w_full().h(shown).child(resize);
    let feedback = panel.error.as_deref().or(match panel.status.as_str() {
        "connecting" => Some("Connecting…"),
        "closed" | "exited" => Some("Terminal session ended."),
        "disconnected" => Some("Terminal disconnected."),
        _ => None,
    });
    if let Some(status) = feedback {
        drawer = drawer.child(
            text(status)
                .font(TextStyle::new(12., 400.))
                .color(theme.text_secondary)
                .px(4.),
        );
    }
    Some(drawer.child(grid))
}
