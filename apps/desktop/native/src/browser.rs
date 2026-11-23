use super::{Desktop, Request};
use rune::{El, Icon, Key, KeyEvent, TextEdit, TextStyle, Tx, col, el, row, text_input, tok};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const ICON: Icon = Icon {
    name: "browser-globe",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/></g>"#,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Labels {
    pub title: String,
    address: String,
    add: String,
    close_tab: String,
    close: String,
    back: String,
    forward: String,
    reload: String,
    take: String,
    release: String,
    retry: String,
}
#[derive(Deserialize)]
pub(crate) struct Tab {
    id: String,
    url: String,
    title: String,
    back: bool,
    forward: bool,
    failure: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    tabs: Vec<Tab>,
    selected: Option<String>,
    can_write: bool,
    controlled: bool,
    can_control: bool,
    status: String,
    viewport: [u32; 2],
    busy: bool,
    error: Option<String>,
}
impl Page {
    fn tab(&self) -> Option<&Tab> {
        self.tabs
            .iter()
            .find(|t| Some(&t.id) == self.selected.as_ref())
    }
}

pub(crate) struct Panel {
    watch: u64,
    operation: u64,
    page: Page,
    address: TextEdit,
    submitted_address: Option<String>,
    image: Option<rune::Image>,
    size: [f32; 2],
    requested_size: Option<[u32; 2]>,
    dragging: bool,
}
#[derive(Default)]
pub(crate) struct Browser {
    pub labels: Option<Box<Labels>>,
    pub panel: Option<Panel>,
}
impl Desktop {
    pub(crate) fn browser_available(&self) -> bool {
        self.workspace.browser.labels.is_some()
            && self.workspace.terminal_watch().is_some()
            && self.new_threads.active.is_none()
            && !self.workspace.show_projects
            && !self.history.on_issues()
            && !self.pending
    }
    fn browser_request(&mut self, command: Value) {
        if let Some(p) = &self.workspace.browser.panel {
            self.write_request(
                None,
                Request::Browser {
                    watch: p.watch,
                    operation: p.operation,
                    command,
                },
            );
        }
    }
    pub(crate) fn close_browser(&mut self) {
        self.browser_request(json!({"action":"hide"}));
        self.workspace.browser.panel = None;
    }
    pub(crate) fn toggle_browser(&mut self, tx: &mut Tx) {
        if self.workspace.browser.panel.is_some() {
            self.close_browser();
            tx.focus_when_shown("toggle-browser", true);
        } else if self.browser_available()
            && let Some(watch) = self.workspace.terminal_watch()
        {
            self.close_diff();
            self.close_files_panel();
            let operation = self.next_request_id();
            self.workspace.browser.panel = Some(Panel {
                watch,
                operation,
                page: Page::default(),
                address: TextEdit::new(""),
                submitted_address: None,
                image: None,
                size: [0., 0.],
                requested_size: None,
                dragging: false,
            });
            self.browser_request(json!({"action":"show"}));
            tx.focus_when_shown("browser-address", false);
        }
    }
    pub(crate) fn receive_browser(&mut self, watch: u64, operation: u64, page: Page) {
        if let Some(p) = &mut self.workspace.browser.panel
            && (p.watch, p.operation) == (watch, operation)
        {
            let url = page.tab().map_or("", |t| t.url.as_str());
            let previous = p.page.tab().map_or("", |t| t.url.as_str());
            if p.page.selected != page.selected {
                p.address = TextEdit::new(url);
                p.image = None;
                p.submitted_address = None;
            } else if url != previous {
                if p.address.text == previous
                    || p.submitted_address.as_ref() == Some(&p.address.text)
                {
                    p.address = TextEdit::new(url);
                }
                p.submitted_address = None;
            }
            if !page.controlled || page.selected != p.page.selected {
                p.requested_size = None;
                p.dragging = false;
            }
            p.page = page;
            self.resize_browser();
        }
    }
    pub(crate) fn browser_frame(
        &mut self,
        watch: u64,
        operation: u64,
        tab: &str,
        sequence: u64,
        image: Option<rune::Image>,
    ) {
        if let Some(p) = &mut self.workspace.browser.panel
            && (p.watch, p.operation) == (watch, operation)
        {
            if p.page.selected.as_deref() == Some(tab) && image.is_some() {
                p.image = image;
            }
            self.browser_request(json!({"action":"painted","sequence":sequence}));
        }
    }
    fn browser_command(&mut self, action: &str) {
        let Some(p) = &mut self.workspace.browser.panel else {
            return;
        };
        let mut request = json!({"action":action,"tab":p.page.selected});
        if action == "navigate" {
            p.submitted_address = Some(p.address.text.clone());
            request["value"] = json!(p.address.text);
        } else if action == "add" {
            request["value"] = json!("");
        }
        self.browser_request(request);
    }
    fn browser_input(&mut self, input: Value) {
        if let Some(tab) = self
            .workspace
            .browser
            .panel
            .as_ref()
            .and_then(|p| p.page.selected.as_ref())
        {
            self.browser_request(json!({"action":"input","tab":tab,"input":input}));
        }
    }
    fn resize_browser(&mut self) {
        let Some(p) = &mut self.workspace.browser.panel else {
            return;
        };
        let size = p.size.map(|n| n.round() as u32);
        if !p.page.controlled || size.iter().any(|n| *n < 100) || p.requested_size == Some(size) {
            return;
        }
        p.requested_size = Some(size);
        self.browser_input(json!({"type":"resize","width":size[0],"height":size[1]}));
    }

    pub(crate) fn close_hidden_browser(&mut self) -> bool {
        let Some(p) = &self.workspace.browser.panel else {
            return false;
        };
        if self.workspace.terminal_watch() == Some(p.watch)
            && self.new_threads.active.is_none()
            && !self.workspace.show_projects
            && !self.history.on_issues()
            && !self.settings_open
        {
            return false;
        }
        self.close_browser();
        true
    }
    fn browser_coords(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        let p = self.workspace.browser.panel.as_ref()?;
        let [w, h] = p.size;
        let [vw, vh] = p.page.viewport.map(|n| n as f32);
        if !p.page.controlled || p.image.is_none() || vw <= 0. || vh <= 0. {
            return None;
        }
        let scale = (w / vw).min(h / vh);
        if scale <= 0. {
            return None;
        }
        let xy = [
            (x - (w - vw * scale) / 2.) / scale,
            (y - (h - vh * scale) / 2.) / scale,
        ];
        (p.dragging || ((0.0..vw).contains(&xy[0]) && (0.0..vh).contains(&xy[1])))
            .then(|| [xy[0].clamp(0., vw - 1.), xy[1].clamp(0., vh - 1.)])
    }
}
fn key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx, down: bool) -> bool {
    if !s
        .workspace
        .browser
        .panel
        .as_ref()
        .is_some_and(|p| p.page.controlled)
    {
        return false;
    }
    if ev.key == Key::Escape && ev.mods.shift {
        if down {
            tx.focus("browser-address", true);
        }
        return true;
    }
    if ev.mods.shift && ev.is_command('j') {
        return false;
    }
    let (key, code_num) = match &ev.key {
        Key::Text(v) => (
            v.clone(),
            v.chars()
                .next()
                .filter(char::is_ascii)
                .map_or(0, |c| c.to_ascii_uppercase() as u32),
        ),
        Key::Command(c) => (c.to_string(), c.to_ascii_uppercase() as u32),
        Key::Left => ("ArrowLeft".into(), 37),
        Key::Up => ("ArrowUp".into(), 38),
        Key::Right => ("ArrowRight".into(), 39),
        Key::Down => ("ArrowDown".into(), 40),
        Key::Enter => ("Enter".into(), 13),
        Key::Tab => ("Tab".into(), 9),
        Key::Backspace => ("Backspace".into(), 8),
        Key::Delete => ("Delete".into(), 46),
        Key::Escape => ("Escape".into(), 27),
        Key::Home => ("Home".into(), 36),
        Key::End => ("End".into(), 35),
        Key::PageUp => ("PageUp".into(), 33),
        Key::PageDown => ("PageDown".into(), 34),
        _ => return false,
    };
    let modifiers = mods(ev.mods);
    let text = (down && modifiers & 7 == 0).then(|| {
        if ev.key == Key::Enter {
            "\r"
        } else if matches!(ev.key, Key::Text(_)) {
            &key
        } else {
            ""
        }
    });
    let code = tx
        .physical_key()
        .map_or_else(|| key.clone(), |k| format!("{k:?}"));
    let mut value = json!({"type":"key","action":if down {"down"} else {"up"},"key":key,"code":code,"keyCode":code_num,"modifiers":modifiers});
    if let Some(t) = text.filter(|t| !t.is_empty()) {
        value["text"] = json!(t)
    }
    s.browser_input(value);
    true
}
fn mods(m: rune::Mods) -> u8 {
    u8::from(m.alt) | (u8::from(m.ctrl) << 1) | (u8::from(m.logo) << 2) | (u8::from(m.shift) << 3)
}

fn button(
    name: &str,
    content: El,
    enabled: bool,
    theme: &tok::Theme,
    f: impl Fn(&mut Desktop, &mut Tx) + 'static,
) -> El {
    let b = el()
        .label(name)
        .help(name)
        .role("button")
        .no_window_drag()
        .min_w(26.)
        .h(26.)
        .px(4.)
        .radius(5.)
        .center()
        .child(content);
    if enabled {
        b.focusable(true).hover(theme.hover).on_press_tx(f)
    } else {
        b.disabled(true).opacity(0.45)
    }
}
pub(crate) fn view(
    s: &Desktop,
    theme: &tok::Theme,
    height: f32,
    leading: f32,
    trailing: f32,
) -> El {
    let (Some(p), Some(labels)) = (&s.workspace.browser.panel, &s.workspace.browser.labels) else {
        return el();
    };
    let tab = p.page.tab();
    let glyph = |g: &'static Icon| rune::icon(g, 14.).color(theme.text_secondary);
    let mut tabs = row()
        .key("browser-tabs")
        .role("tablist")
        .label(&labels.title)
        .focus_group()
        .grow(1.)
        .min_w(0.)
        .h(28.)
        .gap(2.)
        .center_y()
        .scroll_x();
    for tab in &p.page.tabs {
        let id = tab.id.clone();
        let selected = p.page.selected.as_ref() == Some(&id);
        let content = row()
            .max_w(142.)
            .min_w(0.)
            .gap(4.)
            .center_y()
            .child(glyph(&ICON))
            .child(
                super::workspace::label(&tab.title, 12., 500., theme.text)
                    .min_w(0.)
                    .truncate(),
            );
        let choose = button(&tab.title, content, true, theme, move |s, _| {
            s.browser_request(json!({"action":"select","tab":id}))
        })
        .role("tab")
        .focus_selected(selected);
        let mut group = row().key(&tab.id).center_y().radius(5.).child(choose);
        if selected {
            group = group.fill(theme.hover);
            if p.page.can_write {
                group = group.child(button(
                    &labels.close_tab,
                    glyph(&rune_kit::glyphs::CLOSE),
                    !p.page.busy,
                    theme,
                    |s, _| s.browser_command("close"),
                ));
            }
        }
        tabs = tabs.child(group);
    }
    tabs = tabs.child(button(
        &labels.add,
        super::workspace::label("+", 16., 400., theme.text_secondary),
        p.page.can_write && !p.page.busy,
        theme,
        |s, _| s.browser_command("add"),
    ));
    let top = row()
        .h(height)
        .w_full()
        .pl(leading + 6.)
        .pr(trailing + 6.)
        .gap(2.)
        .center_y()
        .window_drag()
        .child(tabs)
        .child(button(
            &labels.close,
            glyph(&super::workspace::workspace_tree::PANEL_ICON),
            true,
            theme,
            Desktop::toggle_browser,
        ));
    let field = text_input(&p.address, TextStyle::new(13., 400.))
        .key("browser-address")
        .role("textbox")
        .label(&labels.address)
        .placeholder(&labels.address, theme.text_secondary)
        .single_line()
        .grow(1.)
        .min_w(0.)
        .h(28.)
        .px(7.)
        .radius(5.)
        .border(1., theme.separator)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.workspace.browser.panel.as_mut().unwrap().address)
        .on_submit(|s: &mut Desktop, _| s.browser_command("navigate"));
    let toolbar = row()
        .w_full()
        .px(6.)
        .gap(2.)
        .h(34.)
        .center_y()
        .child(button(
            &labels.back,
            glyph(&super::navigation::BACK),
            p.page.controlled && tab.is_some_and(|t| t.back),
            theme,
            |s, _| s.browser_input(json!({"type":"history","delta":-1})),
        ))
        .child(button(
            &labels.forward,
            glyph(&super::navigation::FORWARD),
            p.page.controlled && tab.is_some_and(|t| t.forward),
            theme,
            |s, _| s.browser_input(json!({"type":"history","delta":1})),
        ))
        .child(button(
            &labels.reload,
            glyph(&super::workspace::palette::REFRESH),
            p.page.controlled,
            theme,
            |s, _| s.browser_input(json!({"type":"reload"})),
        ))
        .child(
            if p.page.can_write && (tab.is_none() || p.page.controlled) {
                field
            } else {
                field.disabled(true)
            },
        );
    let mut frame = el()
        .key("browser-page")
        .label(&labels.title)
        .role("image")
        .grow(1.)
        .min_h(0.)
        .w_full()
        .on_layout(|s: &mut Desktop, r| {
            if let Some(p) = &mut s.workspace.browser.panel {
                p.size = [r[2], r[3]];
                s.resize_browser();
            }
        });
    if let Some(image) = &p.image {
        frame = frame.pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
    }
    if p.page.controlled {
        frame = frame.focusable(true)
            .on_pointer_move_local(|s: &mut Desktop, e, tx| {
                if s.workspace.browser.panel.as_ref().is_some_and(|p| !p.dragging)
                    && let Some([x, y]) = s.browser_coords(e.x, e.y) {
                    s.browser_input(json!({"type":"mouse", "action":"move", "x":x, "y":y, "button":"none", "buttons":0,"clickCount":0,"modifiers":0}));
                }
                tx.live_only();
            })
            .on_drag(|s: &mut Desktop, d, tx| {
                let point = s.browser_coords(d.x, d.y);
                let Some(panel) = &mut s.workspace.browser.panel else {return};
                let (action, buttons) = match d.phase {
                    rune::Phase::Down if point.is_some() => {
                        tx.focus("browser-page", false);
                        panel.dragging = true;
                        ("down", 1)
                    }
                    rune::Phase::Move if panel.dragging => ("move", 1),
                    rune::Phase::Up if panel.dragging => {
                        panel.dragging = false;
                        ("up", 0)
                    }
                    _ => return,
                };
                if let Some([x, y]) = point {
                    s.browser_input(json!({"type":"mouse", "action":action, "x":x, "y":y, "button":"left", "buttons":buttons, "clickCount":d.clicks, "modifiers":0}));
                }
            })
            .on_wheel(|s: &mut Desktop, e, _| {
                let Some([x, y]) = s.browser_coords(e.x, e.y) else {return false};
                s.browser_input(json!({"type":"wheel","x":x,"y":y,"deltaX":-e.dx,"deltaY":-e.dy,"modifiers":mods(e.mods)}));
                true
            })
            .on_key(|s: &mut Desktop, e, tx| key(s, e, tx, true))
            .on_key_up(|s: &mut Desktop, e, tx| key(s, e, tx, false));
    }
    let mut body = col().grow(1.).min_w(0.).h_full().child(top).child(toolbar);
    if let Some(error) = p
        .page
        .error
        .as_ref()
        .or_else(|| tab.and_then(|t| t.failure.as_ref()))
    {
        body = body.child(
            row()
                .w_full()
                .px(6.)
                .gap(6.)
                .center_y()
                .child(
                    super::workspace::label(error, 12., 400., theme.text_secondary)
                        .wrap()
                        .min_w(0.)
                        .grow(1.),
                )
                .child(button(
                    &labels.retry,
                    super::workspace::label(&labels.retry, 12., 500., theme.text),
                    true,
                    theme,
                    |s, _| s.browser_command("retry"),
                )),
        );
    }
    body = body.child(frame);
    if !p.page.status.is_empty() {
        let mut status = row().w_full().px(8.).h(28.).gap(6.).center_y().child(
            super::workspace::label(&p.page.status, 12., 400., theme.text_secondary)
                .truncate()
                .grow(1.),
        );
        if p.page.can_control {
            let (name, action) = if p.page.controlled {
                (&labels.release, "releaseControl")
            } else {
                (&labels.take, "takeControl")
            };
            status = status.child(button(
                name,
                super::workspace::label(name, 12., 500., theme.text),
                true,
                theme,
                move |s, _| s.browser_input(json!({"type":action})),
            ));
        }
        body = body.child(status);
    }
    body
}
