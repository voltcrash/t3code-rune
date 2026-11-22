use super::{Desktop, new_thread::NewThreads, workspace::Workspace};
use rune::{El, Icon, TextStyle, el, icon, row, text, tok};

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) enum Page {
    #[default]
    Home,
    Thread(String),
    NewThread(String),
}

#[derive(Default)]
pub(super) enum Landing {
    #[default]
    Index,
    Loading {
        project: String,
        request: u64,
    },
    Failed(String),
    Left,
}

impl Landing {
    pub(super) fn matches(&self, id: Option<u64>) -> bool {
        matches!(self, Self::Loading {request, ..} if Some(*request) == id)
    }
    pub(super) fn fail(&mut self) {
        if let Self::Loading { project, .. } = self {
            *self = Self::Failed(std::mem::take(project));
        }
    }
    pub(super) fn project(&self) -> Option<&str> {
        match self {
            Self::Loading { project, .. } | Self::Failed(project) => Some(project),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct History {
    current: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
}

impl History {
    pub(super) fn replace(&mut self, page: Page) {
        self.current = page;
    }

    pub fn visit(&mut self, page: Page) {
        if self.current == page {
            return;
        }
        self.back.push(std::mem::replace(&mut self.current, page));
        self.forward.clear();
    }

    pub(super) fn on_thread(&self, id: &str) -> bool {
        matches!(&self.current, Page::Thread(current) if current == id)
    }

    pub(super) fn close_thread(&mut self, id: &str) {
        if self.on_thread(id) {
            self.current = Page::Home;
        }
    }

    pub(super) fn available(&self, forward: bool) -> bool {
        if forward {
            !self.forward.is_empty()
        } else {
            !self.back.is_empty()
        }
    }
    fn pop(&mut self, forward: bool) -> Option<Page> {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        let page = from.pop()?;
        to.push(std::mem::replace(&mut self.current, page.clone()));
        Some(page)
    }

    pub fn retain(&mut self, ws: &Workspace, drafts: &NewThreads) {
        let available = |page: &Page| match page {
            Page::Home => true,
            Page::Thread(id) => ws.has_thread(id),
            Page::NewThread(id) => drafts.has_draft(id),
        };
        if !available(&self.current) {
            self.current = if let Some(id) = &drafts.active {
                Page::NewThread(id.clone())
            } else if !ws.show_projects
                && let Some(pane) = &ws.conversation
            {
                Page::Thread(pane.id.clone())
            } else {
                Page::Home
            };
        }
        for stack in [&mut self.back, &mut self.forward] {
            stack.retain(&available);
            stack.dedup();
            if stack.last() == Some(&self.current) {
                stack.pop();
            }
        }
    }

    pub fn launched(&mut self, project: &str, id: &str) {
        if self.current == Page::NewThread(project.into()) {
            self.current = Page::Thread(id.into());
            self.forward.clear();
        }
    }
}

impl Desktop {
    pub(super) fn go(&mut self, forward: bool) {
        if self.pending || self.environment.is_none() || self.palette.is_some() {
            return;
        }
        self.history.retain(&self.workspace, &self.new_threads);
        match self.history.pop(forward) {
            Some(Page::Home) => self.show_projects(),
            Some(Page::Thread(id)) => self.open_thread(&id),
            Some(Page::NewThread(project)) => {
                self.leave_visible_thread();
                self.close_conversation_inspectors();
                self.workspace.show_projects = false;
                self.new_threads.reopen(project);
            }
            None => {}
        }
    }
}

pub(super) const BACK: Icon = Icon {
    name: "history-back",
    view: 24.,
    body: r#"<path d="m12 5-7 7 7 7M5 12h15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub(super) const FORWARD: Icon = Icon {
    name: "history-forward",
    view: 24.,
    body: r#"<path d="m12 5 7 7-7 7m7-7H4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

fn history_button(s: &Desktop, forward: bool, theme: &tok::Theme) -> El {
    let (caption, glyph, available) = if forward {
        ("Go forward", &FORWARD, s.history.available(true))
    } else {
        ("Go back", &BACK, s.history.available(false))
    };
    let button = el()
        .key(caption)
        .role("button")
        .label(caption)
        .help(caption)
        .size(26., 28.)
        .center()
        .radius(5.)
        .no_window_drag()
        .child(icon(glyph, 14.).color(theme.text_secondary));
    if available && !s.pending {
        button
            .focusable(true)
            .hover(theme.separator)
            .on_press(move |s: &mut Desktop| s.go(forward))
    } else {
        button.disabled(true).opacity(0.45)
    }
}

pub(super) fn header(s: &Desktop, theme: &tok::Theme, compact: bool) -> El {
    let home = el()
        .key("t3-home")
        .role("button")
        .label("T3 Code home")
        .help("T3 Code home")
        .h(28.)
        .px(2.)
        .center()
        .radius(5.)
        .no_window_drag()
        .child(
            row()
                .center_y()
                .gap(4.)
                .child(icon(&WORDMARK, 17.).color(theme.text))
                .child(
                    text("Code")
                        .font(TextStyle::new(14., 500.).tracking(-0.35))
                        .color(theme.text_secondary),
                ),
        );
    let brand = row().center_y().gap(2.).min_w(0.).child(if s.pending {
        home.disabled(true)
    } else {
        home.focusable(true)
            .hover(theme.separator)
            .on_press(Desktop::show_projects)
    });
    if compact {
        brand
    } else {
        brand
            .child(history_button(s, false, theme))
            .child(history_button(s, true, theme))
    }
}

const WORDMARK: Icon = Icon {
    name: "t3-wordmark",
    view: 94.3941,
    body: r#"<path transform="translate(-15.5309 -18.28295)" d="M33.4509 93V47.56H15.5309V37H64.3309V47.56H46.4109V93H33.4509ZM86.7253 93.96C82.832 93.96 78.9653 93.4533 75.1253 92.44C71.2853 91.3733 68.032 89.88 65.3653 87.96L70.4053 78.04C72.5386 79.5867 75.0186 80.8133 77.8453 81.72C80.672 82.6267 83.5253 83.08 86.4053 83.08C89.6586 83.08 92.2186 82.44 94.0853 81.16C95.952 79.88 96.8853 78.12 96.8853 75.88C96.8853 73.7467 96.0586 72.0667 94.4053 70.84C92.752 69.6133 90.0853 69 86.4053 69H80.4853V60.44L96.0853 42.76L97.5253 47.4H68.1653V37H107.365V45.4L91.8453 63.08L85.2853 59.32H89.0453C95.9253 59.32 101.125 60.8667 104.645 63.96C108.165 67.0533 109.925 71.0267 109.925 75.88C109.925 79.0267 109.099 81.9867 107.445 84.76C105.792 87.48 103.259 89.6933 99.8453 91.4C96.432 93.1067 92.0586 93.96 86.7253 93.96Z" fill="currentColor"/>"#,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_to_a_removed_thread_recovers_home_and_keeps_the_forward_destination() {
        let mut app = Desktop::new();
        app.environment = Some("Test".into());
        let thread = |id: &str| {
            serde_json::from_value(serde_json::json!({
                "id": id, "project": "project", "title": id, "status": "idle",
                "model": "", "settled": false
            }))
            .unwrap()
        };
        app.replace_workspace(vec![], vec![thread("a"), thread("b")]);
        app.open_thread("a");
        app.open_thread("b");
        app.go(false);
        app.pending = true;
        app.receive(
            serde_json::from_value(serde_json::json!({
                "id": app.request_id, "type": "threadunavailable", "thread": "a"
            }))
            .unwrap(),
        );
        assert!(app.workspace.has_thread("a"));
        app.workspace.feed = Some(9);
        app.receive(
            serde_json::from_value(serde_json::json!({
                "type": "workspacelive", "watch": 9, "scratch_available": false,
                "threads": [], "projects": [], "removed_threads": ["a"], "removed_projects": []
            }))
            .unwrap(),
        );
        assert_eq!(app.history.current, Page::Home);
        assert!(!app.workspace.has_thread("a"));
        assert!(app.workspace.conversation.is_none());
        assert!(!app.history.available(false));
        app.go(true);
        assert_eq!(app.history.current, Page::Thread("b".into()));
    }

    #[test]
    fn initial_draft_replaces_home_and_late_options_respect_explicit_navigation() {
        let record = serde_json::json!({
            "id": "p", "title": "Project", "path": "/p", "archived": false, "scratch": false
        });
        let open = |operate| {
            let mut app = Desktop::new();
            app.can_operate = operate;
            app.pending = true;
            app.receive(
                serde_json::from_value(serde_json::json!({
                    "type": "workspace", "watch": 0, "id": 0, "landing_project": "p",
                    "scratch_available": false, "projects": [record], "threads": []
                }))
                .unwrap(),
            );
            assert!(!app.pending);
            app
        };
        let finish = |app: &mut Desktop| {
            app.receive(
                serde_json::from_value(serde_json::json!({
                    "type": "launchoptions", "id": 1, "record": record, "project": "p",
                    "choices": [], "permissions": {}, "base": "main", "worktree": false
                }))
                .unwrap(),
            )
        };
        let mut app = open(true);
        app.environment = Some("Original workspace".into());
        app.request_id = app.next_request_id();
        app.pending = true;
        app.pairing = true;
        finish(&mut app);
        assert!(app.pending);
        app.receive(super::super::WorkerMessage {
            id: Some(app.request_id),
            reply: super::super::Reply::Error {
                message: "Pairing link expired".into(),
            },
        });
        assert!(!app.pending && !app.pairing);
        assert_eq!(app.environment.as_deref(), Some("Original workspace"));
        assert_eq!(app.history.current, Page::NewThread("p".into()));
        assert!(!app.history.available(false));
        app.environment = Some("Test".into());
        app.show_projects();
        app.workspace.refresh_request = Some(2);
        app.go(false);
        assert_eq!(app.new_threads.active.as_deref(), Some("p"));
        app.go(true);
        app.request_id = 3;
        app.pending = true;
        let refreshed = serde_json::json!({
            "type": "workspace", "watch": 2, "id": 2, "landing_project": "p",
            "scratch_available": false, "projects": [record], "threads": []
        });
        app.receive(serde_json::from_value(refreshed.clone()).unwrap());
        assert_eq!(app.workspace.feed, Some(2));
        assert!(app.workspace.refresh_request.is_none());
        assert!(app.pending);
        assert_eq!(app.history.current, Page::Home);
        app.receive(serde_json::from_value(refreshed).unwrap());
        assert!(app.pending);

        let mut browsing = open(true);
        browsing.show_projects();
        finish(&mut browsing);
        assert_eq!(browsing.history.current, Page::Home);
        assert!(browsing.new_threads.active.is_none());
        let mut empty = Desktop::new();
        empty.can_operate = true;
        empty.workspace.feed = Some(9);
        let first_project = || {
            serde_json::from_value(serde_json::json!({
                "type": "workspacelive", "watch": 9, "landing_project": "p",
                "scratch_available": false, "projects": [record], "threads": [],
                "removed_threads": [], "removed_projects": []
            }))
            .unwrap()
        };
        empty.receive(first_project());
        assert!(!empty.pending);
        assert!(empty.landing.matches(Some(1)));
        empty.leave_landing();
        let folder_request = empty.request_id;
        empty.receive(first_project());
        assert!(!empty.pending);
        assert_eq!(empty.request_id, folder_request);
        finish(&mut empty);
        assert!(empty.new_threads.active.is_none());
        let mut viewer = open(false);
        finish(&mut viewer);
        assert!(viewer.new_threads.active.is_none());
    }

    #[test]
    fn launching_replaces_the_draft_and_a_new_visit_discards_forward_history() {
        let mut history = History::default();
        let page = |id: &str| Page::Thread(id.into());
        history.visit(page("a"));
        history.visit(Page::NewThread("project".into()));
        history.launched("project", "b");
        history.visit(page("b"));
        assert_eq!(history.pop(false), Some(page("a")));
        assert_eq!(history.pop(true), Some(page("b")));
        assert_eq!(history.pop(false), Some(page("a")));
        history.visit(page("c"));
        assert_eq!(history.pop(true), None);
        assert_eq!(history.pop(false), Some(page("a")));
        assert_eq!(history.pop(false), Some(Page::Home));
        assert_eq!(history.pop(false), None);
    }
}
