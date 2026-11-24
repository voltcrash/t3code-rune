//! The web's `SavedViewsMenu`: named sets of a list's search, filters and sort, kept on this device.
use crate::{Desktop, Request, workspace::label};
use rune::{El, Icon, TextEdit, TextStyle, col, el, icon, row, text_input, tok};
use rune_kit::{Kit, Menu, MenuItem, glyphs};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Deserialize)]
pub(crate) struct ListLabels {
    menu: String,
    save_title: String,
    rename_title: String,
    placeholder: String,
}

#[derive(Deserialize)]
pub(crate) struct Labels {
    views: String,
    modified: String,
    save_current: String,
    update: String,
    rename: String,
    delete: String,
    description: String,
    name: String,
    cancel: String,
    save: String,
    empty_name: String,
    too_long: String,
    duplicate: String,
    max_length: usize,
    pull_requests: ListLabels,
    issues: ListLabels,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct View<P> {
    id: String,
    name: String,
    preferences: P,
}

pub(crate) struct Views<P> {
    views: Vec<View<P>>,
    /// Views an older or newer build wrote that this one can't read, kept so a save keeps them.
    unreadable: Vec<serde_json::Value>,
    /// Like the web's, the matched view sticks once shown, so changing a filter marks it modified.
    selected: std::cell::RefCell<Option<String>>,
    pub(crate) open: bool,
    /// The name being typed, and the view it renames (none for a new view).
    editor: Option<(Option<String>, TextEdit)>,
}

impl Labels {
    pub(crate) fn pull_requests(&self) -> &ListLabels {
        &self.pull_requests
    }
    pub(crate) fn issues(&self) -> &ListLabels {
        &self.issues
    }
}

impl ListLabels {
    pub(crate) fn placeholder(&self) -> &str {
        &self.placeholder
    }
}

impl<P> Default for Views<P> {
    fn default() -> Self {
        Self {
            views: Vec::new(),
            unreadable: Vec::new(),
            selected: Default::default(),
            open: false,
            editor: None,
        }
    }
}

impl<P: DeserializeOwned> Views<P> {
    /// The saved views as the worker read them; one this build can't read is left out.
    pub(crate) fn restore(&mut self, saved: Vec<serde_json::Value>) {
        self.views.clear();
        self.unreadable.clear();
        for view in saved {
            match serde_json::from_value(view.clone()) {
                Ok(view) => self.views.push(view),
                Err(_) => self.unreadable.push(view),
            }
        }
    }
}

impl<P: PartialEq> Views<P> {
    /// The chosen view, else the first whose controls match the list's, as the web finds it.
    fn selected(&self, current: &P) -> Option<&View<P>> {
        let chosen = self.selected.borrow().clone();
        let view = chosen
            .and_then(|id| self.views.iter().find(|view| view.id == id))
            .or_else(|| self.views.iter().find(|view| &view.preferences == current));
        *self.selected.borrow_mut() = view.map(|view| view.id.clone());
        view
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct Saved {
    #[serde(default, rename = "pullRequests")]
    pub(crate) pull_requests: Vec<serde_json::Value>,
    #[serde(default)]
    pub(crate) issues: Vec<serde_json::Value>,
}

/// One list that keeps saved views: where they live and how its controls are read and set.
pub(crate) trait SavedList: 'static {
    type Prefs: Clone + PartialEq + Serialize + 'static;
    const LIST: &'static str;
    const KEY: &'static str;
    fn views(s: &Desktop) -> &Views<Self::Prefs>;
    fn views_mut(s: &mut Desktop) -> &mut Views<Self::Prefs>;
    fn current(s: &Desktop) -> Self::Prefs;
    fn apply(s: &mut Desktop, prefs: Self::Prefs);
    fn labels(labels: &Labels) -> &ListLabels;
    /// The web's `suggestedName`, or empty.
    fn suggested(s: &Desktop, labels: &ListLabels) -> String;
}

const BOOKMARK: Icon = Icon {
    name: "bookmark",
    view: 24.,
    body: r#"<path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const PLUS: Icon = Icon {
    name: "plus",
    view: 24.,
    body: r#"<path d="M5 12h14M12 5v14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

fn persist<L: SavedList>(s: &mut Desktop) {
    let saved = L::views(s);
    let mut views: Vec<serde_json::Value> = saved
        .views
        .iter()
        .filter_map(|view| serde_json::to_value(view).ok())
        .collect();
    views.extend(saved.unreadable.iter().cloned());
    let views = serde_json::Value::Array(views);
    s.write_request(
        None,
        Request::SetSavedViews {
            list: L::LIST,
            views,
        },
    );
}

fn name_error<'a, L: SavedList>(s: &Desktop, labels: &'a Labels) -> Option<&'a str> {
    let views = L::views(s);
    let (editing, input) = views.editor.as_ref()?;
    let name = input.text.trim();
    if name.is_empty() {
        return Some(&labels.empty_name);
    }
    // UTF-16 units, as the web input's maxLength and the worker count.
    if name.encode_utf16().count() > labels.max_length {
        return Some(&labels.too_long);
    }
    views
        .views
        .iter()
        .any(|view| {
            Some(&view.id) != editing.as_ref() && view.name.to_lowercase() == name.to_lowercase()
        })
        .then_some(labels.duplicate.as_str())
}

fn save_name<L: SavedList>(s: &mut Desktop) {
    let Some(labels) = s.workspace.saved_view_labels.take() else {
        return;
    };
    let invalid = name_error::<L>(s, &labels).is_some();
    s.workspace.saved_view_labels = Some(labels);
    if invalid {
        return;
    }
    let current = L::current(s);
    let views = L::views_mut(s);
    let Some((editing, input)) = views.editor.take() else {
        return;
    };
    let name = input.text.trim().to_owned();
    let id = match editing {
        // A view removed while its rename was open is saved as a new one instead.
        Some(id) if views.views.iter().any(|view| view.id == id) => {
            for view in &mut views.views {
                if view.id == id {
                    view.name = name.clone();
                }
            }
            id
        }
        _ => {
            let id = uuid::Uuid::new_v4().to_string();
            views.views.push(View {
                id: id.clone(),
                name,
                preferences: current,
            });
            id
        }
    };
    *views.selected.borrow_mut() = Some(id);
    persist::<L>(s);
}

/// The Views button and its menu, like the web toolbar's.
pub(crate) fn menu<L: SavedList>(
    s: &Desktop,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
) -> El {
    let Some(labels) = &s.workspace.saved_view_labels else {
        return el();
    };
    let list = L::labels(labels);
    let views = L::views(s);
    let current = L::current(s);
    let selected = views.selected(&current);
    let modified = selected.is_some_and(|view| view.preferences != current);
    let caption = selected.map_or(labels.views.as_str(), |view| view.name.as_str());
    let mut trigger = row()
        .key(format!("{}-trigger", L::KEY))
        .role("button")
        .label(&list.menu)
        .help(selected.map_or(list.menu.as_str(), |view| view.name.as_str()))
        .h(32.)
        .px(10.)
        .gap(6.)
        .center_y()
        .radius(8.)
        .border(1., theme.card_border)
        .focusable(true)
        .hover(theme.hover)
        .child(icon(&BOOKMARK, 14.).color(theme.text_secondary))
        .child(label(caption, 13., 500., theme.text).truncate().max_w(160.));
    if modified {
        trigger = trigger.child(label("*", 13., 500., theme.text).label(&labels.modified));
    }
    let trigger = trigger
        .child(icon(&glyphs::CHEVRON_DOWN, 12.).color(theme.text_secondary))
        .on_press(|s: &mut Desktop| {
            let views = L::views_mut(s);
            views.open = !views.open;
        });
    let mut menu = Menu::new(L::KEY)
        .trigger(trigger)
        .open(views.open)
        .min_width(200.)
        .on_close(|s: &mut Desktop| L::views_mut(s).open = false);
    if !views.open {
        return menu.build(kit, overlays);
    }
    let lead = |glyph: &'static Icon| icon(glyph, 14.).color(theme.text_secondary);
    for view in &views.views {
        let shown = selected.is_some_and(|chosen| chosen.id == view.id) && !modified;
        let id = view.id.clone();
        menu = menu.item(
            MenuItem::new(&view.name)
                .lead(lead(if shown { &glyphs::CHECK } else { &BOOKMARK }))
                .on_select(move |s: &mut Desktop| {
                    let views = L::views_mut(s);
                    views.open = false;
                    *views.selected.borrow_mut() = Some(id.clone());
                    if let Some(view) = views.views.iter().find(|view| view.id == id) {
                        let prefs = view.preferences.clone();
                        L::apply(s, prefs);
                    }
                }),
        );
    }
    if !views.views.is_empty() {
        menu = menu.separator();
    }
    menu = menu.item(
        MenuItem::new(&labels.save_current)
            .lead(lead(&PLUS))
            .on_select_tx(|s: &mut Desktop, tx| {
                tx.focus_when_shown(&format!("{}-name", L::KEY), true);
                let suggested = s
                    .workspace
                    .saved_view_labels
                    .as_ref()
                    .map(|labels| L::suggested(s, L::labels(labels)))
                    .unwrap_or_default();
                let views = L::views_mut(s);
                views.open = false;
                views.editor = Some((None, TextEdit::new(&suggested)));
            }),
    );
    if let Some(chosen) = selected {
        let id = chosen.id.clone();
        if modified {
            let id = id.clone();
            menu = menu.item(
                MenuItem::new(&labels.update).on_select(move |s: &mut Desktop| {
                    let current = L::current(s);
                    let views = L::views_mut(s);
                    views.open = false;
                    for view in &mut views.views {
                        if view.id == id {
                            view.preferences = current.clone();
                        }
                    }
                    persist::<L>(s);
                }),
            );
        }
        let rename = (id.clone(), chosen.name.clone());
        menu = menu
            .item(
                MenuItem::new(&labels.rename).on_select_tx(move |s: &mut Desktop, tx| {
                    tx.focus_when_shown(&format!("{}-name", L::KEY), true);
                    let views = L::views_mut(s);
                    views.open = false;
                    views.editor = Some((Some(rename.0.clone()), TextEdit::new(&rename.1)));
                }),
            )
            .item(
                MenuItem::new(&labels.delete)
                    .danger(true)
                    .on_select(move |s: &mut Desktop| {
                        let views = L::views_mut(s);
                        views.open = false;
                        views.views.retain(|view| view.id != id);
                        *views.selected.borrow_mut() = None;
                        persist::<L>(s);
                    }),
            );
    }
    menu.build(kit, overlays)
}

/// The name dialog for saving or renaming a view.
pub(crate) fn dialog<L: SavedList>(s: &Desktop, theme: &tok::Theme, overlays: &mut Vec<El>) {
    let (Some(labels), Some((editing, input))) =
        (&s.workspace.saved_view_labels, &L::views(s).editor)
    else {
        return;
    };
    let list = L::labels(labels);
    let error = name_error::<L>(s, labels);
    let field = text_input(input, TextStyle::new(14., 400.))
        .key(format!("{}-name", L::KEY))
        .label(&labels.name)
        .placeholder(&list.placeholder, theme.text_secondary)
        .single_line()
        .w_full()
        .px(4.)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut L::views_mut(s).editor.as_mut().expect("open editor").1)
        .on_submit(|s: &mut Desktop, _| save_name::<L>(s));
    let mut body = col()
        .w_full()
        .gap(8.)
        .child(label(&labels.name, 14., 500., theme.text))
        .child(
            row()
                .w_full()
                .h(36.)
                .px(8.)
                .center_y()
                .radius(8.)
                .border(1., theme.card_border)
                .child(field),
        );
    if let Some(error) = error.filter(|_| !input.text.is_empty()) {
        body =
            body.child(label(error, 13., 400., crate::confirm_dialog::destructive(theme)).wrap());
    }
    let title = if editing.is_some() {
        &list.rename_title
    } else {
        &list.save_title
    };
    crate::confirm_dialog::form(
        crate::confirm_dialog::Confirm {
            key: L::KEY,
            title,
            description: Some(&labels.description),
            cancel: &labels.cancel,
            confirm: &labels.save,
            destructive: false,
        },
        Some(body),
        error.is_none(),
        theme,
        |s| L::views_mut(s).editor = None,
        save_name::<L>,
        overlays,
    );
}
