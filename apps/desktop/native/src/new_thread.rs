use super::{Desktop, Request, composer};
use rune::{El, TextEdit, TextStyle, col, row, spacer, text, text_input, tok};
use rune_kit::{Combobox, ComboboxOption, ComboboxState, Kit};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Choice {
    id: String,
    name: String,
    provider: String,
    runtime: String,
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
    worktree: bool,
    model: Option<String>,
    choices: Vec<Choice>,
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
    pub fn configure(&mut self, project: String, choices: Vec<Choice>, worktree: bool, base: &str) {
        let draft = self
            .drafts
            .entry(project.clone())
            .or_insert_with(|| LaunchDraft {
                prompt: TextEdit::new(""),
                files: vec![],
                base: TextEdit::new(base),
                worktree,
                model: None,
                choices: Vec::new(),
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
    fn current(&mut self) -> &mut LaunchDraft {
        self.drafts
            .get_mut(self.active.as_deref().expect("open thread draft"))
            .expect("known project")
    }
}

impl LaunchDraft {
    fn ready(&self) -> bool {
        self.model.is_some()
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
        let Some(choice) = draft
            .choices
            .iter()
            .find(|c| Some(&c.id) == draft.model.as_ref())
        else {
            return;
        };
        let request = Request::LaunchThread {
            runtime: choice.runtime.clone(),
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
    kit: &Kit,
    overlays: &mut Vec<El>,
) -> El {
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
    if !scratch {
        controls = controls.child(super::workspace::action(
            if draft.worktree {
                "New worktree"
            } else {
                "Local checkout"
            },
            theme,
            !locked,
            |s| {
                let draft = s.new_threads.current();
                draft.worktree = !draft.worktree;
            },
        ));
    }
    if !scratch && draft.worktree {
        controls = controls.child(
            text_input(&draft.base, TextStyle::new(12.0, 400.0))
                .key("launch-base")
                .label("Base branch")
                .single_line()
                .w(110.0)
                .h(26.0)
                .color(theme.text)
                .placeholder("Base branch", theme.text_secondary)
                .disabled(locked)
                .on_edit(|s: &mut Desktop| &mut s.new_threads.current().base),
        );
    }
    controls = controls.child(spacer());
    if let Some(choice) = draft
        .choices
        .iter()
        .find(|c| Some(&c.id) == draft.model.as_ref())
    {
        let label = match choice.runtime.as_str() {
            "approval-required" => "Approval required",
            "auto-accept-edits" => "Auto-accept edits",
            "full-access" => "Full access",
            _ => "Auto",
        };
        controls = controls.child(
            text(label)
                .font(TextStyle::new(12.0, 400.0))
                .color(theme.text_secondary),
        );
    }
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
                .font(TextStyle::new(
                    if scratch { 14.0 } else { 30.0 },
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
                    .font(TextStyle::new(30.0, 400.0))
                    .color(theme.text),
            )
            .child(project_picker)
    } else {
        let title = row()
            .max_w(736.0)
            .center_y()
            .gap(6.0)
            .child(
                text("What should we build in")
                    .font(TextStyle::new(30.0, 400.0))
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
    let surface = composer::surface("new-thread", field, controls, theme, list);
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
            runtime: "approval-required".into(),
            preferred: true,
        };
        let mut drafts = NewThreads::default();
        drafts.configure("a".into(), vec![choice("one")], false, "main");
        drafts.editable_files().unwrap().push("photo.png".into());
        assert!(drafts.current().ready());
        let original = drafts.current().operation.clone();
        drafts.mark_unconfirmed("a", true);
        drafts.configure("b".into(), vec![choice("two")], true, "main");
        assert!(drafts.current().prompt.text.is_empty());
        drafts.configure("a".into(), vec![choice("one")], false, "main");
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().files, ["photo.png"]);
        assert!(drafts.editable_files().is_none());
        drafts.configure("a".into(), vec![choice("replacement")], false, "main");
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().model.as_deref(), Some("one"));
        drafts.mark_unconfirmed("a", false);
        drafts.configure("a".into(), vec![choice("replacement")], false, "main");
        assert_eq!(drafts.current().operation, original);
        assert_eq!(drafts.current().model.as_deref(), Some("replacement"));
        drafts.retain(|id| id == "b");
        assert!(drafts.active.is_none());
    }
}
