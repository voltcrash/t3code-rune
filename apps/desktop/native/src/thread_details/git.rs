use super::{Desktop, Request, Settings, label};
use rune::{El, Icon, Tx, col, el, icon, row, tok};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Selection {
    draft: bool,
    target: String,
    cwd: String,
}

type Key<'a> = (u64, bool, &'a str, &'a str);

#[derive(Default)]
pub(super) struct State {
    target: Option<(u64, Selection)>,
    subscription: u64,
    non_repo: bool,
    pending: Option<u64>,
}

impl State {
    fn key(&self) -> Option<Key<'_>> {
        self.target.as_ref().map(|(watch, selection)| {
            (
                *watch,
                selection.draft,
                selection.target.as_str(),
                selection.cwd.as_str(),
            )
        })
    }
}

impl Desktop {
    fn git_target(&self) -> Option<Key<'_>> {
        let details = &self.workspace.details;
        let settings = details.settings.as_ref().filter(|s| s.git_actions)?;
        let (draft, id) = self.details_target()?;
        if settings.environment || settings.open_in {
            let visible = if details.inline.get() {
                !settings.hidden.contains(draft, id)
            } else {
                details
                    .popup
                    .as_ref()
                    .is_some_and(|(d, t)| *d == draft && t == id)
            };
            if !visible {
                return None;
            }
        }
        let (project, cwd) = if draft {
            let p = self.workspace.project(id)?;
            (p, p.path.as_str())
        } else {
            let t = self.workspace.visible_thread()?;
            (
                self.workspace.project(&t.project)?,
                self.thread_workspace_root()?,
            )
        };
        (!project.scratch).then_some((self.workspace.feed?, draft, id, cwd))
    }

    pub(crate) fn sync_git(&mut self) -> bool {
        let next = self.git_target();
        if next == self.workspace.details.git.key() {
            return false;
        }
        let previous = self.workspace.details.git.target.as_ref().map(|(w, _)| *w);
        let target = next.map(|(w, d, id, cwd)| {
            (
                w,
                Selection {
                    draft: d,
                    target: id.into(),
                    cwd: cwd.into(),
                },
            )
        });
        let mut git = State::default();
        if let Some((watch, selection)) = target {
            let id = self.next_request_id();
            if self.write_request(
                Some(id),
                Request::WatchGit {
                    watch,
                    selection: Some(&selection),
                },
            ) {
                git.target = Some((watch, selection));
                git.subscription = id;
            }
        } else if let Some(watch) = previous {
            self.write_request(
                None,
                Request::WatchGit {
                    watch,
                    selection: None,
                },
            );
        }
        self.workspace.details.git = git;
        true
    }

    pub(crate) fn git_status(&mut self, watch: u64, id: Option<u64>, non_repo: bool) {
        let git = &mut self.workspace.details.git;
        if self.workspace.feed == Some(watch) && id == Some(git.subscription) {
            git.non_repo = non_repo;
        }
    }

    pub(super) fn has_git_row(&self) -> bool {
        let git = &self.workspace.details.git;
        git.non_repo && git.target.is_some() && git.key() == self.git_target()
    }

    fn init_git(&mut self, tx: &mut Tx) {
        let Some(watch) = self.workspace.feed else {
            return;
        };
        let details = &self.workspace.details;
        let Some(settings) = details.settings.as_ref().filter(|s| s.git_write) else {
            return;
        };
        if !self.has_git_row() || details.git.pending.is_some() {
            return;
        }
        let focus = if settings.environment || settings.open_in {
            "toggle-thread-details"
        } else if self.new_threads.active.is_some() {
            "new-thread-prompt"
        } else {
            "message-input"
        };
        let subscription = details.git.subscription;
        let id = self.next_request_id();
        if self.write_request(
            Some(id),
            Request::InitGit {
                watch,
                subscription,
            },
        ) {
            self.workspace.details.git.pending = Some(id);
            tx.focus_when_shown(focus, true);
        }
    }

    pub(crate) fn git_initialized(&mut self, watch: u64, id: Option<u64>, error: Option<String>) {
        if self.workspace.details.git.key() != self.git_target() {
            return;
        }
        let git = &mut self.workspace.details.git;
        if self.workspace.feed == Some(watch) && git.pending.is_some() && git.pending == id {
            git.pending = None;
            if let Some(error) = error {
                self.error = Some(error);
            } else {
                git.non_repo = false;
            }
        }
    }
}

const BRANCH_PLUS: Icon = Icon {
    name: "git-branch-plus",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3v12M18 9a9 9 0 0 1-9 9M16 21v-6m-3 3h6"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/></g>"#,
};

pub(super) fn section(s: &Desktop, settings: &Settings, theme: &tok::Theme, separated: bool) -> El {
    let pending = s.workspace.details.git.pending.is_some();
    let name = if pending {
        &settings.labels.initializing
    } else {
        &settings.labels.initialize_git
    };
    let enabled = settings.git_write && !pending;
    let button = row()
        .key("initialize-git")
        .role("button")
        .label(name)
        .help(name)
        .w_full()
        .h(32.)
        .px(10.)
        .gap(10.)
        .center_y()
        .radius(8.)
        .hover(theme.hover)
        .focusable(enabled)
        .disabled(!enabled)
        .child(icon(&BRANCH_PLUS, 14.).color(theme.text_secondary))
        .child(label(name, 13., 500., theme.text.alpha(0.8)).truncate())
        .on_press_tx(Desktop::init_git);
    let mut section = col().w_full();
    if separated {
        section = section.child(el().w_full().h(1.).fill(theme.separator));
    }
    section.child(col().w_full().padding(9.).child(button))
}
