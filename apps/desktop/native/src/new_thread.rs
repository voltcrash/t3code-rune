use super::{Desktop, Request, composer};
use rune::{El, TextEdit, TextStyle, col, row, spacer, text, text_input, tok};
use rune_kit::{Combobox, ComboboxOption, ComboboxState, Kit};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::permissions;
pub(super) type ProviderPermissions = HashMap<String, Vec<permissions::OptionRow>>;

#[derive(Deserialize)]
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
}

struct LaunchDraft {
    prompt: TextEdit,
    files: Vec<String>,
    base: TextEdit,
    checkout_branch: String,
    worktree: bool,
    model: Option<String>,
    choices: Vec<Choice>,
    permissions: ProviderPermissions,
    runtime_override: Option<String>,
    picker: ComboboxState,
    operation: String,
    uncertain: bool,
}

impl NewThreads {
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
        if !choices.iter().any(|c| Some(&c.id) == draft.model.as_ref()) {
            draft.model = choices
                .iter()
                .find(|c| c.preferred)
                .or_else(|| choices.first())
                .map(|c| c.id.clone());
        }
        draft.choices = choices;
        draft.permissions = permissions;
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
    pub(super) fn editable_files(&mut self) -> Option<&mut Vec<String>> {
        let draft = self.current();
        (!draft.uncertain).then_some(&mut draft.files)
    }
    pub(super) fn picker_options(&mut self) -> Option<(Vec<permissions::OptionRow>, String)> {
        let draft = self.current();
        if draft.uncertain {
            return None;
        }
        Some((
            draft.permission_options().to_vec(),
            draft
                .permission()
                .map_or_else(String::new, |p| p.mode.clone()),
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
    fn choice(&self) -> Option<&Choice> {
        self.choices
            .iter()
            .find(|choice| Some(&choice.id) == self.model.as_ref())
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
    project: &str,
    title: &str,
    scratch: bool,
    theme: &tok::Theme,
    cx: &rune::Cx<Desktop>,
    overlays: &mut Vec<El>,
) -> El {
    let kit = &Kit::neutral(s.dark(cx));
    let width = cx
        .layout_size("thread-titlebar")
        .map_or(840., |size| size[0]);
    let heading_width = (width - 32.).clamp(100., 736.);
    let compact = width < 404.;
    let draft = &s.new_threads.drafts[project];
    let locked = s.pending || draft.uncertain;
    let field = text_input(&draft.prompt, TextStyle::new(14.0, 400.0).lh(21.0))
        .key("new-thread-prompt")
        .label("Message")
        .w_full()
        .color(theme.text)
        .placeholder("Ask anything...", theme.text_secondary)
        .disabled(locked)
        .on_edit(|s: &mut Desktop| &mut s.new_threads.current().prompt)
        .on_submit(|s: &mut Desktop, _| s.launch());
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
        draft.choice().map_or("Choose a model", |c| c.name.as_str()),
        theme,
        !locked,
        width,
    ))
    .placeholder("Choose a model")
    .search_placeholder("Search models")
    .on_choose(|s, id, _| {
        if !s.pending {
            let draft = s.new_threads.current();
            draft.model = Some(id.to_owned());
        }
    })
    .build(kit, overlays);
    let target = super::attachments::Target::Project(project.to_owned());
    let mut controls = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(target.button(theme, locked));
    let mut footer = row().w_full().max_w(736.).self_center().center_y().gap(6.);
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
    if draft.choice().is_some() {
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
    controls = controls.child(model).child(composer::send_button(
        if draft.uncertain {
            "Retry launch"
        } else {
            "Start thread"
        },
        !s.pending && draft.ready(),
        Desktop::launch,
    ));
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
            .role("button")
            .focusable(true)
            .shrink()
            .child(
                text(if scratch {
                    "No project".into()
                } else {
                    format!("{title}?")
                })
                .truncate()
                .max_w(heading_width)
                .font(TextStyle::new(
                    if scratch {
                        14.0
                    } else if compact {
                        24.
                    } else {
                        30.0
                    },
                    if scratch { 400.0 } else { 500.0 },
                ))
                .color(if scratch {
                    theme.text_secondary
                } else {
                    theme.text
                }),
            ),
    )
    .on_choose(|s, id, _| s.prepare_thread(id))
    .build(kit, overlays);
    let heading = if scratch {
        col()
            .center()
            .gap(8.0)
            .child(
                text("What should we work on?")
                    .max_w(heading_width)
                    .wrap()
                    .font(TextStyle::new(if compact { 24. } else { 30. }, 400.0))
                    .color(theme.text),
            )
            .child(project_picker)
    } else {
        let title = row()
            .w(heading_width)
            .flex_wrap()
            .center()
            .gap(6.0)
            .child(
                text("What should we build in")
                    .max_w(heading_width)
                    .wrap()
                    .font(TextStyle::new(if compact { 24. } else { 30. }, 400.0))
                    .color(theme.text)
                    .shrink(),
            )
            .child(project_picker);
        let mut heading = col().center().gap(8.0).child(title);
        if s.workspace.scratch_available {
            heading = heading.child(super::workspace::action(
                "or start without a project",
                theme,
                !s.pending,
                Desktop::prepare_scratch,
            ));
        }
        heading
    };
    let list = target.list(&draft.files, theme, locked);
    let surface = composer::surface("new-thread", field, controls, theme, list, Some(footer));
    let mut content = col()
        .w_full()
        .grow(1.0)
        .center()
        .gap(12.0)
        .child(heading)
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
        drafts.editable_files().unwrap().push("photo.png".into());
        assert!(drafts.current().ready());
        drafts.current().runtime_override = Some("approval-required".into());
        let original = drafts.current().operation.clone();
        drafts.mark_unconfirmed("a", true);
        drafts.configure("b".into(), vec![choice("two")], modes(), true, "main");
        assert!(drafts.current().prompt.text.is_empty());
        drafts.configure("a".into(), vec![choice("one")], modes(), false, "main");
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().files, ["photo.png"]);
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
