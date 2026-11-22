use crate::{Desktop, Request, workspace::label};
use rune::{El, Icon, TextStyle, col, el, icon, row, spacer, text, tok};
use rune_kit::glyphs;
use std::collections::HashSet;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Banner {
    key: String,
    refresh_label: String,
    dismiss_label: String,
    refreshing: String,
    refresh: String,
    title: String,
    message: String,
    is_warning: bool,
    is_advisory: bool,
}

type Target<'a> = (u64, &'a str, Option<&'a str>);

#[derive(Default)]
pub(super) struct Status {
    target: Option<(u64, String, Option<String>)>,
    subscription: u64,
    banner: Option<Banner>,
    refresh: Option<u64>,
    error: Option<String>,
    dismissed: HashSet<String>,
}

impl Status {
    pub(super) fn for_new_connection(&mut self) -> Self {
        Self {
            dismissed: std::mem::take(&mut self.dismissed),
            ..Default::default()
        }
    }

    fn target(&self) -> Option<Target<'_>> {
        self.target
            .as_ref()
            .map(|(watch, project, instance)| (*watch, project.as_str(), instance.as_deref()))
    }
}

impl Desktop {
    fn status_target(&self) -> Option<Target<'_>> {
        let feed = self.workspace.feed?;
        if let Some(project) = self.new_threads.active.as_deref() {
            return Some((feed, project, self.new_threads.provider_instance()));
        }
        let thread = self.workspace.visible_thread()?;
        let current = thread.permission.as_ref()?.instance.as_str();
        let instance = self
            .workspace
            .drafts
            .get(&thread.id)
            .map_or(current, |draft| draft.target_instance(current));
        Some((feed, &thread.project, Some(instance)))
    }

    pub(super) fn sync_provider_status(&mut self) -> bool {
        let target = self.status_target();
        if target == self.provider_status.target() {
            return false;
        }
        let next = target.map(|(watch, project, instance)| {
            (watch, project.to_owned(), instance.map(str::to_owned))
        });
        let previous = self.provider_status.target.take();
        self.provider_status.banner = None;
        self.provider_status.refresh = None;
        self.provider_status.error = None;
        if let Some((watch, project, instance)) = &next {
            let operation = self.next_request_id();
            if self.write_request(
                Some(operation),
                Request::WatchProviderStatus {
                    watch: *watch,
                    project: Some(project),
                    instance: instance.as_deref(),
                },
            ) {
                self.provider_status.target = next;
                self.provider_status.subscription = operation;
            }
        } else if let Some((watch, _, _)) = previous {
            self.write_request(
                None,
                Request::WatchProviderStatus {
                    watch,
                    project: None,
                    instance: None,
                },
            );
        }
        true
    }

    pub(super) fn receive_provider_status(
        &mut self,
        id: Option<u64>,
        watch: u64,
        banner: Option<Banner>,
    ) {
        if self.workspace.feed == Some(watch) && id == Some(self.provider_status.subscription) {
            if self.provider_status.banner.as_ref().map(|b| &b.key)
                != banner.as_ref().map(|b| &b.key)
            {
                self.provider_status.refresh = None;
                self.provider_status.error = None;
            }
            self.provider_status.banner = banner;
        }
    }

    pub(super) fn refreshed_provider(
        &mut self,
        id: Option<u64>,
        watch: u64,
        error: Option<String>,
    ) {
        if self.workspace.feed == Some(watch) && id.is_some() && id == self.provider_status.refresh
        {
            self.provider_status.refresh = None;
            self.provider_status.error = error;
        }
    }

    fn refresh_provider(&mut self) {
        let Some((watch, _, _)) = self.status_target() else {
            return;
        };
        let Some(banner) = &self.provider_status.banner else {
            return;
        };
        if self.provider_status.refresh.is_some()
            || self.status_target() != self.provider_status.target()
        {
            return;
        }
        let key = banner.key.clone();
        let id = self.next_request_id();
        if self.write_request(Some(id), Request::RefreshProvider { watch, key: &key }) {
            self.provider_status.refresh = Some(id);
            self.provider_status.error = None;
        }
    }
}

const INFO: Icon = Icon {
    name: "provider-status-info",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="9.5"/><path d="M12 11v5m0-9v.2"/></g>"#,
};
pub(super) fn view(
    s: &Desktop,
    width: f32,
    top: f32,
    theme: &tok::Theme,
    dark: bool,
) -> Option<El> {
    let status = &s.provider_status;
    let banner = status.banner.as_ref()?;
    if status.dismissed.contains(&banner.key) || s.status_target() != status.target() {
        return None;
    }
    let (accent, foreground, surface) = match (banner.is_warning, dark) {
        (true, true) => ("#fe9a00", "#ffb900", "#211c12"),
        (true, false) => ("#fe9a00", "#bb4d00", "#fcf8f1"),
        (false, true) => ("#fb414a", "#ff6467", "#211415"),
        (false, false) => ("#fb2c36", "#c10007", "#fcf4f4"),
    };
    let [accent, foreground, surface] =
        [accent, foreground, surface].map(|hex| rune::color::hex(hex).unwrap());
    let busy = status.refresh.is_some();
    let mut copy = col()
        .min_w(0.)
        .shrink()
        .gap(2.)
        .pt(3.)
        .child(
            text(&banner.title)
                .font(TextStyle::new(14., 500.).lh(20.))
                .color(foreground)
                .wrap(),
        )
        .child(
            text(&banner.message)
                .font(TextStyle::new(14., 400.).lh(20.))
                .color(foreground.alpha(0.8))
                .wrap()
                .max_lines(3)
                .help(&banner.message),
        );
    if let Some(error) = &status.error {
        copy = copy.child(label(error, 13., 400., theme.text).wrap());
    }
    let refresh = row()
        .key("refresh-provider")
        .role("button")
        .label(&banner.refresh_label)
        .focusable(!busy)
        .disabled(busy)
        .h(24.)
        .px(6.)
        .gap(4.)
        .center_y()
        .radius(6.)
        .fill(theme.bg)
        .border(1., theme.separator)
        .hover(theme.card)
        .child(icon(&crate::workspace::palette::REFRESH, 14.).color(theme.text_secondary))
        .child(label(
            if busy {
                &banner.refreshing
            } else {
                &banner.refresh
            },
            12.,
            500.,
            theme.text,
        ))
        .on_press(Desktop::refresh_provider);
    copy = copy.child(row().w_full().pt(6.).child(spacer()).child(refresh));
    let close = el()
        .key("dismiss-provider")
        .role("button")
        .label(&banner.dismiss_label)
        .help(&banner.dismiss_label)
        .size(24., 24.)
        .center()
        .focusable(true)
        .radius(5.)
        .hover(theme.separator)
        .child(icon(&glyphs::CLOSE, 14.).color(theme.text_secondary))
        .on_press_tx(|s: &mut Desktop, tx| {
            if let Some(b) = &s.provider_status.banner {
                s.provider_status.dismissed.insert(b.key.clone());
            }
            let focus = if s.new_threads.active.is_some() {
                "new-thread-prompt"
            } else {
                "message-input"
            };
            if s.can_operate {
                tx.focus_when_shown(focus, true);
            }
        });
    let width = (width - 32.).max(64.);
    let card = row()
        .key("provider-status")
        .role(if banner.is_advisory {
            "status"
        } else {
            "alert"
        })
        .max_w(width)
        .min_w(0.)
        .radius(12.)
        .fill(surface)
        .border(1., accent.alpha(0.32))
        .px(14.)
        .pt(12.)
        .pb(13.)
        .gap(8.)
        .child(icon(&INFO, 16.).color(accent).mt(4.))
        .child(copy)
        .child(close);
    Some(
        row()
            .absolute(16., top + 12.)
            .w(width)
            .justify_center()
            .child(card),
    )
}
