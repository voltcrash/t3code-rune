use crate::{Desktop, Request, Thread, workspace::label};
use rune::{El, KeyEvent, Tx, icon, row, tok};
use rune_kit::{Combobox, ComboboxEvent, ComboboxOption, ComboboxState, Kit, Placement, glyphs};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub(super) struct Choice {
    id: String,
    name: String,
    provider: String,
}

#[derive(Default)]
pub(super) struct Picker {
    watch: Option<u64>,
    instance: String,
    request: String,
    options: Vec<ComboboxOption>,
    combo: ComboboxState,
    empty: String,
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
        self.models.combo.step(ComboboxEvent::Toggle {
            current: Some(&thread.model),
        });
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
        if self.new_threads.active.is_some() {
            self.permission_menu = None;
            self.new_threads.toggle_models(tx);
        } else if self.models.combo.is_open() {
            self.models.close();
            tx.focus("thread-model-trigger", true);
        } else {
            self.show_thread_models(tx);
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
                .into_iter()
                .map(|c| {
                    let mut item = ComboboxOption::new(c.id, c.name);
                    item.detail = c.provider;
                    item
                })
                .collect();
            self.models.empty = "No matching models".into();
        }
    }

    fn choose_thread_model(&mut self, model: &str, tx: &mut Tx) {
        let Some(watch) = self.models.watch else {
            return;
        };
        let Some(thread) = self.workspace.visible_thread() else {
            return;
        };
        if !self.can_operate
            || self.pending
            || Some(watch) != self.workspace.terminal_watch()
            || !self.models.options.iter().any(|o| o.id == model)
        {
            return;
        }
        tx.focus("message-input", true);
        self.send(Request::SetThreadModel {
            watch,
            thread: thread.id.clone(),
            instance: self.models.instance.clone(),
            model: model.into(),
            operation: Uuid::new_v4().to_string(),
        });
    }
}

pub(super) fn trigger(caption: &str, theme: &tok::Theme, available: bool, width: f32) -> El {
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
        .gap(3.)
        .radius(5.)
        .hover(theme.text.alpha(0.07))
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
    let available = !s.pending
        && s.can_operate
        && s.workspace.terminal_watch().is_some()
        && thread.permission.is_some();
    let name = s
        .models
        .options
        .iter()
        .find(|o| o.id == thread.model)
        .map_or(thread.model.as_str(), |o| o.label.as_str());
    let trigger = trigger(
        name,
        theme,
        available,
        cx.layout_size("thread-titlebar").map_or(840., |r| r[0]),
    );
    if !s.models.combo.is_open() {
        return rune_kit::Popover::new("thread-model")
            .trigger(trigger.on_press_tx(Desktop::show_thread_models))
            .build(&Kit::neutral(cx.system_dark()), overlays);
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
    .current(Some(&thread.model))
    .disabled(!available)
    .trigger(trigger)
    .placement(Placement::TopEnd)
    .on_choose(Desktop::choose_thread_model)
    .build(&Kit::neutral(cx.system_dark()), overlays)
}
