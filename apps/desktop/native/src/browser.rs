mod device;
mod dialog;
mod download;
mod input;
mod menu;
mod resize;

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
    menu: menu::Labels,
    device: device::Labels,
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
    dialog: String,
    response: String,
    dismiss: String,
    downloaded: String,
    save: String,
    file_not_shown: String,
    file_download: String,
    download_source: String,
    open_external: String,
    accept: String,
    take_to_respond: String,
    recheck_dialog: String,
}
#[derive(Deserialize)]
pub(crate) struct Tab {
    id: String,
    url: String,
    title: String,
    zoom: u16,
    scheme: String,
    device: Option<device::Viewport>,
    can_zoom_in: bool,
    can_zoom_out: bool,
    back: bool,
    forward: bool,
    failure: Option<String>,
    download: Option<download::File>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    tabs: Vec<Tab>,
    device_change: Option<device::LockChange>,
    dialog: Option<dialog::Dialog>,
    download_open_error: Option<String>,
    download: Option<download::File>,
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
    frame_size: [f32; 2],
    resize: Option<resize::Resize>,
    requested_size: Option<[u32; 2]>,
    buttons: u8,
    input: input::Input,
    answer: TextEdit,
    focus_dialog: bool,
    menu_open: bool,
    device: device::Toolbar,
}
impl Panel {
    fn new(watch: u64, operation: u64) -> Self {
        Self {
            watch,
            operation,
            page: Page::default(),
            address: TextEdit::new(""),
            submitted_address: None,
            image: None,
            size: [0., 0.],
            requested_size: None,
            resize: None,
            frame_size: [0., 0.],
            buttons: 0,
            input: input::Input::default(),
            answer: TextEdit::new(""),
            focus_dialog: false,
            menu_open: false,
            device: device::Toolbar::default(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Browser {
    inactive: bool,
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
            self.dismiss_right_panel();
            tx.focus_when_shown("toggle-browser", true);
        } else if self.browser_available()
            && let Some(watch) = self.workspace.terminal_watch()
        {
            self.close_diff();
            self.close_files_panel();
            let operation = self.next_request_id();
            self.workspace.browser.panel = Some(Panel::new(watch, operation));
            self.browser_request(json!({"action":"show"}));
            tx.focus_when_shown("browser-address", false);
        }
    }
    pub(crate) fn receive_browser(&mut self, watch: u64, operation: u64, mut page: Page) {
        if let Some(p) = &mut self.workspace.browser.panel
            && (p.watch, p.operation) == (watch, operation)
        {
            let url = page.tab().map_or("", |t| t.url.as_str());
            let previous = p.page.tab().map_or("", |t| t.url.as_str());
            if p.page.selected != page.selected {
                p.address.replace_text(url);
                p.image = None;
                p.submitted_address = None;
            } else if url != previous {
                if p.address.text == previous
                    || p.submitted_address.as_ref() == Some(&p.address.text)
                {
                    p.address.replace_text(url);
                }
                p.submitted_address = None;
            }
            if p.page.selected != page.selected
                || p.page.dialog.as_ref().map(|d| d.id) != page.dialog.as_ref().map(|d| d.id)
            {
                p.menu_open = false;
                p.answer = TextEdit::new(
                    page.dialog
                        .as_ref()
                        .map_or("", |d| d.default_value.as_str()),
                );
                p.focus_dialog = page.dialog.is_some();
            }
            if !page.controlled || page.dialog.is_some() || page.selected != p.page.selected {
                p.input.reset();
                p.requested_size = None;
                p.buttons = 0;
            }
            let new_tab = p.page.selected != page.selected;
            let viewport = page.tab().and_then(|t| t.device.as_ref());
            if new_tab || p.page.tab().and_then(|t| t.device.as_ref()) != viewport {
                p.resize = None;
                p.device.sync(viewport, new_tab);
            } else if let Some(r) = p.resize
                && (!page.can_write
                    || page.dialog.is_some()
                    || if r.sent() {
                        p.page.busy && !page.busy
                    } else {
                        page.busy
                    })
            {
                p.display_resize(None);
            }
            if let Some(change) = page.device_change.take() {
                p.device.confirm(change);
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
    fn retry_browser(&mut self) {
        let Some(p) = &self.workspace.browser.panel else {
            return;
        };
        if p.page.error.is_some() {
            self.browser_command("retry");
        } else if p.page.controlled
            && let Some(tab) = p.page.tab()
            && tab.failure.is_some()
        {
            self.browser_input(json!({"type": "navigate", "url": tab.url}));
        }
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

    pub(crate) fn sync_browser(&mut self, tx: &mut Tx) -> bool {
        let Some(p) = &self.workspace.browser.panel else {
            return false;
        };
        if self.workspace.terminal_watch() == Some(p.watch)
            && self.new_threads.active.is_none()
            && !self.workspace.show_projects
            && !self.history.on_issues()
            && !self.settings_open
        {
            return self.sync_browser_resize(tx)
                || self.sync_device_focus(tx)
                || self.sync_browser_input(tx);
        }
        self.close_browser();
        true
    }
    fn browser_coords(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        let p = self.workspace.browser.panel.as_ref()?;
        let [w, h] = p.frame_size;
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
        (p.buttons != 0 || ((0.0..vw).contains(&xy[0]) && (0.0..vh).contains(&xy[1])))
            .then(|| [xy[0].clamp(0., vw - 1.), xy[1].clamp(0., vh - 1.)])
    }
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
    bar: super::workspace::Topbar,
    kit: &rune_kit::Kit,
    overlays: &mut Vec<El>,
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
        .h(bar.height)
        .w_full()
        .pl(bar.leading + 6.)
        .pr(bar.trailing + 6.)
        .gap(2.)
        .center_y()
        .window_drag()
        .child(tabs)
        .children(s.maximize_control(theme))
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
        )
        .child(menu::view(p, labels, theme, kit, overlays));
    let mut frame = el()
        .key("browser-page")
        .label(&labels.title)
        .role("image")
        .on_layout(|s: &mut Desktop, r| {
            if let Some(p) = &mut s.workspace.browser.panel {
                p.frame_size = [r[2], r[3]];
            }
        });
    if let Some(image) = &p.image {
        frame = frame.pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
    }
    if p.page.controlled && p.page.dialog.is_none() {
        frame = input::surface(frame, p, theme);
    }
    let mut body = col().grow(1.).min_w(0.).h_full().child(top).child(toolbar);
    if let Some(size) = tab.and_then(|t| t.device.as_ref()) {
        body = body.child(device::view(p, size, &labels.device, theme, kit, overlays));
    }
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
                    p.page.error.is_some() || p.page.controlled,
                    theme,
                    |s, _| s.retry_browser(),
                )),
        );
    }
    if let Some(file) = &p.page.download {
        body = body.child(download::notice(s, p, file, labels, theme));
    }
    let content = match tab.and_then(|t| t.download.as_ref().map(|file| (t, file))) {
        Some((tab, file)) => download::file_page(s, p, tab, file, labels, theme),
        None => resize::canvas(s, frame, p, &labels.device, theme),
    };
    let mut area = col().w_full().min_h(0.).grow(1.).child(content);
    if let Some(d) = &p.page.dialog {
        area = area.child(dialog::view(p, d, labels, theme));
    }
    body = body.child(area);
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
