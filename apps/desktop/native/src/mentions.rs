use crate::{Desktop, Request, attachments::Target};
use rune::{Align, El, Key, KeyEvent, Side, TextEdit, Tx};
use rune_kit::{Kit, PopupItem, PopupList, glyphs};
use serde::Deserialize;
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Default, Deserialize)]
pub(super) struct Labels {
    search: String,
    commands: String,
    switch_model: String,
    searching: String,
    skills: String,
    no_skills: String,
    empty: String,
}

#[derive(Deserialize)]
pub(super) struct Entry {
    path: String,
    kind: String,
    link: String,
    label: Option<String>,
    description: Option<String>,
}

impl Entry {
    fn model_command(query: &str, description: &str) -> Option<Self> {
        let query = query.to_lowercase();
        let mut name = "model".chars();
        if !query.chars().all(|c| name.by_ref().any(|next| c == next))
            && !description.to_lowercase().contains(&query)
        {
            return None;
        }
        Some(Self {
            path: "/model".into(),
            kind: "model".into(),
            label: Some("/model".into()),
            description: Some(description.into()),
            link: String::new(),
        })
    }

    fn name(&self) -> &str {
        self.label
            .as_deref()
            .unwrap_or_else(|| self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path))
    }
    fn choose(&self, edit: &mut TextEdit, mut range: Range<usize>) {
        if self.kind == "model" {
            edit.undo_group(|edit| {
                edit.set_selection(range.start, range.end);
                edit.insert("");
            });
            return;
        }
        if edit.text[range.end..].starts_with(' ') {
            range.end += 1;
        }
        let prefix = match self.kind.as_str() {
            "directory" => "t3-folder:",
            "skill" => "t3-skill:",
            _ => "t3-file:",
        };
        edit.undo_group(|edit| {
            edit.replace_with_pill(range, format!("{prefix}{}", self.link), self.name());
            edit.insert(" ");
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    File,
    Skill,
    Command,
}

#[derive(PartialEq)]
struct Trigger {
    owner: Target,
    watch: u64,
    root: String,
    instance: Option<String>,
    model: bool,
    kind: Kind,
    range: Range<usize>,
    query: String,
}

fn query(edit: &TextEdit) -> Option<(Range<usize>, &str, Kind)> {
    if edit.anchor != edit.focus || edit.is_composing() {
        return None;
    }
    let before = edit.text.get(..edit.focus)?;
    let token = before.rsplit([' ', '\n', '\t', '\r']).next()?;
    let (query, kind) = if let Some(q) = token.strip_prefix('@') {
        (q, Kind::File)
    } else if let Some(q) = token.strip_prefix('/') {
        let tail = edit.text[edit.focus..]
            .split([' ', '\t', '\n', '\r'])
            .next()
            .unwrap_or("");
        if q.contains(['/', '\\']) || tail.contains(['/', '\\']) {
            return None;
        }
        (q, Kind::Command)
    } else {
        static CURRENCY: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"^\p{Sc}").unwrap());
        static AMOUNT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"^\.?[0-9][0-9,._]*(?:[kKmMbBtT]|[eE][0-9]+)?[.,;:!?)]*$").unwrap()
        });
        let q = &token[CURRENCY.find(token)?.end()..];
        if AMOUNT.is_match(q) {
            return None;
        }
        (q, Kind::Skill)
    };
    let range = before.len() - token.len()..before.len();
    if edit
        .pills()
        .any(|(pill, _)| pill.end > range.start && pill.start < range.end)
    {
        return None;
    }
    Some((range, query, kind))
}

pub(super) fn serialized_selection(edit: &TextEdit, selection: Range<usize>) -> Option<String> {
    let mut from = selection.start;
    let mut body = None;
    for (range, id) in edit.pills() {
        if range.start < selection.start || range.end > selection.end {
            continue;
        }
        if let Some(link) = id
            .strip_prefix("t3-file:")
            .or_else(|| id.strip_prefix("t3-folder:"))
            .or_else(|| id.strip_prefix("t3-skill:"))
            .or_else(|| id.strip_prefix("t3-attachment:"))
        {
            let body = body.get_or_insert_with(String::new);
            body.push_str(&edit.text[from..range.start]);
            body.push_str(link);
            from = range.end;
        }
    }
    if let Some(body) = &mut body {
        body.push_str(&edit.text[from..selection.end]);
    }
    body
}

pub(super) fn prompt(edit: &TextEdit) -> String {
    serialized_selection(edit, 0..edit.text.len()).unwrap_or_else(|| edit.text.clone())
}

pub(super) fn copy_selection(edit: &TextEdit) -> Option<String> {
    let (start, end) = edit.range();
    serialized_selection(edit, start..end)
}

struct Search {
    trigger: Trigger,
    due: Option<Instant>,
    operation: u64,
    dismissed: bool,
    loaded: bool,
    needs_refresh: bool,
    restore: Option<String>,
    entries: Vec<Entry>,
    active: usize,
}

impl Search {
    fn waiting_for_selection(&self) -> bool {
        (!self.loaded || !self.entries.is_empty())
            && self.restore.as_deref().is_some_and(|path| {
                self.entries
                    .get(self.active)
                    .is_none_or(|entry| entry.path != path)
            })
    }

    fn hover(&mut self, index: usize) {
        if index < self.entries.len() && !self.waiting_for_selection() {
            self.select(index);
        }
    }

    fn select(&mut self, index: usize) {
        self.active = index;
        self.restore = Some(self.entries[index].path.clone());
    }
}

#[derive(Default)]
pub(super) struct Picker {
    pub(super) labels: Labels,
    search: Option<Search>,
    ready: Option<u64>,
}

impl Picker {
    pub(super) fn skills_changed(&mut self, watch: u64, instance: &str) {
        if let Some(s) = self.search.as_mut()
            && !s.dismissed
            && s.trigger.watch == watch
            && s.trigger.instance.as_deref() == Some(instance)
        {
            s.needs_refresh = true;
        }
    }

    pub(super) fn slash_settings_changed(&mut self) {
        if let Some(s) = &mut self.search
            && !s.dismissed
            && s.trigger.kind == Kind::Command
        {
            s.needs_refresh = true;
        }
    }

    pub(super) fn connection(&mut self, watch: u64, ready: bool) {
        if ready {
            self.ready = Some(watch);
        } else if self.ready == Some(watch) {
            self.ready = None;
            if !self
                .search
                .as_ref()
                .is_some_and(|s| s.dismissed && s.trigger.kind == Kind::Command)
            {
                self.search = None;
            }
        }
    }

    pub(super) fn receive(&mut self, operation: u64, entries: Option<Vec<Entry>>) {
        if let Some(s) = &mut self.search
            && s.operation == operation
            && !s.dismissed
            && !s.needs_refresh
        {
            if let Some(entries) = entries {
                let restored = s
                    .restore
                    .as_ref()
                    .and_then(|path| entries.iter().position(|entry| &entry.path == path));
                s.active = restored.unwrap_or(0);
                if restored.is_some() || s.trigger.kind != Kind::Command {
                    s.restore = None;
                }
                s.entries = entries;
            }
            s.loaded = true;
        }
    }

    pub(super) fn view(
        &self,
        owner: &Target,
        cx: &rune::Cx<Desktop>,
        kit: &Kit,
        overlays: &mut Vec<El>,
    ) {
        let Some(s) = self
            .search
            .as_ref()
            .filter(|s| !s.dismissed && s.trigger.owner == *owner)
        else {
            return;
        };
        let key = owner.field_key();
        let operation = s.operation;
        let items = s.entries.iter().map(|e| {
            let parent = e
                .path
                .strip_suffix(e.name())
                .unwrap_or("")
                .trim_end_matches(['/', '\\']);
            let title = if s.trigger.kind == Kind::Command && e.kind == "skill" {
                format!("/skill:{}", e.path)
            } else {
                e.name().to_owned()
            };
            let item = PopupItem::new(&e.path, title)
                .description(e.description.as_deref().unwrap_or(parent));
            if e.kind == "model" {
                return item;
            }
            let glyph = if e.kind == "skill" {
                &glyphs::SPARKLE
            } else if e.kind == "directory" {
                &glyphs::FOLDER
            } else {
                rune_kit::file_icon(&e.path)
            };
            item.leading(rune::icon(glyph, 16.).color(kit.c.text_weak))
        });
        PopupList::<Desktop>::new("composer-file-mentions", key)
            .anchored(key, Side::Top, Align::Start, 6.)
            .width(cx.layout_size(key).map_or(320., |size| size[0]))
            .max_height(290.)
            .label(match s.trigger.kind {
                Kind::Skill => &self.labels.skills,
                Kind::Command => &self.labels.commands,
                Kind::File => &self.labels.search,
            })
            .empty_message(if s.loaded {
                if s.trigger.kind != Kind::File {
                    &self.labels.no_skills
                } else {
                    &self.labels.empty
                }
            } else {
                &self.labels.searching
            })
            .items(items)
            .active((!s.entries.is_empty() && !s.waiting_for_selection()).then_some(s.active))
            .on_point(move |app, index| {
                if let Some(s) = &mut app.mentions.search
                    && s.operation == operation
                {
                    s.hover(index);
                }
            })
            .on_choose(move |app, path, tx| app.choose_mention(operation, path, tx))
            .build(kit, overlays);
    }
}

impl Target {
    fn field_key(&self) -> &'static str {
        match self {
            Self::Thread(_) => "message-input",
            Self::Project(_) => "new-thread-prompt",
        }
    }
}

impl Desktop {
    fn mention_trigger(&self, focus: Option<&str>) -> Option<Trigger> {
        if self.pending || !self.can_operate {
            return None;
        }
        let watch = self.workspace.feed?;
        let (id, draft, root, edit) = match focus? {
            "message-input" => {
                let id = &self.workspace.visible_thread()?.id;
                (
                    id.as_str(),
                    false,
                    self.thread_workspace_root()?,
                    &self.workspace.drafts.get(id)?.editor,
                )
            }
            "new-thread-prompt" => {
                let id = self.new_threads.active.as_deref()?;
                (
                    id,
                    true,
                    self.workspace.project_root(id)?,
                    self.new_threads.prompt(id)?,
                )
            }
            _ => return None,
        };
        let (range, q, kind) = query(edit)?;
        if kind != Kind::Command && Some(watch) != self.mentions.ready {
            return None;
        }
        let model = kind == Kind::Command
            && (draft || self.can_pick_thread_model(self.workspace.visible_thread()?));
        let instance = match kind {
            Kind::Skill => Some(self.status_target()?.2?.to_owned()),
            Kind::Command if Some(watch) == self.mentions.ready => {
                self.status_target().and_then(|t| t.2).map(str::to_owned)
            }
            Kind::File if !self.can_read_files => return None,
            _ => None,
        };
        if kind == Kind::Command && !model && instance.is_none() {
            return None;
        }
        let target = if draft {
            Target::Project(id.into())
        } else {
            Target::Thread(id.into())
        };
        if !target.active(self) {
            return None;
        }
        Some(Trigger {
            owner: target,
            watch,
            root: root.into(),
            instance,
            model,
            kind,
            range,
            query: q.chars().take(256).collect(),
        })
    }

    pub(super) fn sync_mentions(&mut self, tx: &mut Tx) -> bool {
        let trigger = self.mention_trigger(tx.focused());
        if let (Some(s), Some(current)) = (self.mentions.search.as_mut(), trigger.as_ref())
            && s.dismissed
            && s.trigger.kind == Kind::Command
            && current.kind == Kind::Command
        {
            s.trigger.instance.clone_from(&current.instance);
            s.trigger.model = current.model;
        }
        let previous = self.mentions.search.as_ref();
        let changed = trigger.as_ref() != previous.map(|s| &s.trigger)
            || previous.is_some_and(|s| s.needs_refresh);
        if changed {
            let mut restore = None;
            if let Some(s) = previous
                && s.needs_refresh
                && trigger.as_ref() == Some(&s.trigger)
            {
                restore = s
                    .restore
                    .clone()
                    .or_else(|| s.entries.get(s.active).map(|entry| entry.path.clone()));
            }
            if let Some(s) = self.mentions.search.take()
                && s.due.is_none()
                && !s.loaded
            {
                self.send_mention_search(&s.trigger, s.operation, true);
            }
            if let Some(trigger) = trigger {
                let local = trigger.kind == Kind::Command && trigger.instance.is_none();
                let entries = if trigger.model {
                    Entry::model_command(&trigger.query, &self.mentions.labels.switch_model)
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
                let delay = Duration::from_millis(120);
                self.mentions.search = Some(Search {
                    trigger,
                    due: (!local).then(|| Instant::now() + delay),
                    operation: self.next_request_id(),
                    dismissed: false,
                    loaded: local,
                    needs_refresh: false,
                    restore,
                    entries,
                    active: 0,
                });
                if !local {
                    tx.wake_after(delay);
                }
            }
            return true;
        }
        if let Some(s) = &mut self.mentions.search
            && !s.dismissed
            && let Some(due) = s.due
        {
            if let Some(delay) = due.checked_duration_since(Instant::now()) {
                tx.wake_after(delay);
            } else {
                let mut s = self.mentions.search.take().unwrap();
                s.due = None;
                self.send_mention_search(&s.trigger, s.operation, false);
                self.mentions.search = Some(s);
            }
        }
        false
    }

    fn send_mention_search(&mut self, t: &Trigger, operation: u64, cancel: bool) {
        let (draft, target) = match &t.owner {
            Target::Project(id) => (true, id),
            Target::Thread(id) => (false, id),
        };
        self.write_request(
            None,
            Request::ComposerFiles {
                watch: t.watch,
                operation,
                draft,
                target,
                root: &t.root,
                instance: t.instance.as_deref(),
                slash: t.kind == Kind::Command,
                model: t.model.then(|| self.mentions.labels.switch_model.clone()),
                query: (!cancel).then_some(t.query.as_str()),
            },
        );
    }

    fn choose_mention(&mut self, operation: u64, path: &str, tx: &mut Tx) {
        let current = self.mention_trigger(tx.focused());
        let Some(s) = self.mentions.search.take_if(|s| {
            !s.dismissed
                && !s.needs_refresh
                && s.operation == operation
                && current.as_ref() == Some(&s.trigger)
        }) else {
            return;
        };
        let Some(entry) = s.entries.iter().find(|e| e.path == path) else {
            self.mentions.search = Some(s);
            return;
        };
        let mut open_model = false;
        s.trigger.owner.edit(self, |_, edit| {
            entry.choose(edit, s.trigger.range.clone());
            open_model = entry.kind == "model";
        });
        if open_model {
            self.open_composer_models(tx);
        }
    }

    pub(super) fn mention_key(&mut self, owner: &Target, event: &KeyEvent, tx: &mut Tx) -> bool {
        if event.mods != Default::default()
            || !matches!(
                event.key,
                Key::Escape | Key::Up | Key::Down | Key::Tab | Key::Enter
            )
        {
            return false;
        }
        self.sync_mentions(tx);
        let Some(s) = self
            .mentions
            .search
            .as_mut()
            .filter(|s| !s.dismissed && s.trigger.owner == *owner)
        else {
            return false;
        };
        let len = s.entries.len();
        match event.key {
            Key::Enter | Key::Tab if s.waiting_for_selection() => {}
            Key::Enter if len == 0 && !s.loaded && s.trigger.kind == Kind::Skill => {}
            Key::Escape => s.dismissed = true,
            Key::Down if len > 0 => {
                let index = if s.waiting_for_selection() {
                    0
                } else {
                    (s.active + 1) % len
                };
                s.select(index);
            }
            Key::Up if len > 0 => {
                let index = if s.waiting_for_selection() {
                    len - 1
                } else {
                    (s.active + len - 1) % len
                };
                s.select(index);
            }
            Key::Tab | Key::Enter if len > 0 => {
                let path = s.entries[s.active].path.clone();
                let operation = s.operation;
                self.choose_mention(operation, &path, tx);
            }
            _ => return false,
        }
        true
    }
}

pub(super) fn chip(id: &str) -> rune::PillStyle {
    let glyph = if id.starts_with("t3-skill:") {
        &glyphs::SPARKLE
    } else if id.starts_with("t3-folder:") {
        &glyphs::FOLDER
    } else {
        rune_kit::file_icon(id.strip_suffix(')').unwrap_or(id))
    };
    rune::PillStyle::new().icon(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_files_with_their_paths_and_undo_each_pick_in_one_step() {
        let entry = Entry {
            path: "src/nested/code.rs".into(),
            kind: "file".into(),
            label: None,
            description: None,
            link: "[code.rs](src/nested/code.rs)".into(),
        };
        let mut edit = TextEdit::new("Review @co here @dir");
        edit.set_caret("Review @co".len());
        let (range, term, _) = query(&edit).unwrap();
        assert_eq!(term, "co");
        entry.choose(&mut edit, range);
        assert_eq!(
            prompt(&edit),
            "Review [code.rs](src/nested/code.rs) here @dir"
        );
        edit.set_caret(edit.text.len());
        let (range, _, _) = query(&edit).unwrap();
        let folder = Entry {
            path: "src/utils".into(),
            kind: "directory".into(),
            label: None,
            description: None,
            link: "[utils](src/utils)".into(),
        };
        folder.choose(&mut edit, range);
        assert_eq!(
            prompt(&edit),
            "Review [code.rs](src/nested/code.rs) here [utils](src/utils) "
        );
        edit.set_selection("Review ".len(), "Review code.rs".len());
        assert_eq!(
            copy_selection(&edit).as_deref(),
            Some("[code.rs](src/nested/code.rs)")
        );
        edit.set_selection(0, "Review".len());
        assert!(copy_selection(&edit).is_none());
        edit.undo();
        edit.undo();
        assert_eq!(prompt(&edit), "Review @co here @dir");
    }
    #[test]
    fn undoing_the_model_command_restores_its_original_caret() {
        let mut edit = TextEdit::new("Review /mo tomorrow");
        let caret = "Review /mo".len();
        edit.set_caret(caret);
        let (range, term, kind) = query(&edit).unwrap();
        assert_eq!(kind, Kind::Command);
        Entry::model_command(term, "Switch response model for this thread")
            .unwrap()
            .choose(&mut edit, range);
        assert_eq!(edit.text, "Review  tomorrow");
        edit.undo();
        assert_eq!(edit.text, "Review /mo tomorrow");
        assert_eq!((edit.anchor, edit.focus), (caret, caret));
        edit.insert("d");
        assert_eq!(edit.text, "Review /mod tomorrow");
    }

    #[test]
    fn enter_waits_for_a_highlighted_skill_to_refresh() {
        let mut app = Desktop::new();
        app.can_operate = true;
        app.workspace.feed = Some(1);
        app.mentions.connection(1, true);
        app.workspace.replace(serde_json::from_value(serde_json::json!([{
            "id": "p", "title": "Project", "path": "/project", "archived": false, "scratch": false
        }])).unwrap(), vec![]);
        app.new_threads.configure(
            "p".into(),
            serde_json::from_value(serde_json::json!([{
                "id": "model", "name": "Model", "provider": "Replay", "runtime": "default",
                "instance": "replay", "preferred": true
            }]))
            .unwrap(),
            Default::default(),
            false,
            "",
        );
        app.new_threads.editable_files().unwrap().1.insert("$rev");
        app.text_paste =
            serde_json::from_value(serde_json::json!({"threshold": 8, "input": 500, "file": 500}))
                .unwrap();
        assert!(
            Target::Project("p".into())
                .paste(&mut app, &rune::ClipboardContent::new("long folded text"))
        );
        let (files, edit) = app.new_threads.editable_files().unwrap();
        let staged = std::path::PathBuf::from(&files[0].path);
        edit.undo();
        assert!(staged.exists());
        let mut rt = rune::Runtime::new(
            app,
            |_, _| rune::el().key("new-thread-prompt").focusable(true),
            (320., 200.),
            Default::default(),
        );
        rt.frame(0.);
        rt.update(|_, tx| tx.focus("new-thread-prompt", true));
        rt.frame(0.);
        rt.update(|s, tx| {
            s.sync_mentions(tx);
            let operation = s.mentions.search.as_ref().unwrap().operation;
            let row = || Entry {
                path: "review".into(),
                kind: "skill".into(),
                link: "$review".into(),
                label: None,
                description: None,
            };
            s.mentions.receive(operation, Some(vec![row()]));
            s.mentions.skills_changed(1, "replay");
            let enter = KeyEvent {
                key: Key::Enter,
                mods: Default::default(),
            };
            let owner = Target::Project("p".into());
            assert!(s.mention_key(&owner, &enter, tx));
            assert_eq!(s.new_threads.prompt("p").unwrap().text, "$rev");
            let fresh = s.mentions.search.as_ref().unwrap().operation;
            s.mentions.receive(fresh, Some(vec![row()]));
            assert!(s.mention_key(&owner, &enter, tx));
            assert_eq!(prompt(s.new_threads.prompt("p").unwrap()), "$review ");

            assert!(
                !staged.exists(),
                "choosing the skill removed the last redo reference"
            );

            let (_, edit) = s.new_threads.editable_files().unwrap();
            *edit = TextEdit::new("/");
            edit.set_caret(1);
            s.sync_mentions(tx);
            let operation = s.mentions.search.as_ref().unwrap().operation;
            let model = || Entry::model_command("", "Switch model").unwrap();
            s.mentions.receive(operation, Some(vec![model(), row()]));
            s.mentions.search.as_mut().unwrap().active = 1;
            s.mentions.skills_changed(1, "replay");
            assert!(s.mention_key(&owner, &enter, tx));
            assert_eq!(s.new_threads.prompt("p").unwrap().text, "/");
            let fresh = s.mentions.search.as_ref().unwrap().operation;
            let search = s.mentions.search.as_ref().unwrap();
            assert_eq!(search.entries[0].path, "/model");
            assert!(search.waiting_for_selection());
            s.mentions.search.as_mut().unwrap().hover(0);
            assert!(s.mention_key(&owner, &enter, tx));
            assert_eq!(s.new_threads.prompt("p").unwrap().text, "/");
            s.mentions.receive(fresh, None);
            assert_eq!(
                s.mentions.search.as_ref().unwrap().entries[0].path,
                "/model"
            );
            let tab = KeyEvent {
                key: Key::Tab,
                mods: Default::default(),
            };
            s.mentions.search.as_mut().unwrap().hover(0);
            assert!(s.mention_key(&owner, &enter, tx));
            assert!(s.mention_key(&owner, &tab, tx));
            assert_eq!(s.new_threads.prompt("p").unwrap().text, "/");
            s.mentions.skills_changed(1, "replay");
            s.sync_mentions(tx);
            let fresh = s.mentions.search.as_ref().unwrap().operation;
            s.mentions.receive(fresh, Some(vec![model(), row()]));
            assert!(s.mention_key(&owner, &enter, tx));
            assert_eq!(prompt(s.new_threads.prompt("p").unwrap()), "$review ");

            let (_, edit) = s.new_threads.editable_files().unwrap();
            *edit = TextEdit::new("/mo");
            edit.set_caret(3);
            assert!(s.mention_key(
                &owner,
                &KeyEvent {
                    key: Key::Down,
                    mods: Default::default()
                },
                tx
            ));
            let fresh = s.mentions.search.as_ref().unwrap().operation;
            s.mentions.receive(fresh, Some(vec![row(), model()]));
            assert!(s.mention_key(&owner, &tab, tx));
            assert_eq!(s.new_threads.prompt("p").unwrap().text, "");
            s.models.close();

            let (_, edit) = s.new_threads.editable_files().unwrap();
            *edit = TextEdit::new("/compact");
            edit.set_caret(edit.text.len());
            assert!(!s.mention_key(&owner, &enter, tx));
            s.mentions.connection(1, false);
            s.sync_mentions(tx);
            assert!(s.mention_key(
                &owner,
                &KeyEvent {
                    key: Key::Escape,
                    mods: Default::default()
                },
                tx
            ));
            s.mentions.connection(1, true);
            s.sync_mentions(tx);
            assert!(s.mentions.search.as_ref().unwrap().dismissed);
        });
    }
    #[test]
    fn skill_tokens_use_the_provider_name_and_leave_currency_amounts_alone() {
        for token in ["$20", "€0.99", "$1.5k", "₹2,500!"] {
            let mut edit = TextEdit::new(token);
            edit.set_caret(token.len());
            assert!(query(&edit).is_none());
        }
        let mut edit = TextEdit::new("Run £rev");
        edit.set_caret(edit.text.len());
        let (range, query, skill) = query(&edit).unwrap();
        assert_eq!(skill, Kind::Skill);
        assert_eq!(query, "rev");
        Entry {
            path: "review-code".into(),
            kind: "skill".into(),
            link: "$review-code".into(),
            label: Some("Review Code".into()),
            description: None,
        }
        .choose(&mut edit, range);
        assert_eq!(edit.text, "Run Review Code ");
        assert_eq!(prompt(&edit), "Run $review-code ");
        edit.undo();
        assert_eq!(prompt(&edit), "Run £rev");
    }
}
