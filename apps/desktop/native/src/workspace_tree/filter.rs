use super::{Desktop, Directory, Entry, Explorer, Input, Kind, Rc};
use rune::{El, Key, KeyEvent, TextEdit, TextStyle, Tx, text_input, tok};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Matches {
    pub operation: u64,
    pub error: Option<String>,
    pub truncated: bool,
    pub pending: bool,
    remote: Vec<Rc<Entry>>,
    pub(super) previous_cursor: Option<String>,
    pub directories: HashMap<String, Directory>,
    pub closed: HashSet<String>,
}

impl Matches {
    pub fn collect(&mut self, query: &str, loaded: &HashMap<String, Directory>) {
        let needle = query.trim().replace('\\', "/").to_lowercase();
        let entries = loaded.values().flat_map(|d| &d.entries).chain(&self.remote);
        let mut folders = HashMap::new();
        let mut nodes = HashMap::new();
        for entry in entries {
            if entry.kind == Kind::Directory {
                folders.entry(entry.path.as_str()).or_insert(entry);
            }
            if entry.search_path().contains(&needle) {
                nodes
                    .entry(entry.path.clone())
                    .or_insert_with(|| Rc::clone(entry));
            }
        }
        let hits: Vec<_> = nodes.values().cloned().collect();
        for hit in hits {
            let mut rest = hit.path.as_str();
            while let Some((parent, _)) = rest.rsplit_once('/') {
                if nodes.contains_key(parent) {
                    break;
                }
                let folder = folders.get(parent).map_or_else(
                    || Rc::new(Entry::new(parent.to_owned(), Kind::Directory)),
                    |entry| Rc::clone(entry),
                );
                nodes.insert(parent.to_owned(), folder);
                rest = parent;
            }
        }
        self.directories.clear();
        for (path, e) in nodes {
            let parent = path.rsplit_once('/').map_or("", |(dir, _)| dir);
            self.directories
                .entry(parent.into())
                .or_default()
                .entries
                .push(e);
        }
        for d in self.directories.values_mut() {
            d.sort();
        }
    }
}

impl Explorer {
    pub(super) fn refresh_matches(&mut self) {
        if let Some(f) = &mut self.matches {
            f.collect(&self.query.text, &self.directories);
        }
        self.rebuild();
        if self.matches.is_some()
            && let Some(cursor) = &self.cursor
            && !self.visible.iter().any(|row| row.entry.path == *cursor)
        {
            self.cursor = None;
        }
    }
    pub(super) fn directory_open(&self, path: &str) -> bool {
        self.matches.as_ref().map_or_else(
            || self.expanded.contains(path),
            |f| !f.closed.contains(path),
        )
    }
}

impl Desktop {
    pub(super) fn filter_tree(&mut self) {
        let op = self.next_request_id();
        let Some(tree) = &mut self.workspace.explorer else {
            return;
        };
        let query = tree.query.text.trim().replace('\\', "/");
        let location = tree.location.clone();
        if query.is_empty() {
            let previous = tree.matches.take();
            if let Some(old) = &previous {
                tree.cursor.clone_from(&old.previous_cursor);
            }
            tree.rebuild();
            if let Some(old) = previous {
                self.file_request(
                    location,
                    old.operation,
                    Input::Filter {
                        query,
                        cancel: true,
                    },
                );
            }
            return;
        }
        let previous_cursor = tree
            .matches
            .take()
            .map_or_else(|| tree.cursor.clone(), |f| f.previous_cursor);
        tree.matches = Some(Matches {
            previous_cursor,
            operation: op,
            pending: true,
            ..Default::default()
        });
        tree.refresh_matches();
        self.file_request(
            location,
            op,
            Input::Filter {
                query,
                cancel: false,
            },
        );
    }

    pub(crate) fn tree_filtered(
        &mut self,
        operation: u64,
        entries: Vec<Entry>,
        truncated: bool,
        error: Option<String>,
    ) {
        let Some(tree) = &mut self.workspace.explorer else {
            return;
        };
        let Some(f) = &mut tree.matches else { return };
        if f.operation != operation {
            return;
        }
        f.remote = entries.into_iter().map(Rc::new).collect();
        f.error = error;
        f.truncated = truncated;
        f.pending = false;
        tree.refresh_matches();
    }
}

pub(super) fn find(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx, preview_only: bool) -> bool {
    if ev.mods.alt || ev.mods.shift || !ev.is_command('f') {
        return false;
    }
    if preview_only {
        s.close_file_preview();
    }
    if let Some(tree) = &mut s.workspace.explorer {
        tree.query.set_selection(0, tree.query.text.len());
        tx.focus_when_shown("workspace-file-search", true);
    }
    true
}

pub(super) fn field(tree: &Explorer, theme: &tok::Theme, kit: &rune_kit::Kit) -> El {
    text_input(&tree.query, TextStyle::new(12., 400.))
        .key("workspace-file-search")
        .label("Search files")
        .placeholder("Search files", theme.text_secondary)
        .single_line()
        .min_w(0.)
        .grow(1.)
        .h(28.)
        .radius(5.)
        .px(6.)
        .focus_ring(1., 0., kit.c.border_focus)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| &mut s.workspace.explorer.as_mut().unwrap().query)
        .on_input_change(|s: &mut Desktop, _| s.filter_tree())
        .on_input_key(|s: &mut Desktop, ev, tx: &mut Tx| {
            if find(s, ev, tx, false) {
                return true;
            }
            if ev.key == Key::Escape {
                if let Some(tree) = &mut s.workspace.explorer {
                    tree.query = TextEdit::new("");
                }
                s.filter_tree();
                tx.focus("workspace-tree", true);
                return true;
            }
            if ev.key == Key::Down
                && let Some(tree) = &mut s.workspace.explorer
                && let Some(row) = tree.visible.first()
            {
                let path = row.entry.path.clone();
                tree.focus_row(&path, tx);
                return true;
            }
            false
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::workspace_files::Location;

    #[test]
    fn combines_loaded_ignored_entries_and_remote_matches_without_changing_expansion() {
        let mut app = Desktop::new();
        let mut tree = Explorer::new(
            Location {
                watch: 7,
                root: "/repo".into(),
            },
            1,
        );
        let mut folder = Entry::new("src".into(), Kind::Directory);
        folder.ignored = true;
        tree.loading.insert(String::new());
        tree.insert(
            String::new(),
            vec![folder, Entry::new("README".into(), Kind::File)],
            None,
            false,
        );
        tree.loading.insert("src".into());
        tree.insert(
            "src".into(),
            vec![Entry::new("src/比較.txt".into(), Kind::File)],
            None,
            false,
        );
        tree.cursor = Some("README".into());
        tree.query = TextEdit::new("SRC\\比較");
        app.workspace.explorer = Some(tree);
        app.filter_tree();
        let operation = app
            .workspace
            .explorer
            .as_ref()
            .unwrap()
            .matches
            .as_ref()
            .unwrap()
            .operation;
        let hit = || Entry::new("src/比較/結果.txt".into(), Kind::File);
        app.tree_filtered(operation + 1, vec![hit()], false, None);
        let current = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(current.visible.len(), 2);
        assert!(current.visible[0].entry.ignored);
        app.tree_filtered(
            operation,
            vec![hit(), Entry::new("other.txt".into(), Kind::File)],
            false,
            None,
        );
        let current = app.workspace.explorer.as_mut().unwrap();
        assert_eq!(
            current
                .visible
                .iter()
                .map(|r| r.entry.path.as_str())
                .collect::<Vec<_>>(),
            ["src", "src/比較", "src/比較/結果.txt", "src/比較.txt"]
        );
        current.query = TextEdit::new("");
        app.filter_tree();
        let current = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(current.visible.len(), 2);
        assert_eq!(current.cursor.as_deref(), Some("README"));
        assert!(current.expanded.is_empty());
    }
}
