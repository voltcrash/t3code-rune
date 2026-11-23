//! The web client's alert dialog: title, optional description, and Cancel/Confirm in a footer bar.
use crate::{Desktop, workspace::label};
use rune::{Color, El, col, el, row, tok};

pub(crate) struct Confirm<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub cancel: &'a str,
    pub confirm: &'a str,
    pub destructive: bool,
}

fn button(key: String, caption: &str, color: Color) -> El {
    el().key(key)
        .role("button")
        .label(caption)
        .focusable(true)
        .no_window_drag()
        .h(32.)
        .px(11.)
        .radius(8.)
        .center()
        .child(label(caption, 14., 500., color))
}

/// The web client's `--destructive`: red-500, mixed 90% with white in dark mode.
fn destructive(theme: &tok::Theme) -> Color {
    let dark = theme.bg.r + theme.bg.g + theme.bg.b < 1.5;
    rune::color::hex(if dark { "#fb414a" } else { "#fb2c36" }).unwrap()
}

pub(crate) fn overlay(
    c: Confirm,
    theme: &tok::Theme,
    on_cancel: fn(&mut Desktop),
    on_confirm: fn(&mut Desktop),
    overlays: &mut Vec<El>,
) {
    let mut header = col()
        .w_full()
        .padding(24.)
        .gap(8.)
        .child(label(c.title, 20., 600., theme.text).wrap());
    if let Some(description) = c.description {
        header = header.child(label(description, 14., 400., theme.text_secondary).wrap());
    }
    let cancel = button(format!("{}-cancel", c.key), c.cancel, theme.text)
        .border(1., theme.separator)
        .fill(theme.text.alpha(0.03))
        .hover(theme.text.alpha(0.08))
        .on_press(on_cancel);
    let (fill, text) = if c.destructive {
        (destructive(theme), Color::WHITE)
    } else {
        (theme.text, theme.bg)
    };
    let confirm = button(format!("{}-confirm", c.key), c.confirm, text)
        .fill(fill)
        .hover(fill.alpha(0.9))
        .on_press(on_confirm);
    let panel = col()
        .key(format!("{}-dialog", c.key))
        .role("alertdialog")
        .label(c.title)
        .no_window_drag()
        .w_full()
        .max_w(512.)
        .radius(16.)
        .clip_rounded(16.)
        .border(1., theme.text.alpha(0.08))
        .fill(theme.bg)
        .child(header)
        .child(
            row()
                .w_full()
                .px(24.)
                .py(16.)
                .gap(8.)
                .border_sides([1., 0., 0., 0.], theme.separator)
                .fill(theme.text.alpha(0.03))
                .child(rune::spacer())
                .child(cancel)
                .child(confirm),
        )
        .modal(false, on_cancel);
    overlays.push(
        el().absolute(0., 0.)
            .w_full()
            .h_full()
            .backdrop_blur(4.)
            .fill(theme.bg.alpha(0.64))
            .a11y_hidden(),
    );
    overlays.push(
        el().key(format!("{}-layer", c.key))
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .padding(16.)
            .center()
            .child(panel),
    );
}
