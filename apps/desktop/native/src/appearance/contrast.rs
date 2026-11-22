use super::{Desktop, Request, color, label, reset_button};
use rune::{Color, El, Phase, TextStyle, col, el, row, text, tok};
use rune_kit::Kit;

const MIN: u8 = 50;
const MAX: u8 = 200;
const DEFAULT: u8 = 100;
const STEP: i32 = 5;

#[derive(Clone, Copy)]
pub(crate) struct Contrast {
    saved: u8,
    pending: Option<u8>,
    sending: Option<u8>,
    dragging: Option<u8>,
}

impl Default for Contrast {
    fn default() -> Self {
        Self {
            saved: DEFAULT,
            pending: None,
            sending: None,
            dragging: None,
        }
    }
}

impl Contrast {
    fn value(&self) -> u8 {
        self.dragging.or(self.pending).unwrap_or(self.saved)
    }

    pub(crate) fn cancel_unfocused_drag(&mut self, tx: &rune::Tx) -> bool {
        if tx.focused() != Some("appearance-contrast") {
            return self.dragging.take().is_some();
        }
        false
    }

    pub(crate) fn foreground(self, color: Color, background: Color, target: Color) -> Color {
        let level = self.value();
        let (other, blend) = if level < DEFAULT {
            (background, f32::from(DEFAULT - level) / 100.)
        } else {
            (target, f32::from(level - DEFAULT) / 100.)
        };
        let alpha = color.a * (1. - blend) + other.a * blend;
        if alpha == 0. {
            return Color::TRANSPARENT;
        }
        let mut value = color.to_oklab();
        value[3] = alpha;
        for (channel, end) in value[..3].iter_mut().zip(other.to_oklab()) {
            *channel += (end - *channel) * other.a * blend / alpha;
        }
        Color::from_oklab(value)
    }

    pub(crate) fn border(self, color: Color, target: Color) -> Color {
        let level = self.value();
        if level < DEFAULT {
            return color.alpha(color.a * f32::from(level) / 100.);
        }
        let boost = f32::from(level - DEFAULT) / 400.;
        let alpha = color.a * (1. - boost) + boost;
        if alpha == 0. {
            return color;
        }
        let mix = |c: f32, end: f32| {
            let srgb = rune::color::srgb_encode(c);
            rune::color::srgb_decode(srgb + (rune::color::srgb_encode(end) - srgb) * boost / alpha)
        };
        Color {
            r: mix(color.r, target.r),
            g: mix(color.g, target.g),
            b: mix(color.b, target.b),
            a: alpha,
        }
    }

    pub(crate) fn apply(self, theme: &mut tok::Theme, dark: bool) {
        if self.value() == DEFAULT {
            return;
        }
        let target = if dark { Color::WHITE } else { Color::BLACK };
        theme.separator = self.border(theme.separator, theme.text);
        theme.card_border = self.border(theme.card_border, theme.text);
        theme.text = self.foreground(theme.text, theme.bg, target);
        theme.text_secondary = self.foreground(theme.text_secondary, theme.bg, target);
    }

    pub(crate) fn kit(self, dark: bool) -> Kit {
        let mut kit = Kit::neutral(dark);
        if self.value() == DEFAULT {
            return kit;
        }
        let target = if dark { Color::WHITE } else { Color::BLACK };
        let c = &mut kit.c;
        let border_target = c.text_base;
        for fg in [
            &mut c.text_base,
            &mut c.text_muted,
            &mut c.text_faint,
            &mut c.icon_base,
            &mut c.icon_muted,
            &mut c.icon_faint,
            &mut c.text_strong,
            &mut c.text_weak,
            &mut c.icon_weaker,
            &mut c.std_text,
            &mut c.std_icon,
            &mut c.std_icon_strong,
        ] {
            *fg = self.foreground(*fg, c.bg_base, target);
        }
        for border in [
            &mut c.border_base,
            &mut c.border_muted,
            &mut c.border_strong,
            &mut c.border_weak,
            &mut c.border_hover,
            &mut c.border_disabled,
            &mut c.std_border,
        ] {
            *border = self.border(*border, border_target);
        }
        kit.ring = Some(|palette| palette.std_border);
        kit
    }
}

impl Desktop {
    pub(crate) fn receive_contrast(&mut self, saved: u8) {
        self.contrast.saved = saved;
        if self.contrast.sending.take() == self.contrast.pending {
            self.contrast.pending = None;
        }
        self.save_contrast();
    }

    fn save_contrast(&mut self) {
        let Some(value) = self.contrast.pending else {
            return;
        };
        if self.saving_client_appearance {
            return;
        }
        if value == self.contrast.saved {
            self.contrast.pending = None;
        } else if self.write_request(None, Request::SetContrast { value }) {
            self.contrast.sending = Some(value);
            self.saving_client_appearance = true;
        }
    }

    fn choose_contrast(&mut self, value: u8) {
        if self.contrast.dragging.is_some() {
            return;
        }
        self.contrast.pending = Some(value);
        self.save_contrast();
    }
}

pub(super) fn setting(s: &Desktop, narrow: bool, theme: &tok::Theme, dark: bool) -> El {
    let value = s.contrast.value();
    let fraction = f32::from(value - MIN) / f32::from(MAX - MIN);
    let mut title = row()
        .center_y()
        .gap(4.)
        .child(label("Contrast", 14., 500., theme.text));
    if value != DEFAULT {
        title = title.child(
            reset_button("reset-contrast", "Reset contrast to default", theme).on_press_tx(
                |s: &mut Desktop, tx| {
                    s.choose_contrast(DEFAULT);
                    tx.focus_when_shown("appearance-contrast", true);
                },
            ),
        );
    }
    let copy = col().min_w(0.).grow(1.).gap(3.).child(title).child(
        label(
            "Adjust the contrast of colors and borders across the interface.",
            12.,
            400.,
            theme.text_secondary,
        )
        .wrap(),
    );
    let primary = color(if dark { "#346bf1" } else { "#1b4ed8" });
    let muted = color(if dark { "#080808" } else { "#fafafa" });
    let slider = row()
        .key("appearance-contrast")
        .role("slider")
        .label("Contrast")
        .range_value(f64::from(value), f64::from(MIN), f64::from(MAX))
        .range_step(f64::from(STEP))
        .disabled(s.font_smoothing.is_none())
        .no_window_drag()
        .min_w(0.)
        .grow(1.)
        .h(24.)
        .center_y()
        .child(el().w(0.).grow(fraction).h(6.).radius(3.).fill(primary))
        .child(
            el().size(16., 16.)
                .radius(8.)
                .border(2., primary)
                .fill(theme.bg),
        )
        .child(
            el().w(0.)
                .grow(1. - fraction)
                .h(6.)
                .radius(3.)
                .fill(theme.text_secondary.alpha(0.22)),
        )
        .on_step(|s: &mut Desktop, delta, _| {
            let value = s.contrast.value();
            let stop = if delta < 0 {
                value.div_ceil(STEP as u8)
            } else {
                value / STEP as u8
            };
            let level = (i32::from(stop) + delta) * STEP;
            s.choose_contrast(level.clamp(i32::from(MIN), i32::from(MAX)) as u8);
        })
        .on_step_limit(|s: &mut Desktop, max, _| s.choose_contrast(if max { MAX } else { MIN }))
        .on_drag(|s: &mut Desktop, d, tx| {
            let fraction = ((d.x - 8.) / (d.w - 16.).max(1.)).clamp(0., 1.);
            let value =
                MIN + (fraction * f32::from(MAX - MIN) / STEP as f32).round() as u8 * STEP as u8;
            match d.phase {
                Phase::Down => {
                    tx.focus("appearance-contrast", false);
                    s.contrast.dragging = Some(value);
                }
                Phase::Move if s.contrast.dragging.is_some() => s.contrast.dragging = Some(value),
                Phase::Move => return,
                Phase::Up => {
                    if s.contrast.dragging.take().is_some()
                        && !d.cancelled()
                        && tx.focused() == Some("appearance-contrast")
                    {
                        s.choose_contrast(value);
                    }
                }
            }
            tx.instant();
        });
    let output = el()
        .min_w(48.)
        .h(24.)
        .radius(6.)
        .fill(muted)
        .center()
        .child(
            text(format!("{value}%"))
                .font(TextStyle::new(12., 500.).mono())
                .color(theme.text),
        );
    let controls = row().center_y().gap(12.).child(output).child(slider);
    let controls = if narrow {
        controls.w_full()
    } else {
        controls.w(208.)
    };
    let layout = if narrow { col() } else { row().center_y() };
    layout
        .w_full()
        .padding(if narrow { 12. } else { 16. })
        .gap(12.)
        .child(copy)
        .child(controls)
}
