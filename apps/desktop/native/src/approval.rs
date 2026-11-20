use super::workspace::{Conversation, LiveStatus};
use super::{Desktop, Request};
use rune::{El, Icon, ScrollSpec, TextStyle, col, el, icon, row, text, tok};
use rune_kit::{Kit, Menu, MenuItem, Placement};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Approval {
    pub id: String,
    pub version: String,
    kind: String,
    detail: Option<String>,
    reason: Option<String>,
    app: Option<String>,
    explanation: Option<String>,
    live: bool,
    present: bool,
    options: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    decision: String,
    label: String,
    warning: Option<String>,
}

impl Conversation {
    pub(super) fn replace_approvals(&mut self, approvals: Vec<Approval>) {
        if !approvals
            .iter()
            .any(|a| Some(&a.version) == self.approval_menu.as_ref())
        {
            self.approval_menu = None;
        }
        self.approvals = approvals;
    }
}

impl Desktop {
    fn respond(&mut self, version: &str, decision: &str) {
        if self.pending || !self.can_operate {
            return;
        }
        let Some(pane) = self.workspace.conversation.as_mut() else {
            return;
        };
        if pane.status != LiveStatus::Live {
            return;
        }
        let Some(approval) = pane
            .approvals
            .iter()
            .find(|a| a.version == version && a.live)
        else {
            return;
        };
        if !approval.options.iter().any(|c| c.decision == decision) {
            return;
        }
        pane.approval_menu = None;
        let request = Request::Approval {
            thread: pane.id.clone(),
            request: approval.id.clone(),
            version: approval.version.clone(),
            decision: decision.into(),
            operation: Uuid::new_v4().to_string(),
        };
        self.send(request);
    }
}

const TERMINAL: Icon = Icon {
    name: "terminal",
    view: 24.,
    body: r#"<path d="m4 6 6 6-6 6m9 0h7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const SHIELD: Icon = Icon {
    name: "shield",
    view: 24.,
    body: r#"<path d="M12 22s8-4 8-11V5l-8-3-8 3v6c0 7 8 11 8 11Z" fill="none" stroke="currentColor" stroke-width="2"/>"#,
};
const EDIT: Icon = Icon {
    name: "file-change",
    view: 24.,
    body: r#"<path d="M12 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-7m-3-9 4 4-9 9-5 1 1-5Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>"#,
};
const WRENCH: Icon = Icon {
    name: "wrench",
    view: 24.,
    body: r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0L21.5 5.5a6 6 0 0 1-7.8 7.8l-7.9 7.9a2.1 2.1 0 0 1-3-3l7.9-7.9a6 6 0 0 1 7.8-7.8Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>"#,
};
const HAMMER: Icon = Icon {
    name: "hammer",
    view: 24.,
    body: r#"<path d="m15 12-9 9a2 2 0 0 1-3-3l9-9m6 4 4-4-3-3h-2l-3-3H8l5 5Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>"#,
};

fn inline_button(
    label: &str,
    key: String,
    color: rune::Color,
    enabled: bool,
    press: impl Fn(&mut Desktop) + 'static,
) -> El {
    let button = el().key(key).label(label).role("button").py(2.0).child(
        text(label)
            .font(TextStyle::new(12.0, 500.0))
            .color(color)
            .wrap_anywhere(),
    );
    if enabled {
        button.focusable(true).on_press(press)
    } else {
        button.disabled(true).opacity(0.5)
    }
}

pub(super) fn view(
    app: &Desktop,
    approval: &Approval,
    kit: &Kit,
    theme: &tok::Theme,
    overlays: &mut Vec<El>,
) -> El {
    let (title, glyph) = match approval.kind.as_str() {
        "file-read" => ("File read approval", &rune_kit::glyphs::EYE),
        "file-change" => ("File change approval", &EDIT),
        "permission" => ("App permission approval", &WRENCH),
        "mcp-elicitation" => ("App access approval", &HAMMER),
        _ => ("Command approval", &TERMINAL),
    };
    let details = col()
        .key(format!("detail-{}", approval.id))
        .grow(1.0)
        .min_w(0.0)
        .max_h(160.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .focusable(true)
        .child(
            text(approval.detail.as_deref().unwrap_or(title))
                .font(TextStyle::new(12., 400.).mono().lh(20.))
                .wrap_anywhere()
                .selectable(true)
                .color(theme.text),
        );
    let mut body = col()
        .key(format!("approval-{}", approval.version))
        .role("group")
        .label(title)
        .w_full()
        .gap(3.)
        .py(4.)
        .child(
            row()
                .w_full()
                .gap(14.)
                .child(icon(glyph, 16.).color(theme.text_secondary))
                .child(details),
        );
    let mut info = col().w_full().pl(30.).gap(4.);
    for value in [&approval.app, &approval.reason, &approval.explanation]
        .into_iter()
        .flatten()
    {
        info = info.child(
            text(value)
                .font(TextStyle::new(12., 400.))
                .color(theme.text_secondary)
                .wrap_anywhere()
                .selectable(true),
        );
    }
    let status = if !approval.live {
        "Provider process is gone. Interrupt or restart the run to respond."
    } else if !approval.present {
        "Load earlier messages to see the approval details."
    } else {
        "Waiting for approval"
    };
    let mut actions = row().w_full().flex_wrap().center_y().gap(8.).child(
        row()
            .center_y()
            .gap(4.)
            .child(icon(&SHIELD, 12.).color(kit.c.warning.fg))
            .child(
                text(status)
                    .font(TextStyle::new(12., 400.))
                    .color(kit.c.warning.fg)
                    .wrap_anywhere(),
            ),
    );
    if approval.live && !approval.options.is_empty() {
        actions = actions.child(
            text("·")
                .font(TextStyle::new(12., 400.))
                .color(theme.text_secondary),
        );
        let enabled = app.can_operate
            && !app.pending
            && app
                .workspace
                .conversation
                .as_ref()
                .is_some_and(|p| p.status == LiveStatus::Live);
        let version = &approval.version;
        let mut more = Vec::new();
        for choice in &approval.options {
            if let Some(warning) = &choice.warning {
                info = info.child(
                    text(format!("{} — {warning}", choice.label))
                        .font(TextStyle::new(12., 400.))
                        .color(kit.c.warning.fg)
                        .wrap_anywhere()
                        .selectable(true),
                );
            }
            if choice.decision != "accept" && choice.decision != "decline" {
                more.push(choice);
                continue;
            }
            let target = version.clone();
            let decision = choice.decision.clone();
            let color = if decision == "accept" {
                theme.text
            } else {
                theme.text_secondary
            };
            let mut button = inline_button(
                &choice.label,
                format!("{version}-{decision}"),
                color,
                enabled,
                move |s| s.respond(&target, &decision),
            );
            if let Some(warning) = &choice.warning {
                button = button.description(warning).help(warning);
            }
            actions = actions.child(button);
        }
        if !more.is_empty() {
            let target = version.clone();
            let trigger = inline_button(
                "More",
                format!("{version}-more"),
                theme.text_secondary,
                enabled,
                move |s| {
                    if let Some(pane) = &mut s.workspace.conversation {
                        pane.approval_menu = Some(target.clone());
                    }
                },
            )
            .label("More approval options");
            let open = app
                .workspace
                .conversation
                .as_ref()
                .and_then(|p| p.approval_menu.as_ref())
                == Some(version);
            let mut menu = Menu::new(format!("approval-menu-{version}"))
                .trigger(trigger)
                .placement(Placement::TopEnd)
                .open(open && enabled)
                .on_close(|s: &mut Desktop| {
                    if let Some(pane) = &mut s.workspace.conversation {
                        pane.approval_menu = None;
                    }
                });
            for choice in more {
                let target = version.clone();
                let decision = choice.decision.clone();
                let mut item = MenuItem::new(&choice.label)
                    .on_select(move |s: &mut Desktop| s.respond(&target, &decision));
                if let Some(warning) = &choice.warning {
                    item = item.description(warning);
                }
                menu = menu.item(item);
            }
            actions = actions.child(menu.build(kit, overlays));
        }
    }
    body = body.child(info.child(actions));
    body
}
