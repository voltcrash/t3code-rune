#[path = "workspace_tree/filter.rs"]
mod filter;
use super::workspace_files::{Input, Location};
use super::{Desktop, Topbar, action, label};
use filter::Matches;
use rune::{El, GrowthSpec, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, tok};
use rune_kit::{Kit, TreeItem, TreeRow, TreeStyle, glyphs};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Directory,
    File,
}

#[derive(Deserialize)]
pub(crate) struct Entry {
    path: String,
    kind: Kind,
    #[serde(default)]
    ignored: bool,
    #[serde(skip)]
    search_path: std::cell::OnceCell<String>,
}
impl Entry {
    fn new(path: String, kind: Kind) -> Self {
        Self {
            path,
            kind,
            ignored: false,
            search_path: Default::default(),
        }
    }
    fn search_path(&self) -> &str {
        self.search_path.get_or_init(|| {
            let mut path = self.path.to_lowercase();
            if self.kind == Kind::Directory {
                path.push('/');
            }
            path
        })
    }
    fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Default)]
struct Directory {
    entries: Vec<Rc<Entry>>,
    error: Option<String>,
    truncated: bool,
}

impl Directory {
    fn sort(&mut self) {
        self.entries.sort_by(|a, b| {
            (a.kind == Kind::File)
                .cmp(&(b.kind == Kind::File))
                .then_with(|| rune_kit::natural_cmp(a.name(), b.name()))
        });
    }
}

struct Visible {
    entry: Rc<Entry>,
    depth: u32,
}

pub(crate) struct Explorer {
    pub(super) location: Location,
    generation: u64,
    directories: HashMap<String, Directory>,
    loading: HashSet<String>,
    expanded: HashSet<String>,
    visible: Vec<Visible>,
    cursor: Option<String>,
    revision: u64,
    query: TextEdit,
    matches: Option<Matches>,
}

impl Explorer {
    fn new(location: Location, generation: u64) -> Self {
        Self {
            location,
            generation,
            directories: HashMap::new(),
            loading: HashSet::new(),
            expanded: HashSet::new(),
            visible: Vec::new(),
            cursor: None,
            revision: 0,
            query: TextEdit::new(""),
            matches: None,
        }
    }
    fn focus_row(&mut self, path: &str, tx: &mut Tx) {
        self.cursor = Some(path.into());
        tx.focus("workspace-tree", true);
        tx.scroll_into_view_in("workspace-tree-rows", path);
    }
    fn rebuild(&mut self) {
        self.visible.clear();
        let directories = self
            .matches
            .as_ref()
            .map_or(&self.directories, |f| &f.directories);
        if let Some(root) = directories.get("") {
            let mut queue: Vec<_> = root.entries.iter().rev().map(|e| (e, 0)).collect();
            while let Some((entry, depth)) = queue.pop() {
                self.visible.push(Visible {
                    entry: Rc::clone(entry),
                    depth,
                });
                if self.matches.as_ref().map_or_else(
                    || self.expanded.contains(&entry.path),
                    |f| !f.closed.contains(&entry.path),
                ) && let Some(dir) = directories.get(&entry.path)
                {
                    queue.extend(dir.entries.iter().rev().map(|e| (e, depth + 1)));
                }
            }
        }
        self.revision += 1;
    }
    fn insert(
        &mut self,
        directory: String,
        mut entries: Vec<Entry>,
        error: Option<String>,
        truncated: bool,
    ) {
        if !self.loading.remove(&directory) {
            return;
        }
        entries.retain(|e| {
            let (parent, name) = e.path.rsplit_once('/').unwrap_or(("", &e.path));
            parent == directory && !name.is_empty() && name != "." && name != ".."
        });
        let mut page = Directory {
            entries: entries.into_iter().map(Rc::new).collect(),
            error,
            truncated,
        };
        page.sort();
        self.directories.insert(directory, page);
        self.refresh_matches();
    }
}

impl Desktop {
    pub(crate) fn close_explorer(&mut self) {
        if let Some(tree) = self.workspace.explorer.take() {
            if let Some(filter) = tree.matches {
                self.file_request(
                    tree.location.clone(),
                    filter.operation,
                    Input::Filter {
                        query: String::new(),
                        cancel: true,
                    },
                );
            }
            self.file_request(
                tree.location,
                tree.generation,
                Input::List {
                    directory: String::new(),
                    cancel: true,
                },
            );
        }
    }
    pub(crate) fn close_files_panel(&mut self) {
        self.close_explorer();
        self.close_file_preview();
    }
    pub(crate) fn toggle_files_panel(&mut self, tx: &mut Tx) {
        if self.workspace.explorer.is_some()
            || self.workspace.file_preview.is_some()
            || self.workspace.diff.visible(self)
        {
            self.close_diff();
            self.close_files_panel();
            return;
        }
        self.show_file_tree(tx);
    }
    pub(super) fn show_file_tree(&mut self, tx: &mut Tx) {
        if self.pending {
            return;
        }
        let Some(location) = self.file_location() else {
            return;
        };
        let generation = self.next_request_id();
        self.workspace.explorer = Some(Explorer::new(location, generation));
        self.list_directory("");
        tx.focus_when_shown("workspace-tree", true);
    }
    fn refresh_file_tree(&mut self, tx: &mut Tx) {
        let Some(tree) = &mut self.workspace.explorer else {
            return;
        };
        let expanded = std::mem::take(&mut tree.expanded);
        let selected = tree
            .matches
            .as_ref()
            .map_or_else(|| tree.cursor.clone(), |f| f.previous_cursor.clone());
        let query = tree.query.text.clone();
        self.close_explorer();
        self.show_file_tree(tx);
        if let Some(tree) = &mut self.workspace.explorer {
            tree.expanded = expanded;
            tree.cursor = selected;
            tree.query = TextEdit::new(&query);
        }
        if !query.trim().is_empty() {
            self.filter_tree();
        }
        self.reload_file_preview();
    }
    fn list_directory(&mut self, path: &str) {
        let Some(tree) = &mut self.workspace.explorer else {
            return;
        };
        if !tree.loading.insert(path.into()) {
            return;
        }
        let loc = tree.location.clone();
        let op = tree.generation;
        self.file_request(
            loc,
            op,
            Input::List {
                directory: path.into(),
                cancel: false,
            },
        );
    }
    pub(crate) fn directory_loaded(
        &mut self,
        operation: u64,
        directory: String,
        entries: Vec<Entry>,
        truncated: bool,
        error: Option<String>,
    ) {
        if let Some(tree) = &mut self.workspace.explorer
            && tree.generation == operation
        {
            tree.insert(directory.clone(), entries, error, truncated);
            let expanded: Vec<_> = tree
                .directories
                .get(&directory)
                .into_iter()
                .flat_map(|d| &d.entries)
                .filter(|e| {
                    e.kind == Kind::Directory
                        && tree.expanded.contains(&e.path)
                        && !tree.directories.contains_key(&e.path)
                })
                .map(|e| e.path.clone())
                .collect();
            if let Some(key) = &tree.cursor
                && key.rsplit_once('/').map_or("", |(p, _)| p) == directory
                && !tree.visible.iter().any(|r| r.entry.path == *key)
            {
                tree.cursor = None;
            }
            for path in expanded {
                self.list_directory(&path);
            }
        }
    }
    fn select_entry(&mut self, path: &str, open: bool, preview_only: bool, tx: &mut Tx) {
        let Some(tree) = &mut self.workspace.explorer else {
            return;
        };
        let Some(v) = tree.visible.iter().find(|row| row.entry.path == path) else {
            return;
        };
        let dir = v.entry.kind == Kind::Directory;
        tree.focus_row(path, tx);
        if !dir {
            let loc = tree.location.clone();
            self.open_workspace_file(loc, path.into());
            if preview_only {
                tx.focus_when_shown("Close file", true);
            }
        } else if open {
            if let Some(f) = &mut tree.matches {
                if !f.closed.remove(path) {
                    f.closed.insert(path.into());
                }
                tree.rebuild();
                return;
            }
            if !tree.expanded.remove(path) {
                tree.expanded.insert(path.into());
            }
            let load = tree.expanded.contains(path)
                && tree.directories.get(path).is_none_or(|p| p.error.is_some());
            tree.rebuild();
            if load {
                self.list_directory(path);
            }
        }
    }
}

fn key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx, preview_only: bool) -> bool {
    let Some(tree) = &s.workspace.explorer else {
        return false;
    };
    if ev.mods.primary() || ev.mods.alt || tree.visible.is_empty() {
        return false;
    }
    let i = tree
        .cursor
        .as_ref()
        .and_then(|p| tree.visible.iter().position(|e| &e.entry.path == p));
    let cur = i.unwrap_or(0);
    let current = &tree.visible[cur];
    let toggle = match ev.key {
        Key::Enter => true,
        Key::Text(ref t) if t == " " => true,
        Key::Right => {
            current.entry.kind == Kind::Directory && !tree.directory_open(&current.entry.path)
        }
        Key::Left => {
            current.entry.kind == Kind::Directory && tree.directory_open(&current.entry.path)
        }
        _ => false,
    };
    let target = if toggle {
        cur
    } else {
        match ev.key {
            Key::Home => 0,
            Key::End => tree.visible.len() - 1,
            Key::Down => i.map_or(0, |n| (n + 1).min(tree.visible.len() - 1)),
            Key::Up => cur.saturating_sub(1),
            Key::Right
                if tree
                    .visible
                    .get(cur + 1)
                    .is_some_and(|child| child.depth > current.depth) =>
            {
                cur + 1
            }
            Key::Left => {
                let Some((parent, _)) = current.entry.path.rsplit_once('/') else {
                    return true;
                };
                let Some(n) = tree.visible.iter().position(|r| r.entry.path == parent) else {
                    return true;
                };
                n
            }
            _ => return false,
        }
    };
    if !toggle && i == Some(target) {
        return true;
    }
    let path = tree.visible[target].entry.path.clone();
    if !toggle || !tx.key_repeat() {
        s.select_entry(&path, toggle, preview_only, tx);
    }
    true
}

pub(super) fn tree_view(s: &Desktop, theme: &tok::Theme, kit: &Kit, preview_only: bool) -> El {
    let tree = s.workspace.explorer.as_ref().unwrap();
    let style = TreeStyle::from_kit(kit).chevron_only_folders(true);
    let k = kit.clone();
    let rows = col()
        .key("workspace-tree-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            tree.visible.len(),
            style.row_height,
            |s: &Desktop, i| {
                s.workspace
                    .explorer
                    .as_ref()
                    .and_then(|t| t.visible.get(i))
                    .map_or("".into(), |v| v.entry.path.as_str().into())
            },
            move |s: &Desktop, i| {
                let Some(tree) = &s.workspace.explorer else {
                    return el();
                };
                let Some(v) = tree.visible.get(i) else {
                    return el();
                };
                let e = &v.entry;
                let path = e.path.clone();
                let folder = e.kind == Kind::Directory;
                let open = folder.then(|| tree.directory_open(&path));
                let state = TreeRow::new(&path, v.depth)
                    .expanded(open)
                    .selected(tree.cursor.as_ref() == Some(&path));
                let glyph = if folder {
                    &glyphs::FOLDER
                } else {
                    rune_kit::file_icon(&path)
                };
                let mut item = TreeItem::new(e.name()).icon(glyph);
                if tree.matches.is_some() {
                    item = item.highlight(tree.query.text.trim());
                }
                if e.ignored {
                    item = item.label_color(k.c.text_muted);
                }
                let mut row = item.build(&k, &state, &style).key(&path).label(&path);
                if folder && open == Some(true) {
                    if tree.loading.contains(&path) {
                        row = row.help("Loading files…");
                    } else if let Some(dir) = tree.directories.get(&path)
                        && let Some(msg) = &dir.error
                    {
                        row = row.help(msg);
                    }
                }
                row.on_press_tx(move |s: &mut Desktop, tx| {
                    s.select_entry(&path, true, preview_only, tx)
                })
            },
        )
        .virtual_rows_version((tree.generation, tree.revision));
    let mut list = col()
        .key("workspace-tree")
        .role("tree")
        .label("Workspace files")
        .focusable(true)
        .grow(1.)
        .min_h(0.)
        .w_full()
        .on_key(move |s: &mut Desktop, ev, tx| key(s, ev, tx, preview_only));
    if let Some(cur) = &tree.cursor {
        list = list.active_descendant(cur);
    }
    let mut viewport = col()
        .grow(1.)
        .min_h(0.)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(rows);
    let message = if let Some(f) = &tree.matches {
        f.error
            .as_deref()
            .or(tree.visible.is_empty().then_some(if f.pending {
                "Loading files…"
            } else {
                "No matching files."
            }))
    } else if let Some(root) = tree.directories.get("") {
        root.error
            .as_deref()
            .or(root.entries.is_empty().then_some("This folder is empty."))
    } else {
        Some("Loading files…")
    };
    if let Some(message) = message {
        viewport = viewport.child(
            label(message, 12., 400., theme.text_secondary)
                .padding(8.)
                .wrap(),
        );
    }
    if let Some(d) = tree
        .cursor
        .as_ref()
        .filter(|p| tree.expanded.contains(*p))
        .and_then(|p| tree.directories.get(p))
        && let Some(err) = &d.error
    {
        viewport = viewport.child(
            label(err, 12., 400., theme.text_secondary)
                .padding(8.)
                .wrap(),
        );
    }
    if tree.matches.as_ref().map_or_else(
        || {
            tree.directories
                .iter()
                .any(|(k, d)| d.truncated && (k.is_empty() || tree.expanded.contains(k)))
        },
        |f| f.truncated,
    ) {
        viewport = viewport.child(
            label(
                "More files available. Refine your search.",
                12.,
                400.,
                theme.text_secondary,
            )
            .padding(8.)
            .wrap(),
        );
    }
    let controls = row()
        .w_full()
        .h(36.)
        .px(8.)
        .gap(6.)
        .center_y()
        .child(
            el().size(24., 24.)
                .role("button")
                .label("Refresh workspace files")
                .help("Refresh files")
                .focusable(true)
                .radius(4.)
                .center()
                .hover(theme.hover)
                .child(icon(&super::palette::REFRESH, 14.).color(theme.text_secondary))
                .on_press_tx(Desktop::refresh_file_tree),
        )
        .child(filter::field(tree, theme, kit));
    col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(controls)
        .child(list.child(viewport))
}

pub(super) static PANEL_ICON: rune::Icon = rune::Icon {
    name: "panel-right",
    view: 24.,
    body: r#"<rect x="3" y="3" width="18" height="18" rx="2" fill="none" stroke="currentColor" stroke-width="2"/><path d="M15 3v18" fill="none" stroke="currentColor" stroke-width="2"/>"#,
};
pub(super) fn toggle_button(open: bool, theme: &tok::Theme) -> El {
    super::panel_control(
        "toggle-right-panel",
        "Toggle right panel",
        &PANEL_ICON,
        open,
        theme,
    )
    .on_press_tx(Desktop::toggle_files_panel)
}
pub(super) fn panel(s: &Desktop, theme: &tok::Theme, kit: &Kit, bar: Topbar, width: f32) -> El {
    let preview_only = width < 416.;
    let tree = tree_view(s, theme, kit, preview_only);
    if s.workspace.file_preview.is_some() {
        let sidebar = (!preview_only).then(|| {
            col()
                .w((width - 256.).clamp(160., 256.))
                .h_full()
                .child(tree)
        });
        return super::workspace_files::view(s, theme, kit, bar, sidebar)
            .on_key(move |s: &mut Desktop, ev, tx| filter::find(s, ev, tx, preview_only));
    }
    col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(
            row()
                .h(bar.height)
                .pl(bar.leading + 12.)
                .pr(bar.trailing + 12.)
                .center_y()
                .window_drag()
                .child(label("Files", 14., 500., theme.text).grow(1.))
                .child(action(
                    "Close panel",
                    theme,
                    true,
                    Desktop::close_files_panel,
                )),
        )
        .child(el().h(1.).w_full().fill(theme.separator))
        .child(tree)
        .on_key(|s: &mut Desktop, ev, tx| filter::find(s, ev, tx, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_order_folder_reads_preserve_expansion_after_refresh() {
        let mut app = Desktop::new();
        let location = Location {
            watch: 8,
            root: "/repo".into(),
        };
        let mut tree = Explorer::new(location, 12);
        tree.expanded.extend(["docs".to_owned(), "src".to_owned()]);
        tree.loading.insert(String::new());
        app.workspace.explorer = Some(tree);
        let entry = |path: &str, kind| Entry::new(path.into(), kind);
        app.directory_loaded(
            12,
            String::new(),
            vec![
                entry("src", Kind::Directory),
                entry("docs", Kind::Directory),
            ],
            false,
            None,
        );
        app.directory_loaded(
            12,
            "src".into(),
            vec![entry("src/lib.rs", Kind::File)],
            false,
            None,
        );
        app.directory_loaded(
            11,
            "docs".into(),
            vec![entry("docs/removed", Kind::File)],
            false,
            None,
        );
        app.directory_loaded(
            12,
            "docs".into(),
            vec![entry("docs/guide.md", Kind::File)],
            false,
            None,
        );
        let tree = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(
            tree.visible
                .iter()
                .map(|r| r.entry.path.as_str())
                .collect::<Vec<_>>(),
            ["docs", "docs/guide.md", "src", "src/lib.rs"]
        );
        assert!(tree.loading.is_empty());
    }
}
