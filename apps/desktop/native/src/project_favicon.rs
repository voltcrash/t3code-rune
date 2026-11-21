use super::{Desktop, Project, Request};
use rune::{El, Image, SurfaceFit, el};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

pub(super) type Cache = VecDeque<Arc<Image>>;

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Loading(String),
    Missing,
    Loaded(Weak<Image>),
}

#[derive(Deserialize)]
pub(super) struct Target {
    pub(super) path: Option<String>,
    #[serde(skip, default = "operation")]
    generation: String,
    #[serde(skip)]
    state: State,
}

fn operation() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Project {
    pub(super) fn badge(&self, watch: Option<u64>, dark: bool) -> Option<El> {
        let fallback = self.icon.as_ref()?;
        let (Some(target), Some(watch)) = (&self.favicon, watch) else {
            return Some(fallback.view(dark));
        };
        if self.archived {
            return Some(fallback.view(dark));
        }
        let image = match &target.state {
            State::Loaded(weak) => weak.upgrade(),
            _ => None,
        };
        let tile = match &image {
            Some(image) => el()
                .size(16., 16.)
                .clip_rounded(4.)
                .a11y_hidden()
                .pixels_fit(image.pixels(), SurfaceFit::Fit),
            None => fallback.view(dark),
        };
        let project = self.id.clone();
        let generation = target.generation.clone();
        Some(
            tile.key(format!("project-favicon-{watch}-{generation}"))
                .on_visible(move |s: &mut Desktop, visible, _| {
                    // The mounted badge holds its image even after it leaves the offscreen cache.
                    if visible && image.is_none() {
                        s.load_project_favicon(watch, &project, &generation);
                    }
                }),
        )
    }
}

impl super::Workspace {
    pub(crate) fn refresh_favicons(&mut self, projects: Option<Vec<String>>) {
        fn invalidate(row: &mut Project) {
            if let Some(target) = &mut row.favicon {
                target.generation = operation();
                target.state = State::Idle;
            }
        }
        if let Some(ids) = projects {
            for index in ids.iter().filter_map(|id| self.by_project.get(id)) {
                invalidate(&mut self.projects[*index]);
            }
        } else {
            for row in &mut self.projects {
                if let Some(target) = &row.favicon
                    && !matches!(target.state, State::Loaded(_))
                {
                    invalidate(row);
                }
            }
        }
    }
}

impl Desktop {
    fn load_project_favicon(&mut self, watch: u64, project: &str, generation: &str) {
        if self.workspace.feed != Some(watch) {
            return;
        }
        let Some(&index) = self.workspace.by_project.get(project) else {
            return;
        };
        let row = &mut self.workspace.projects[index];
        let Some(target) = &mut row.favicon else {
            return;
        };
        if target.generation != generation
            || row.archived
            || match &target.state {
                State::Loading(_) | State::Missing => true,
                State::Loaded(image) => image.strong_count() > 0,
                State::Idle => false,
            }
        {
            return;
        }
        let operation = operation();
        target.state = State::Loading(operation.clone());
        let request = Request::ProjectFavicon {
            watch,
            project: project.to_owned(),
            cwd: row.path.clone(),
            path: target.path.clone(),
            operation,
        };
        self.write_request(None, request);
    }

    pub(crate) fn project_favicon_result(
        &mut self,
        watch: u64,
        project: &str,
        operation: &str,
        image: Option<Image>,
        retryable: bool,
    ) {
        if self.workspace.feed != Some(watch) {
            return;
        }
        let Some(&index) = self.workspace.by_project.get(project) else {
            return;
        };
        let Some(target) = &mut self.workspace.projects[index].favicon else {
            return;
        };
        if !matches!(&target.state, State::Loading(pending) if pending == operation) {
            return;
        }
        target.state = if let Some(image) = image {
            let image = Arc::new(image);
            let weak = Arc::downgrade(&image);
            let cache = &mut self.workspace.favicon_cache;
            if cache.len() >= 64 {
                cache.pop_front();
            }
            cache.push_back(image);
            State::Loaded(weak)
        } else if retryable {
            State::Idle
        } else {
            State::Missing
        };
    }
}
