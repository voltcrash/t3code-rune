use crate::{Desktop, Request, Thread, workspace::label};
use rune::{El, KeyEvent, Tx, icon, row, tok};
use rune_kit::{Combobox, ComboboxEvent, ComboboxOption, ComboboxState, Placement, glyphs};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub(super) struct Choice {
    instance: String,
    model: String,
    name: String,
    provider: String,
}

#[derive(Clone, serde::Serialize)]
pub(super) struct Handoff {
    pub project: String,
    pub source: String,
    pub instance: String,
    pub model: String,
    #[serde(skip)]
    pub name: String,
}

#[derive(Default)]
pub(super) struct Picker {
    watch: Option<u64>,
    instance: String,
    request: String,
    choices: Vec<Choice>,
    options: Vec<ComboboxOption>,
    combo: ComboboxState,
    empty: String,
}

struct Selected<'a> {
    instance: Option<&'a str>,
    name: &'a str,
    index: Option<usize>,
}

impl Picker {
    pub(super) fn close(&mut self) {
        self.combo.close();
    }
}

impl Desktop {
    pub(super) fn close_stale_models(&mut self) {
        if let Some(watch) = self.models.watch
            && (self.workspace.terminal_watch() != Some(watch)
                || self.new_threads.active.is_some()
                || self.workspace.visible_thread().is_none_or(|t| {
                    t.permission
                        .as_ref()
                        .is_none_or(|p| p.instance != self.models.instance)
                        || self.workspace.questions.contains_key(&t.id)
                }))
        {
            self.models = Picker::default();
        }
    }

    pub(super) fn can_pick_thread_model(&self, thread: &Thread) -> bool {
        !self.pending
            && self.can_operate
            && self.workspace.terminal_watch().is_some()
            && thread.permission.is_some()
            && !self.workspace.questions.contains_key(&thread.id)
    }

    pub(super) fn open_composer_models(&mut self, tx: &mut Tx) {
        if self.new_threads.active.is_some() {
            self.permission_menu = None;
            self.new_threads.toggle_models(tx);
        } else {
            self.show_thread_models(tx);
        }
    }

    fn show_thread_models(&mut self, tx: &mut Tx) {
        if self.pending || !self.can_operate {
            return;
        }
        let Some(watch) = self.workspace.terminal_watch() else {
            return;
        };
        let Some(thread) = self.workspace.visible_thread() else {
            return;
        };
        if self.workspace.questions.contains_key(&thread.id) {
            return;
        }
        let Some(provider) = &thread.permission else {
            return;
        };
        self.permission_menu = None;
        let operation = Uuid::new_v4().to_string();
        self.models = Picker {
            watch: Some(watch),
            instance: provider.instance.clone(),
            request: operation.clone(),
            empty: "Loading models…".into(),
            ..Picker::default()
        };
        self.models
            .combo
            .step(ComboboxEvent::Toggle { current: None });
        self.write_request(None, Request::ThreadModels { watch, operation });
        tx.focus("search", true);
    }

    pub(super) fn model_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if !event.mods.shift
            || event.mods.alt
            || !event.is_command('m')
            || tx.focused() == Some("thread-terminal-grid")
            || self.palette.is_some()
            || self.pending
            || !self.can_operate
        {
            return false;
        }
        if tx.key_repeat() {
            return true;
        }
        if self.new_threads.active.is_none() && self.models.combo.is_open() {
            self.models.close();
            tx.focus("thread-model-trigger", true);
        } else {
            self.open_composer_models(tx);
        }
        true
    }

    pub(super) fn receive_models(
        &mut self,
        watch: u64,
        request: &str,
        instance: &str,
        options: Vec<Choice>,
        error: Option<String>,
    ) {
        if self.models.watch != Some(watch)
            || self.models.request != request
            || !self.models.combo.is_open()
        {
            return;
        }
        if let Some(error) = error {
            self.models.empty = error;
        } else if instance != self.models.instance {
            self.models.empty = "The provider changed. Reopen the model picker.".into();
        } else {
            self.models.options = options
                .iter()
                .enumerate()
                .map(|(index, c)| {
                    let mut item = ComboboxOption::new(index.to_string(), &c.name);
                    item.detail = c.provider.clone();
                    item
                })
                .collect();
            self.models.choices = options;
            if self.models.combo.search().text.is_empty()
                && let Some(thread) = self.workspace.visible_thread()
                && let Some(i) = self.selected_thread_model(thread).index
            {
                self.models.combo.step(ComboboxEvent::Highlight {
                    id: i.to_string(),
                    reveal: true,
                });
            }
            self.models.empty = "No matching models".into();
        }
    }

    fn selected_thread_model<'a>(&'a self, thread: &'a Thread) -> Selected<'a> {
        let current = thread.permission.as_ref().map(|p| p.instance.as_str());
        let handoff =
            current.and_then(|id| self.workspace.drafts.get(&thread.id)?.handoff_from(id));
        let instance = handoff.map(|h| h.instance.as_str()).or(current);
        let model = handoff.map_or(thread.model.as_str(), |h| h.model.as_str());
        let index = self
            .models
            .choices
            .iter()
            .position(|c| Some(c.instance.as_str()) == instance && c.model == model);
        let name = index
            .map(|i| self.models.choices[i].name.as_str())
            .or_else(|| handoff.map(|h| h.name.as_str()))
            .unwrap_or(model);
        Selected {
            instance,
            name,
            index,
        }
    }

    fn choose_thread_model(&mut self, index: &str, tx: &mut Tx) {
        let Some(watch) = self.models.watch else {
            return;
        };
        let Some(thread) = self.workspace.visible_thread() else {
            return;
        };
        let Some(source) = &thread.permission else {
            return;
        };
        let Some(choice) = index
            .parse::<usize>()
            .ok()
            .and_then(|i| self.models.choices.get(i))
        else {
            return;
        };
        if !self.can_operate
            || self.pending
            || Some(watch) != self.workspace.terminal_watch()
            || self.models.instance != source.instance
        {
            return;
        }
        tx.focus("message-input", true);
        if choice.instance != source.instance {
            let handoff = Handoff {
                project: thread.project.clone(),
                source: source.instance.clone(),
                instance: choice.instance.clone(),
                model: choice.model.clone(),
                name: choice.name.clone(),
            };
            let id = thread.id.clone();
            if let Some(draft) = self.workspace.drafts.get_mut(&id) {
                draft.set_model(Some(handoff), &choice.instance, &choice.model);
            }
            return;
        }
        self.send(Request::SetThreadModel {
            watch,
            thread: thread.id.clone(),
            instance: source.instance.clone(),
            model: choice.model.clone(),
            operation: Uuid::new_v4().to_string(),
        });
    }
}

pub(super) fn trigger(
    s: &Desktop,
    instance: Option<&str>,
    caption: &str,
    theme: &tok::Theme,
    available: bool,
    cx: &rune::Cx<Desktop>,
) -> El {
    let width = cx.layout_size("thread-titlebar").map_or(840., |r| r[0]);
    let mark = if width >= 404. {
        instance.and_then(|id| s.workspace.providers.composer_mark(id, s.dark(cx), theme))
    } else {
        None
    };
    row()
        .role("button")
        .label("Model")
        .help(caption)
        .focusable(available)
        .disabled(!available)
        .min_w(0.)
        .max_w((width - 158.).clamp(40., 280.))
        .shrink()
        .h(28.)
        .px(4.)
        .center_y()
        .gap(if mark.is_some() { 6. } else { 3. })
        .radius(5.)
        .hover(theme.text.alpha(0.07))
        .children(mark)
        .child(
            label(caption, 13., 500., theme.text_secondary)
                .min_w(0.)
                .shrink()
                .truncate(),
        )
        .children(
            (width < 404.).then(|| icon(&glyphs::CHEVRON_DOWN, 12.).color(theme.text_secondary)),
        )
}

pub(super) fn control(
    s: &Desktop,
    thread: &Thread,
    theme: &tok::Theme,
    cx: &rune::Cx<Desktop>,
    overlays: &mut Vec<El>,
) -> El {
    let available = s.can_pick_thread_model(thread);
    let Selected {
        instance,
        name,
        index,
    } = s.selected_thread_model(thread);
    let trigger = trigger(s, instance, name, theme, available, cx);
    if !s.models.combo.is_open() {
        return rune_kit::Popover::new("thread-model")
            .trigger(trigger.on_press_tx(Desktop::show_thread_models))
            .build(&s.contrast.kit(s.dark(cx)), overlays);
    }
    Combobox::new(
        "thread-model",
        &s.models.combo,
        &s.models.options,
        |s: &mut Desktop| &mut s.models.combo,
    )
    .label("Model")
    .search_placeholder("Search models")
    .empty_message(&s.models.empty)
    .current(index.map(|i| i.to_string()).as_deref())
    .disabled(!available)
    .trigger(trigger)
    .placement(Placement::TopEnd)
    .on_choose(Desktop::choose_thread_model)
    .build(&s.contrast.kit(s.dark(cx)), overlays)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_remote_provider_change_supersedes_the_draft_handoff_in_the_picker() {
        let mut app = Desktop::new();
        let mut thread: Thread = serde_json::from_value(json!({
            "id":"t", "project":"p", "title":"test", "status":"idle", "settled":false,
            "model":"model-a", "permission":{"instance":"a","mode":"default","label":"Default"}
        }))
        .unwrap();
        app.workspace
            .drafts
            .entry("t".into())
            .or_default()
            .set_model(
                Some(Handoff {
                    project: "p".into(),
                    source: "a".into(),
                    instance: "b".into(),
                    model: "model-b".into(),
                    name: "Beta".into(),
                }),
                "b",
                "model-b",
            );
        app.models.choices = vec![
            Choice {
                instance: "b".into(),
                model: "model-b".into(),
                name: "Beta".into(),
                provider: "B".into(),
            },
            Choice {
                instance: "c".into(),
                model: "model-c".into(),
                name: "Gamma".into(),
                provider: "C".into(),
            },
        ];
        let selected = app.selected_thread_model(&thread);
        assert_eq!(
            (selected.instance, selected.name, selected.index),
            (Some("b"), "Beta", Some(0))
        );
        thread.permission.as_mut().unwrap().instance = "c".into();
        thread.model = "model-c".into();
        let selected = app.selected_thread_model(&thread);
        assert_eq!(
            (selected.instance, selected.name, selected.index),
            (Some("c"), "Gamma", Some(1))
        );
        app.models.choices.clear();
        assert_eq!(app.selected_thread_model(&thread).name, "model-c");
    }
}
