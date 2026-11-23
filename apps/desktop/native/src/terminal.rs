use crate::{Desktop, Request};
use rune::{Color, El, Family, Icon, Key, KeyEvent, TextStyle, Tx, col, el, row, text, tok};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use uuid::Uuid;

pub(crate) const ICON: Icon = Icon {
    name: "terminal",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m6 9 3 3-3 3m6 0h4"/></g>"#,
};
const CH: f32 = 16.;

#[derive(Serialize)]
#[serde(
    tag = "action",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Input {
    Open {
        #[serde(skip_serializing_if = "Option::is_none")]
        script: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        terminal_id: Option<String>,
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
#[derive(Deserialize)]
pub(crate) struct Session {
    id: String,
    label: String,
}
pub(crate) struct Panel {
    sessions: Vec<Session>,
    active: Option<String>,
    select: Option<String>,
    script: Option<String>,
    watch: u64,
    operation: String,
    requested_copy: u64,
    applied_copy: u64,
    grid: Option<(u16, u16, bool)>,
    font: Cell<Option<(u64, GridFont)>>,
    background: Option<u32>,
    cursor: Option<[u32; 4]>,
    lines: Vec<Vec<Run>>,
    status: String,
    error: Option<String>,
}
#[derive(Clone, Copy)]
struct GridFont {
    family: Family,
    cell_width: f32,
}

pub(super) fn fits_grid(mono: TextStyle) -> bool {
    let width = rune::text::advance("0", mono);
    let bold = TextStyle {
        weight: 700.,
        ..mono
    };
    [mono, mono.italic(), bold, bold.italic()]
        .into_iter()
        .all(|style| {
            ["i", "M", "W", "0", "@", "#", ".", " "]
                .into_iter()
                .all(|glyph| (rune::text::advance(glyph, style) - width).abs() < 0.01)
        })
}

impl Panel {
    fn grid_font(&self) -> GridFont {
        let epoch = rune::text::font_epoch();
        if let Some((cached_epoch, font)) = self.font.get()
            && cached_epoch == epoch
        {
            return font;
        }
        let mono = TextStyle::new(12., 400.).mono();
        let family = if fits_grid(mono) {
            Family::Mono
        } else {
            Family::SystemMono
        };
        let font = GridFont {
            family,
            cell_width: rune::text::advance("0", mono.family(family)).max(1.),
        };
        self.font.set(Some((epoch, font)));
        font
    }
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
        self.open_thread_terminal(None, None, tx);
    }
    pub(crate) fn open_thread_terminal(
        &mut self,
        script: Option<String>,
        select: Option<String>,
        tx: &mut Tx,
    ) {
        if !self.terminal_available() {
            return;
        }
        let Some(watch) = self.workspace.terminal_watch() else {
            return;
        };
        let sessions = self
            .terminal
            .as_mut()
            .filter(|p| p.watch == watch)
            .map(|p| std::mem::take(&mut p.sessions))
            .unwrap_or_default();
        self.close_terminal();
        self.terminal = Some(Panel {
            sessions,
            active: select.clone(),
            select,
            script,
            watch,
            operation: Uuid::new_v4().to_string(),
            grid: None,
            font: Cell::new(None),
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
    fn terminal_layout(&mut self, w: f32, h: f32, cell_width: f32, dark: bool) {
        let cols = (((w - 8.) / cell_width).floor() as u16).clamp(1, 1000);
        let rows = (((h - 8.) / CH).floor() as u16).clamp(1, 500);
        let Some(panel) = &mut self.terminal else {
            return;
        };
        let previous = panel.grid.replace((cols, rows, dark));
        if previous == Some((cols, rows, dark)) {
            return;
        }
        let script = panel.script.take();
        let terminal_id = panel.select.take();
        self.terminal_input(if previous.is_none() {
            Input::Open {
                cols,
                rows,
                dark,
                script,
                terminal_id,
            }
        } else {
            Input::Resize { cols, rows, dark }
        });
    }
    pub(crate) fn terminal_sessions(
        &mut self,
        watch: u64,
        operation: &str,
        active: String,
        rows: Vec<Session>,
    ) {
        if let Some(panel) = &mut self.terminal
            && panel.watch == watch
            && panel.operation == operation
        {
            panel.sessions = rows;
            panel.active = Some(active);
        }
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
    if s.zoom_key(e, tx) {
        return true;
    }
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
const PANEL_BOTTOM: Icon = Icon {
    name: "panel-bottom",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 15h18"/></g>"#,
};

pub(crate) fn button(open: bool, theme: &tok::Theme) -> El {
    crate::workspace::panel_control(
        "toggle-terminal",
        "Toggle terminal drawer",
        &PANEL_BOTTOM,
        open,
        theme,
    )
    .on_press_tx(Desktop::toggle_terminal)
}
pub(crate) fn view(s: &Desktop, theme: &tok::Theme, dark: bool, height: f32) -> Option<El> {
    let panel = s.terminal.as_ref()?;
    if !s.terminal_available() {
        return None;
    }
    let GridFont { family, cell_width } = panel.grid_font();
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
            el().key(format!(
                "terminal-layout-{}-{dark}-{}",
                panel.operation,
                cell_width.to_bits()
            ))
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .a11y_hidden()
            .on_layout(move |s: &mut Desktop, rect| {
                s.terminal_layout(rect[2], rect[3], cell_width, dark)
            }),
        )
        .cursor(rune::Cursor::Text)
        .on_drag(move |s: &mut Desktop, d, tx| {
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
                x: ((d.x - 4.) / cell_width).floor() as i32,
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
                .family(family)
                .lh(CH);
            if run.style & 2 != 0 {
                font = font.italic();
            }
            let width = run.width as f32 * cell_width;
            let mut piece = el()
                .absolute(4. + run.x as f32 * cell_width, 4. + y as f32 * CH)
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
            2 => (cell_width, 2., CH - 2.),
            _ => (cell_width, CH, 0.),
        };
        grid = grid.child(
            el().absolute(4. + x as f32 * cell_width, 4. + y as f32 * CH + dy)
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
    if panel
        .sessions
        .iter()
        .any(|s| Some(s.id.as_str()) != panel.active.as_deref())
    {
        let mut tabs = row()
            .key("thread-terminal-tabs")
            .role("radiogroup")
            .w_full()
            .h(28.)
            .center_y()
            .scroll_x();
        for session in &panel.sessions {
            let id = session.id.clone();
            let active = panel.active.as_deref() == Some(id.as_str());
            let mut tab = el()
                .key(&id)
                .role("radio")
                .label(&session.label)
                .checked(active)
                .focusable(true)
                .min_w(64.)
                .h(24.)
                .px(8.)
                .center()
                .radius(5.)
                .hover(theme.hover)
                .child(
                    text(&session.label)
                        .font(TextStyle::new(12., 500.))
                        .color(if active {
                            theme.text
                        } else {
                            theme.text_secondary
                        }),
                );
            if active {
                tab = tab.fill(theme.hover);
            } else {
                tab = tab.on_press_tx(move |s: &mut Desktop, tx| {
                    s.open_thread_terminal(None, Some(id.clone()), tx)
                });
            }
            tabs = tabs.child(tab);
        }
        drawer = drawer.child(tabs);
    }
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
