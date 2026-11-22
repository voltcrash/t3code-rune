use super::{Desktop, NEW_THREAD_ICON, Project, Workspace, add_project};
use rune::{El, Icon, Key, KeyEvent, TextStyle, Tx, el, icon, row, text_input, tok};
use rune_kit::{Combobox, ComboboxOption, ComboboxState};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ProjectScope {
    chosen: Vec<String>,
    active: HashSet<String>,
    picker: ComboboxState,
}

impl ProjectScope {
    pub(super) fn is_open(&self) -> bool {
        self.picker.is_open()
    }
    pub fn update(&mut self, projects: &[Project]) {
        self.active = projects
            .iter()
            .filter(|p| !p.archived && self.chosen.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
    }

    pub fn close(&mut self) {
        self.picker.close();
    }

    pub fn includes(&self, id: &str) -> bool {
        self.active.is_empty() || self.active.contains(id)
    }

    pub fn single(&self) -> Option<&str> {
        if self.active.len() == 1 {
            self.active.iter().next().map(String::as_str)
        } else {
            None
        }
    }

    fn choose(&mut self, id: &str, toggle: bool, projects: &[Project]) {
        if id.is_empty() {
            self.chosen.clear();
        } else if !projects.iter().any(|p| p.id == id && !p.archived) {
            return;
        } else if toggle {
            if self.chosen.iter().any(|p| p == id) {
                self.chosen.retain(|p| p != id);
            } else {
                self.chosen.push(id.into());
            }
        } else {
            self.chosen = vec![id.into()];
        }
        self.update(projects);
    }
}

impl Workspace {
    fn query_changed(&mut self) {
        self.filter();
        self.search_target = self
            .filtered
            .first()
            .filter(|_| !self.search.text.trim().is_empty())
            .map(|&i| self.threads[i].id.clone());
    }
    fn choose_scope(&mut self, id: &str, multi: bool) {
        self.project_scope.choose(id, multi, &self.projects);
        self.filter();
    }
    fn clear_search(&mut self) {
        self.search.clear();
        self.query_changed();
    }
}

impl Desktop {
    pub(super) fn select_sidebar_thread(&mut self, id: &str) {
        if self.pending || !self.workspace.has_thread(id) {
            return;
        }
        if !self.workspace.search.text.is_empty() {
            self.workspace.clear_search();
        }
        self.open_thread(id);
    }
}

fn search_key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let ws = &mut s.workspace;
    if ws.search.preedit().is_some()
        || ws.search.text.is_empty()
        || (ev.key != Key::Escape && ws.search.text.trim().is_empty())
    {
        return false;
    }
    match ev.key {
        Key::Escape => {
            ws.clear_search();
            tx.scroll_to("thread-list", 0., 0.);
        }
        Key::Down | Key::Up if !ws.filtered.is_empty() => {
            let current = ws
                .search_target
                .as_ref()
                .and_then(|id| ws.by_thread.get(id))
                .and_then(|i| ws.filtered.iter().position(|index| index == i));
            let n = ws.filtered.len();
            let next = match (ev.key == Key::Down, current) {
                (true, None) => 0,
                (true, Some(i)) => (i + 1) % n,
                (false, Some(0) | None) => n - 1,
                (false, Some(i)) => i - 1,
            };
            let id = &ws.threads[ws.filtered[next]].id;
            tx.scroll_into_view(format!("thread-rows/{id}"));
            ws.search_target = Some(id.clone());
        }
        Key::Enter if !tx.key_repeat() => {
            if let Some(id) = ws.search_target.clone() {
                s.select_sidebar_thread(&id);
            }
        }
        _ => return false,
    }
    true
}

const FILTER: Icon = Icon {
    name: "project-filter",
    view: 24.,
    body: r#"<path d="M3 6h18M6 12h12M9 18h6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};
const FOLDER_PLUS: Icon = Icon {
    name: "folder-plus",
    view: 24.,
    body: r#"<path d="M12 10v6m-3-3h6m5 7a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const CLEAR: Icon = Icon {
    name: "clear-search",
    view: 24.,
    body: r#"<path d="m6 6 12 12M18 6 6 18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};

fn button(name: &str, glyph: &'static Icon, theme: &tok::Theme) -> El {
    el().role("button")
        .label(name)
        .help(name)
        .focusable(true)
        .size(28., 28.)
        .center()
        .radius(5.)
        .hover(theme.separator)
        .child(icon(glyph, 16.).color(theme.text_secondary))
}

fn scope_picker(s: &Desktop, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) -> El {
    let ws = &s.workspace;
    let scope = &ws.project_scope;
    let mut options = Vec::new();
    if scope.picker.is_open() {
        options.push(ComboboxOption::new("", "All projects"));
        for p in ws.projects.iter().filter(|p| !p.archived) {
            let mut option = ComboboxOption::new(&p.id, &p.title);
            if scope.active.len() > 1 && scope.active.contains(&p.id) {
                option.detail = "Selected".into();
            }
            options.push(option);
        }
    }
    let names = scope
        .chosen
        .iter()
        .filter(|id| scope.active.contains(*id))
        .filter_map(|id| ws.project(id))
        .map(|p| p.title.as_str())
        .collect::<Vec<_>>();
    let label = if names.is_empty() {
        "Filter threads by project".to_owned()
    } else {
        format!("Filter threads by project: {}", names.join(", "))
    };
    let mut trigger = button(&label, &FILTER, theme);
    if !scope.active.is_empty() {
        trigger = trigger.fill(theme.separator);
    }
    let open = scope.picker.is_open().then(|| scope.picker.clone());
    Combobox::new(
        "project-scope",
        &scope.picker,
        &options,
        |s: &mut Desktop| &mut s.workspace.project_scope.picker,
    )
    .label(&label)
    .search_placeholder("Search projects")
    .width(272.)
    .current(if scope.active.is_empty() {
        Some("")
    } else {
        scope.single()
    })
    .trigger(trigger)
    .on_choose(move |s, id, tx| {
        let ws = &mut s.workspace;
        let multi = tx.mods().shift && !id.is_empty();
        ws.choose_scope(id, multi);
        if multi && let Some(open) = &open {
            ws.project_scope.picker = open.clone();
        }
        tx.scroll_to("thread-list", 0., 0.);
    })
    .build(&s.contrast.kit(dark), overlays)
}

pub(super) fn view(s: &Desktop, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) -> El {
    let ws = &s.workspace;
    let edit = text_input(&ws.search, TextStyle::new(14., 400.))
        .key("thread-search")
        .label("Search threads")
        .min_w(0.)
        .grow(1.)
        .h(19.)
        .color(theme.text)
        .placeholder("Search", theme.text_secondary)
        .single_line()
        .on_edit(|s: &mut Desktop| &mut s.workspace.search)
        .on_input_change(|s: &mut Desktop, tx| {
            s.workspace.query_changed();
            tx.scroll_to("thread-list", 0., 0.);
        })
        .on_input_key(search_key);
    let mut search = row()
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .center_y()
        .gap(8.)
        .px(6.)
        .radius(5.)
        .hover(theme.separator)
        .child(icon(&rune_kit::glyphs::SEARCH, 16.).color(theme.text_secondary))
        .child(edit);
    if !ws.search.text.is_empty() {
        search = search.child(
            button("Clear thread search", &CLEAR, theme)
                .size(16., 20.)
                .on_press_tx(|s: &mut Desktop, tx| {
                    s.workspace.clear_search();
                    tx.focus("thread-search", true);
                    tx.scroll_to("thread-list", 0., 0.);
                }),
        );
    }
    let mut bar = row().w_full().h(48.).center_y().pl(2.).child(search);
    if !ws.projects.is_empty() {
        bar = bar.child(scope_picker(s, theme, dark, overlays));
        if s.can_operate {
            bar = bar.child(
                button("Add project", &FOLDER_PLUS, theme)
                    .key("sidebar-add-project")
                    .disabled(s.pending)
                    .on_press_tx(|s: &mut Desktop, tx| {
                        if !s.pending && !s.local {
                            s.show_projects();
                        }
                        add_project(s, tx);
                    }),
            );
        }
    }
    if s.can_operate {
        bar = bar.child(
            button("New thread", &NEW_THREAD_ICON, theme)
                .key("new-thread")
                .disabled(s.pending)
                .on_press(Desktop::new_thread_in_workspace),
        );
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_filters_follow_archive_restore_and_do_not_redirect_search_selection() {
        let project = |id: &str, archived: bool| {
            serde_json::from_value(json!({
                "id": id, "title": id, "path": id, "scratch": false, "archived": archived
            }))
            .unwrap()
        };
        let thread = |id: &str, project: &str, settled: bool| {
            serde_json::from_value(json!({
                "id": id, "project": project, "title": "Matching", "model": "test",
                "status": "idle", "settled": settled
            }))
            .unwrap()
        };
        let mut ws = Workspace::new();
        ws.replace(
            vec![project("p", false), project("q", false)],
            vec![thread("a", "p", false), thread("b", "q", true)],
        );
        ws.project_scope.choose("q", false, &ws.projects);
        ws.filter();
        assert!(ws.filtered.is_empty());
        assert_eq!(ws.settled, [1]);
        ws.show_settled = true;
        ws.filter();
        assert!(
            ws.filtered.is_empty(),
            "settled rows belong in their own section"
        );
        ws.search = rune::TextEdit::new("match");
        ws.query_changed();
        assert_eq!(ws.search_target.as_deref(), Some("b"));
        ws.choose_scope("p", true);
        assert_eq!(ws.filtered, [0, 1]);
        assert_eq!(ws.search_target.as_deref(), Some("b"));
        ws.replace(
            vec![project("p", false), project("q", true)],
            vec![thread("a", "p", false), thread("b", "q", true)],
        );
        assert_eq!(ws.filtered, [0]);
        assert_eq!(ws.search_target, None);
        ws.replace(
            vec![project("p", false), project("q", false)],
            vec![thread("a", "p", false), thread("b", "q", true)],
        );
        assert_eq!(ws.filtered, [0, 1]);
        ws.project_scope.choose("p", true, &ws.projects);
        assert_eq!(ws.project_scope.single(), Some("q"));
        ws.project_scope.choose("", false, &ws.projects);
        assert!(ws.project_scope.active.is_empty());
    }
}
