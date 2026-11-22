use super::{Desktop, Request, composer};
use crate::attachments::Selection as Attachment;
use rune::{El, TextEdit, TextStyle, col, row, spacer, text, text_input, tok};
use rune_kit::{Combobox, ComboboxOption, ComboboxState};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::permissions;
pub(super) type ProviderPermissions = HashMap<String, Vec<permissions::OptionRow>>;

#[derive(Clone, Deserialize)]
pub(super) struct Choice {
    id: String,
    name: String,
    provider: String,
    runtime: String,
    instance: String,
    preferred: bool,
}
#[derive(Default)]
pub(super) struct NewThreads {
    pub active: Option<String>,
    project_picker: ComboboxState,
    drafts: HashMap<String, LaunchDraft>,
    catalog_watch: Option<(u64, String, u64)>,
    catalog_dirty: bool,
}

struct LaunchDraft {
    prompt: TextEdit,
    files: Vec<Attachment>,
    base: TextEdit,
    checkout_branch: String,
    worktree: bool,
    model: Option<String>,
    picked: Option<Choice>,
    choices: Vec<Choice>,
    permissions: ProviderPermissions,
    runtime_override: Option<String>,
    picker: ComboboxState,
    operation: String,
    uncertain: bool,
}

impl NewThreads {
    pub(super) fn provider_instance(&self) -> Option<&str> {
        self.drafts
            .get(self.active.as_deref()?)?
            .displayed_choice()
            .map(|c| c.instance.as_str())
    }
    pub(crate) fn details_checkout(&self) -> (bool, bool) {
        self.active
            .as_ref()
            .and_then(|id| self.drafts.get(id))
            .map_or((false, false), |d| (d.worktree, !d.uncertain))
    }
    pub(crate) fn set_details_checkout(&mut self, worktree: bool) {
        if self.details_checkout().1 {
            self.current().worktree = worktree;
        }
    }
    pub(super) fn has_draft(&self, project: &str) -> bool {
        self.drafts.contains_key(project)
    }
    pub(super) fn reopen(&mut self, project: String) {
        self.active = Some(project);
        self.project_picker = ComboboxState::default();
        self.current().picker = ComboboxState::default();
    }
    pub fn configure(
        &mut self,
        project: String,
        choices: Vec<Choice>,
        permissions: ProviderPermissions,
        worktree: bool,
        base: &str,
    ) {
        let draft = self
            .drafts
            .entry(project.clone())
            .or_insert_with(|| LaunchDraft {
                prompt: TextEdit::new(""),
                files: vec![],
                base: TextEdit::new(base),
                checkout_branch: base.to_owned(),
                worktree,
                model: None,
                picked: None,
                choices: Vec::new(),
                permissions: Default::default(),
                runtime_override: None,
                picker: ComboboxState::default(),
                operation: Uuid::new_v4().to_string(),
                uncertain: false,
            });
        self.active = Some(project.clone());
        self.project_picker = ComboboxState::default();
        if draft.uncertain {
            return;
        }
        draft.update_models(choices, permissions);
        draft.checkout_branch = base.to_owned();
        draft.picker = ComboboxState::default();
    }
    pub fn mark_unconfirmed(&mut self, project: &str, uncertain: bool) {
        if let Some(draft) = self.drafts.get_mut(project) {
            draft.uncertain = uncertain;
        }
    }
    pub fn forget(&mut self, project: &str) {
        self.drafts.remove(project);
        if self.active.as_deref() == Some(project) {
            self.active = None;
        }
    }
    pub fn retain(&mut self, available: impl Fn(&str) -> bool) {
        self.drafts.retain(|id, _| available(id));
        if self
            .active
            .as_ref()
            .is_some_and(|id| !self.drafts.contains_key(id))
        {
            self.active = None;
        }
    }
    pub(super) fn paste_text(&mut self, value: &str) {
        let draft = self.current();
        if !draft.uncertain {
            draft.prompt.paste(value);
        }
    }
    pub(super) fn editable_files(&mut self) -> Option<&mut Vec<Attachment>> {
        let draft = self.current();
        (!draft.uncertain).then_some(&mut draft.files)
    }
    pub(super) fn picker_options(&self) -> Option<(&[permissions::OptionRow], &str)> {
        let draft = self.drafts.get(self.active.as_ref()?)?;
        if draft.uncertain || draft.permission_options().is_empty() {
            return None;
        }
        Some((
            draft.permission_options(),
            draft.permission().map_or("", |p| p.mode.as_str()),
        ))
    }
    pub(super) fn select_permission(&mut self, mode: &str) -> bool {
        if self.active.is_none() {
            return false;
        }
        let draft = self.current();
        if draft.uncertain
            || !draft
                .permission_options()
                .iter()
                .any(|p| p.mode == mode && p.denied.is_none())
        {
            return false;
        }
        draft.runtime_override = Some(mode.into());
        true
    }
    pub(super) fn toggle_models(&mut self, tx: &mut rune::Tx) {
        let draft = self.current();
        if draft.uncertain {
            return;
        }
        draft.picker.step(rune_kit::ComboboxEvent::Toggle {
            current: draft.model.as_deref(),
        });
        tx.focus(
            if draft.picker.is_open() {
                "search"
            } else {
                "launch-model-trigger"
            },
            true,
        );
    }
    fn current(&mut self) -> &mut LaunchDraft {
        self.drafts
            .get_mut(self.active.as_deref().expect("open thread draft"))
            .expect("known project")
    }
}

impl LaunchDraft {
    fn update_models(&mut self, choices: Vec<Choice>, permissions: ProviderPermissions) {
        if self.picked.is_none() {
            self.model = choices
                .iter()
                .find(|choice| choice.preferred)
                .or_else(|| {
                    choices
                        .iter()
                        .find(|choice| Some(&choice.id) == self.model.as_ref())
                })
                .or_else(|| choices.first())
                .map(|choice| choice.id.clone());
        }
        self.choices = choices;
        self.permissions = permissions;
    }
    fn select_model(&mut self, id: &str) {
        if let Some(choice) = self.choices.iter().find(|choice| choice.id == id) {
            self.model = Some(choice.id.clone());
            self.picked = Some(choice.clone());
        }
    }
    fn choice(&self) -> Option<&Choice> {
        self.choices
            .iter()
            .find(|choice| Some(&choice.id) == self.model.as_ref())
    }
    fn displayed_choice(&self) -> Option<&Choice> {
        self.choice().or(self.picked.as_ref())
    }
    fn permission_options(&self) -> &[permissions::OptionRow] {
        self.choice()
            .and_then(|choice| self.permissions.get(&choice.instance))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
    fn permission(&self) -> Option<&permissions::OptionRow> {
        let choice = self.choice()?;
        let mode = self.runtime_override.as_deref().unwrap_or(&choice.runtime);
        self.permission_options()
            .iter()
            .find(|p| p.mode == mode && p.denied.is_none())
    }
    fn ready(&self) -> bool {
        self.permission().is_some()
            && (!self.prompt.text.trim().is_empty() || !self.files.is_empty())
            && (!self.worktree || !self.base.text.trim().is_empty())
    }
}

impl Desktop {
    fn model_watch_target(&self) -> Option<(u64, &str)> {
        let id = self.new_threads.active.as_deref()?;
        let draft = self.new_threads.drafts.get(id)?;
        if draft.uncertain || !self.can_operate {
            return None;
        }
        Some((self.workspace.feed?, id))
    }

    pub(super) fn sync_draft_model_watch(&mut self) {
        if self.pending {
            return;
        }
        let target = self.model_watch_target();
        let previous = self.new_threads.catalog_watch.as_ref();
        if !self.new_threads.catalog_dirty
            && target == previous.map(|(watch, id, _)| (*watch, id.as_str()))
        {
            return;
        }
        let next = target.map(|(watch, id)| (watch, id.to_owned()));
        self.new_threads.catalog_dirty = false;
        let old = self.new_threads.catalog_watch.take();
        if let Some((watch, project)) = next {
            let operation = self.next_request_id();
            if self.write_request(
                Some(operation),
                Request::WatchDraftModels {
                    watch,
                    project: Some(&project),
                },
            ) {
                self.new_threads.catalog_watch = Some((watch, project, operation));
            }
        } else if let Some((watch, _, _)) = old {
            self.write_request(
                None,
                Request::WatchDraftModels {
                    watch,
                    project: None,
                },
            );
        }
    }

    pub(super) fn receive_draft_models(
        &mut self,
        watch: u64,
        project: &str,
        operation: Option<u64>,
        choices: Vec<Choice>,
        permissions: ProviderPermissions,
    ) {
        if self.model_watch_target() == Some((watch, project))
            && self
                .new_threads
                .catalog_watch
                .as_ref()
                .is_some_and(|(w, id, op)| *w == watch && id == project && Some(*op) == operation)
        {
            if self.pending {
                self.new_threads.catalog_dirty = true;
                return;
            }
            self.new_threads
                .current()
                .update_models(choices, permissions);
        }
    }
    pub fn prepare_thread(&mut self, project: &str) {
        if self.can_operate && !self.pending {
            self.send(Request::PrepareThread {
                project: project.to_owned(),
            });
        }
    }
    fn launch(&mut self) {
        if self.pending || !self.can_operate {
            return;
        }
        let Some(project) = self.new_threads.active.clone() else {
            return;
        };
        let draft = self.new_threads.current();
        if !draft.ready() {
            return;
        }
        let Some(choice) = draft.choice() else {
            return;
        };
        let request = Request::LaunchThread {
            runtime: choice.runtime.clone(),
            runtime_override: draft.runtime_override.clone(),
            project,
            model: draft.model.clone().unwrap(),
            text: draft.prompt.text.clone(),
            files: draft.files.clone(),
            operation: draft.operation.clone(),
            base: draft.worktree.then(|| draft.base.text.trim().to_owned()),
        };
        self.send(request);
    }
}

pub(super) fn view(
    s: &Desktop,
    record: &crate::workspace::Project,
    theme: &tok::Theme,
    cx: &rune::Cx<Desktop>,
    lane: crate::workspace::thread_details::Lane,
    overlays: &mut Vec<El>,
    draft_preview: &mut Option<El>,
) -> El {
    let (project, title, scratch) = (record.id.as_str(), record.title.as_str(), record.scratch);
    let dark = s.dark(cx);
    let kit = &s.contrast.kit(dark);
    let width = cx
        .layout_size("thread-titlebar")
        .map_or(840., |size| size[0]);
    let [viewport_width, viewport_height] = cx.layout_size("workspace").unwrap_or([1100., 780.]);
    let heading_width = (width - lane.left - lane.right)
        .clamp(0., s.chat_width.limit().unwrap_or(1024.).min(1024.));
    let compact = width < 404.;
    let draft = &s.new_threads.drafts[project];
    let locked = s.pending || draft.uncertain;
    let target = super::attachments::Target::Project(project.to_owned());
    let field = text_input(&draft.prompt, TextStyle::new(14.0, 400.0).lh(21.0))
        .key("new-thread-prompt")
        .label("Message")
        .w_full()
        .color(theme.text)
        .placeholder("Ask anything...", theme.text_secondary)
        .disabled(locked)
        .on_edit(|s: &mut Desktop| &mut s.new_threads.current().prompt)
        .on_submit(|s: &mut Desktop, _| s.launch());
    let field = target.input(field);
    let options = draft
        .choices
        .iter()
        .map(|c| {
            let mut item = ComboboxOption::new(&c.id, &c.name);
            item.detail = c.provider.clone();
            item
        })
        .collect::<Vec<_>>();
    let model = Combobox::new(
        "launch-model",
        &draft.picker,
        &options,
        |s: &mut Desktop| &mut s.new_threads.current().picker,
    )
    .disabled(locked)
    .label("Model")
    .current(draft.model.as_deref())
    .trigger(crate::models::trigger(
        draft
            .displayed_choice()
            .map(|c| c.name.as_str())
            .unwrap_or("Choose a model"),
        theme,
        !locked,
        width,
    ))
    .placeholder("Choose a model")
    .search_placeholder("Search models")
    .on_choose(|s, id, _| {
        if !s.pending {
            s.new_threads.current().select_model(id);
        }
    })
    .build(kit, overlays);
    let mut controls = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(target.button(theme, locked));
    let mut footer = s.chat_width.lane(row()).center_y().gap(6.);
    if !scratch {
        let checkout = if draft.worktree {
            "New worktree"
        } else {
            "Current checkout"
        };
        footer = footer.child(
            permissions::context_button(checkout, &rune_kit::glyphs::FOLDER, theme, compact)
                .label(checkout)
                .disabled(locked)
                .focusable(!locked)
                .on_press(|s: &mut Desktop| {
                    let draft = s.new_threads.current();
                    draft.worktree = !draft.worktree;
                }),
        );
    }
    footer = footer.child(spacer());
    if !scratch && draft.worktree {
        footer = footer.child(
            text_input(&draft.base, TextStyle::new(12.0, 400.0))
                .key("launch-base")
                .label("Base branch")
                .single_line()
                .w(100.)
                .h(26.)
                .color(theme.text)
                .placeholder("Base branch", theme.text_secondary)
                .disabled(locked)
                .on_edit(|s: &mut Desktop| &mut s.new_threads.current().base),
        );
    } else if !scratch && !draft.checkout_branch.is_empty() && draft.checkout_branch != "HEAD" {
        footer = footer.child(permissions::branch_label(
            &draft.checkout_branch,
            theme,
            compact,
        ));
    }
    if !draft.permission_options().is_empty() {
        footer = footer.child(permissions::control(
            s,
            draft
                .permission()
                .map(|p| (p.mode.as_str(), p.label.as_str())),
            theme,
            kit,
            overlays,
            locked,
            compact,
        ));
    }
    controls = controls.child(spacer());
    controls = controls.child(model).child(s.send_button(
        cx,
        theme,
        if draft.uncertain {
            "Retry launch"
        } else {
            "Start thread"
        },
        !s.pending && draft.ready(),
        Desktop::launch,
    ));
    let heading_font = if viewport_width >= 640. {
        TextStyle::new(30., 400.).tracking(-0.75).lh(36.)
    } else {
        TextStyle::new(24., 400.).tracking(-0.6).lh(32.)
    };
    let project_font = TextStyle {
        weight: 500.,
        ..heading_font
    };
    let prefix = "What should we build in";
    let inline = !scratch
        && viewport_width >= 1024.
        && rune::text::advance(prefix, heading_font)
            + 8.
            + rune::text::advance(title, project_font)
            + rune::text::advance("?", heading_font)
            <= heading_width;
    let show_question = scratch || viewport_height > 448.;
    let project_options = s.workspace.launch_projects();
    let project_picker = Combobox::new(
        "launch-project",
        &s.new_threads.project_picker,
        &project_options,
        |s: &mut Desktop| &mut s.new_threads.project_picker,
    )
    .disabled(s.pending)
    .current(Some(project))
    .label("Project")
    .search_placeholder("Search projects")
    .trigger(
        row()
            .label("Project")
            .help(if scratch { "No project" } else { title })
            .role("button")
            .focusable(true)
            .shrink()
            .child(
                text(if scratch { "No project" } else { title })
                    .truncate()
                    .max_w(heading_width)
                    .font(if inline {
                        project_font
                    } else if scratch {
                        TextStyle::new(14., 400.).lh(24.)
                    } else {
                        TextStyle::new(16., 500.).lh(24.)
                    })
                    .color(if scratch {
                        theme.text_secondary
                    } else {
                        theme.text
                    }),
            ),
    )
    .on_choose(|s, id, _| s.prepare_thread(id))
    .build(kit, overlays);
    let mut headline = if inline { row() } else { col() };
    headline = headline.w(heading_width).center().gap(8.);
    if show_question {
        let question = if scratch {
            "What should we work on?"
        } else if inline {
            prefix
        } else {
            "What should we build?"
        };
        headline = headline.child(
            text(question)
                .max_w(heading_width)
                .wrap()
                .font(heading_font)
                .color(theme.text),
        );
    }
    let mut selector = row().center_y().min_w(0.).child(project_picker);
    if inline && show_question {
        selector = selector.child(text("?").font(heading_font).color(theme.text));
    }
    let mut heading = col().items_center().gap(8.).child(headline.child(selector));
    if !scratch && s.workspace.scratch_available {
        heading = heading.child(super::workspace::action(
            "or start without a project",
            theme,
            !s.pending,
            Desktop::prepare_scratch,
        ));
    }
    let list = target.list(&draft.files, theme, locked, s, draft_preview);
    let surface = composer::surface(
        s.composer_card(theme, dark).key("composer-new-thread"),
        field,
        controls,
        list,
        Some(footer),
        lane,
    )
    .pt(0.)
    .pb(28.);
    let top = col()
        .w_full()
        .grow(1.)
        .justify_end()
        .items_center()
        .child(lane.pad(row().w_full().justify_center().child(heading.pb(16.))));
    let mut content = col()
        .key("new-thread-body")
        .w_full()
        .grow(1.)
        .min_h(0.)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .items_center()
        .child(top)
        .child(target.drop_area(surface, locked));
    if draft.uncertain {
        content = content.child(
            text("Delivery is unconfirmed. Retry to recover this thread.")
                .color(theme.text_secondary),
        );
    }
    if draft.choices.is_empty() {
        content = content
            .child(text("No available models for this project.").color(theme.text_secondary));
    }
    if viewport_height > 544. {
        content = content.child(spacer());
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returning_to_a_project_preserves_its_draft_and_pending_launch_identity() {
        let choice = |id: &str| Choice {
            id: id.into(),
            name: id.into(),
            provider: "Replay".into(),
            instance: "replay".into(),
            runtime: "approval-required".into(),
            preferred: true,
        };
        let modes = || {
            serde_json::from_str(r#"{"replay":[{"mode":"approval-required","label":"Supervised","description":""}]}"#).unwrap()
        };
        let mut drafts = NewThreads::default();
        drafts.configure("a".into(), vec![choice("one")], modes(), false, "main");
        drafts
            .editable_files()
            .unwrap()
            .push(Attachment::local("photo.png"));
        assert!(drafts.current().ready());
        drafts.current().runtime_override = Some("approval-required".into());
        let original = drafts.current().operation.clone();
        let mut app = Desktop::new();
        app.workspace.feed = Some(5);
        app.can_operate = true;
        app.new_threads = drafts;
        let update = |app: &mut Desktop, watch: u64, name: &str| {
            let message = serde_json::json!({
                "type": "draftmodels", "watch": watch, "project": "a", "id": 7,
                "choices": [{"id":name,"name":name,"provider":"Replay","instance":"replay",
                    "runtime":"approval-required","preferred":true}],
                "permissions": {"replay":[{"mode":"approval-required","label":"Supervised","description":""}]},
            });
            app.receive(serde_json::from_value(message).unwrap());
        };
        app.new_threads.catalog_watch = Some((5, "a".into(), 7));
        app.new_threads
            .current()
            .picker
            .step(rune_kit::ComboboxEvent::Toggle {
                current: Some("one"),
            });
        update(&mut app, 5, "live");
        assert_eq!(app.new_threads.current().model.as_deref(), Some("live"));
        assert!(app.new_threads.current().picker.is_open());
        app.new_threads.current().select_model("live");
        update(&mut app, 5, "other");
        assert_eq!(app.new_threads.current().model.as_deref(), Some("live"));
        assert!(!app.new_threads.current().ready());
        assert_eq!(app.new_threads.provider_instance(), Some("replay"));
        update(&mut app, 5, "live");
        assert!(app.new_threads.current().ready());
        app.new_threads.current().picked = None;
        update(&mut app, 5, "one");
        update(&mut app, 4, "stale");
        assert_eq!(app.new_threads.current().model.as_deref(), Some("one"));
        app.pending = true;
        update(&mut app, 5, "during-launch");
        assert_eq!(app.new_threads.current().model.as_deref(), Some("one"));
        let mut drafts = app.new_threads;
        drafts.mark_unconfirmed("a", true);
        drafts.configure("b".into(), vec![choice("two")], modes(), true, "main");
        assert!(drafts.current().prompt.text.is_empty());
        drafts.configure("a".into(), vec![choice("one")], modes(), false, "main");
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().files[0].path, "photo.png");
        assert!(drafts.editable_files().is_none());
        drafts.configure(
            "a".into(),
            vec![choice("replacement")],
            modes(),
            false,
            "main",
        );
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().model.as_deref(), Some("one"));
        drafts.mark_unconfirmed("a", false);
        drafts.configure(
            "a".into(),
            vec![choice("replacement")],
            modes(),
            false,
            "main",
        );
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().model.as_deref(), Some("replacement"));
        assert_eq!(
            drafts.current().permission().unwrap().mode,
            "approval-required"
        );
        let mut changed = choice("replacement");
        changed.runtime = "full-access".into();
        let permissions = serde_json::from_str(
            r#"{"replay":[{"mode":"full-access","label":"Full access","description":""}]}"#,
        )
        .unwrap();
        drafts.configure("a".into(), vec![changed], permissions, false, "main");
        assert!(!drafts.current().ready());
        drafts.retain(|id| id == "b");
        assert!(drafts.active.is_none());
    }
}
