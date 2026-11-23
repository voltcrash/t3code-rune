use super::{Desktop, El, Panel, TextEdit, TextStyle, Tx, button, json, text_input, tok};
use rune_kit::{Kit, Menu, MenuItem, Placement};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub(super) struct Labels {
    pub show: String,
    pub hide: String,
    close: String,
    title: String,
    dimensions: String,
    limits: Limits,
    preset: String,
    responsive: String,
    width: String,
    height: String,
    rotate: String,
    lock: String,
    unlock: String,
    presets: Vec<Preset>,
}
#[derive(Deserialize)]
struct Limits {
    min: u32,
    max: u32,
    area: u32,
}

#[derive(Deserialize)]
struct Preset {
    id: String,
    label: String,
    detail: String,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Viewport {
    width: u32,
    height: u32,
    preset_id: Option<String>,
}

#[derive(Clone, Copy)]
enum Axis {
    Width,
    Height,
}

impl Axis {
    fn key(self) -> &'static str {
        match self {
            Self::Width => "browser-device-width",
            Self::Height => "browser-device-height",
        }
    }
    fn edit(self, toolbar: &mut Toolbar) -> &mut TextEdit {
        match self {
            Self::Width => &mut toolbar.width,
            Self::Height => &mut toolbar.height,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum LockChange {
    Rotate,
    Preset(f64),
}

#[derive(Default)]
pub(super) struct Toolbar {
    width: TextEdit,
    height: TextEdit,
    edited: Option<Axis>,
    aspect_ratio: Option<f64>,
    pub picker: bool,
}

impl Toolbar {
    pub fn sync(&mut self, viewport: Option<&Viewport>, new_tab: bool) {
        self.edited = None;
        self.picker = false;
        if new_tab || viewport.is_none() {
            self.aspect_ratio = None;
        }
        if let Some(size) = viewport {
            self.width.replace_text(size.width.to_string());
            self.height.replace_text(size.height.to_string());
        }
    }
    pub fn confirm(&mut self, change: LockChange) {
        if let Some(ratio) = self.aspect_ratio {
            self.aspect_ratio = Some(match change {
                LockChange::Rotate => 1. / ratio,
                LockChange::Preset(ratio) => ratio,
            });
        }
    }
    fn update(&mut self, axis: Axis, limits: &Limits) {
        self.edited = Some(axis);
        let Some(ratio) = self.aspect_ratio else {
            return;
        };
        let Ok(value) = axis.edit(self).text.parse::<u32>() else {
            return;
        };
        if !(limits.min..=limits.max).contains(&value) {
            return;
        }
        let to_primary = |value: f64| match axis {
            Axis::Width => value * ratio,
            Axis::Height => value / ratio,
        };
        let to_companion = |value: u32| match axis {
            Axis::Width => (f64::from(value) / ratio).round() as u32,
            Axis::Height => (f64::from(value) * ratio).round() as u32,
        };
        let min = f64::from(limits.min);
        let max = f64::from(limits.max);
        let minimum = min.max(to_primary(min)).ceil() as u32;
        let maximum = max
            .min(to_primary(max))
            .min(to_primary(f64::from(limits.area)).sqrt())
            .floor() as u32;
        let mut primary = value.max(minimum).min(maximum);
        let mut next = to_companion(primary);
        while primary * next > limits.area && primary > minimum {
            primary -= 1;
            next = to_companion(primary);
        }
        let other = match axis {
            Axis::Width => &mut self.height,
            Axis::Height => &mut self.width,
        };
        other.replace_text(next.to_string());
    }
    fn edit_request(&self) -> Option<Value> {
        Some(json!({
            "width": self.width.text.parse::<u32>().ok()?,
            "height": self.height.text.parse::<u32>().ok()?,
            "axis": match self.edited? { Axis::Width => "width", Axis::Height => "height" },
            "aspectRatio": self.aspect_ratio,
        }))
    }
}

impl Desktop {
    pub(super) fn change_device(&mut self, mut change: Value) {
        let Some(panel) = &mut self.workspace.browser.panel else {
            return;
        };
        let Some(tab) = panel.page.tab() else { return };
        if !panel.page.can_write || panel.page.busy {
            return;
        }
        if matches!(change["kind"].as_str(), Some("edit" | "rotate"))
            && let Some(edit) = panel.device.edit_request()
        {
            change["edit"] = edit;
        }
        panel.device.sync(tab.device.as_ref(), false);
        let id = tab.id.clone();
        self.browser_request(json!({"action":"viewport","tab":id,"change":change}));
    }

    pub(super) fn toggle_device(&mut self) {
        let Some(panel) = &self.workspace.browser.panel else {
            return;
        };
        let fixed = panel.page.tab().is_some_and(|tab| tab.device.is_some());
        let [width, height] = panel.size;
        self.change_device(if fixed {
            json!({"kind":"fill"})
        } else {
            json!({"kind":"show","width":width,"height":height})
        });
    }

    pub(super) fn sync_device_focus(&mut self, tx: &Tx) -> bool {
        let Some(p) = &self.workspace.browser.panel else {
            return false;
        };
        if p.device.edited.is_some()
            && !p.device.picker
            && p.page.can_write
            && !p.page.busy
            && !tx
                .focused()
                .is_some_and(|key| key.starts_with("browser-device-"))
        {
            self.change_device(json!({"kind":"edit"}));
            return true;
        }
        false
    }
}

fn dimension(p: &Panel, axis: Axis, name: &str, enabled: bool, theme: &tok::Theme) -> El {
    let edit = match axis {
        Axis::Width => &p.device.width,
        Axis::Height => &p.device.height,
    };
    let field = text_input(edit, TextStyle::new(12., 400.))
        .key(axis.key())
        .label(name)
        .role("textbox")
        .single_line()
        .w(48.)
        .h(24.)
        .px(4.)
        .radius(4.)
        .border(1., theme.separator)
        .color(theme.text);
    if !enabled {
        return field.disabled(true);
    }
    field
        .on_edit(move |s: &mut Desktop| {
            axis.edit(&mut s.workspace.browser.panel.as_mut().unwrap().device)
        })
        .on_input_change(move |s: &mut Desktop, _| {
            let browser = &mut s.workspace.browser;
            if let (Some(p), Some(labels)) = (&mut browser.panel, &browser.labels) {
                p.device.update(axis, &labels.device.limits);
            }
        })
        .on_submit(|s: &mut Desktop, _| s.change_device(json!({"kind":"edit"})))
        .on_key(|s: &mut Desktop, ev, _| {
            if ev.key == rune::Key::Escape
                && let Some(p) = &mut s.workspace.browser.panel
                && p.device.edited.is_some()
            {
                p.device
                    .sync(p.page.tab().and_then(|t| t.device.as_ref()), false);
                return true;
            }
            false
        })
}

const ROTATE: rune::Icon = rune::Icon {
    name: "browser-rotate-device",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="4" width="10" height="16" rx="2" transform="rotate(42 12 12)"/><path d="M3 9V4h5m13 11v5h-5"/></g>"#,
};
const LOCK: rune::Icon = rune::Icon {
    name: "browser-aspect-lock",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="m10 13 4-4m-5 1-3 3a3.5 3.5 0 0 0 5 5l3-3m1-1 3-3a3.5 3.5 0 0 0-5-5l-3 3"/></g>"#,
};

pub(super) fn view(
    p: &Panel,
    size: &Viewport,
    labels: &Labels,
    theme: &tok::Theme,
    kit: &Kit,
    overlays: &mut Vec<El>,
) -> El {
    let enabled = p.page.can_write && !p.page.busy;
    let selected = labels
        .presets
        .iter()
        .find(|v| Some(&v.id) == size.preset_id.as_ref());
    let label = |s: &str| crate::workspace::label(s, 12., 400., theme.text_secondary);
    let glyph = |icon| rune::icon(icon, 15.).color(theme.text_secondary);
    let trigger = button(
        &labels.preset,
        rune::row()
            .center_y()
            .gap(3.)
            .w_full()
            .child(
                label(selected.map_or(&labels.responsive, |v| &v.label))
                    .grow(1.)
                    .min_w(0.)
                    .single_line()
                    .truncate(),
            )
            .child(glyph(&rune_kit::glyphs::CHEVRON_DOWN)),
        enabled,
        theme,
        |s, _| {
            if let Some(p) = &mut s.workspace.browser.panel {
                p.device.picker = !p.device.picker;
            }
        },
    )
    .w(if p.size[0] >= 440. { 130. } else { 92. });
    let mut picker = Menu::new("browser-device-preset")
        .open(p.device.picker && enabled)
        .trigger(trigger)
        .min_width(280.)
        .placement(Placement::BottomStart)
        .on_close(|s: &mut Desktop| {
            if let Some(p) = &mut s.workspace.browser.panel {
                p.device.picker = false;
            }
        });
    if p.device.picker && enabled {
        picker = picker.item(
            MenuItem::new(&labels.responsive)
                .checked(selected.is_none())
                .on_select(|s: &mut Desktop| s.change_device(json!({"kind":"responsive"}))),
        );
        for preset in &labels.presets {
            let id = preset.id.clone();
            picker = picker.item(
                MenuItem::new(format!("{} · {}", preset.label, preset.detail))
                    .checked(Some(&id) == size.preset_id.as_ref())
                    .on_select(move |s: &mut Desktop| {
                        s.change_device(json!({"kind":"preset","id":id}))
                    }),
            );
        }
    }
    let mut toolbar = rune::row()
        .role("toolbar")
        .label(&labels.title)
        .w_full()
        .h(38.)
        .gap(3.)
        .px(4.)
        .center_y();
    if p.size[0] >= 560. {
        toolbar = toolbar.child(label(&labels.dimensions));
    }
    toolbar
        .child(picker.build(kit, overlays))
        .child(dimension(p, Axis::Width, &labels.width, enabled, theme))
        .child(label("×"))
        .child(dimension(p, Axis::Height, &labels.height, enabled, theme))
        .child(
            button(
                if p.device.aspect_ratio.is_some() {
                    &labels.unlock
                } else {
                    &labels.lock
                },
                glyph(&LOCK),
                enabled,
                theme,
                |s, _| {
                    if let Some(p) = &mut s.workspace.browser.panel {
                        p.device.aspect_ratio = if p.device.aspect_ratio.is_some() {
                            None
                        } else {
                            p.page
                                .tab()
                                .and_then(|t| t.device.as_ref())
                                .map(|size| f64::from(size.width) / f64::from(size.height))
                        };
                    }
                },
            )
            .key("browser-device-lock")
            .role("switch")
            .fill(if p.device.aspect_ratio.is_some() {
                theme.hover
            } else {
                rune::Color::TRANSPARENT
            })
            .checked(p.device.aspect_ratio.is_some()),
        )
        .child(
            button(&labels.rotate, glyph(&ROTATE), enabled, theme, |s, _| {
                s.change_device(json!({"kind":"rotate"}))
            })
            .key("browser-device-rotate"),
        )
        .child(rune::el().grow(1.))
        .child(
            button(&labels.close, label("×"), enabled, theme, |s, tx| {
                s.change_device(json!({"kind":"fill"}));
                tx.focus_when_shown("browser-more-trigger", true);
            })
            .key("browser-device-close"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_width_preview_matches_the_committed_rounding() {
        let mut toolbar = Toolbar {
            width: TextEdit::new("600"),
            aspect_ratio: Some(240. / 263.),
            ..Default::default()
        };
        toolbar.update(
            Axis::Width,
            &Limits {
                min: 240,
                max: 4096,
                area: 8_000_000,
            },
        );
        assert_eq!(toolbar.height.text, "658");
    }
}
