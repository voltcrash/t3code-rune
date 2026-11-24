use super::{Desktop, SettingsLabels, chat::Switch, label};
use rune::{El, col, el, tok};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub(super) title: String,
    confirmations: String,
    pub(super) unpin: String,
    pub(super) unpin_description: String,
    pub(super) unpin_reset: String,
    pub(super) archive: String,
    pub(super) archive_description: String,
    pub(super) archive_reset: String,
    pub(super) delete: String,
    pub(super) delete_description: String,
    pub(super) delete_reset: String,
}

pub(super) const SETTINGS: rune::Icon = rune::Icon {
    name: "settings-2",
    view: 24.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 17H5"/><path d="M19 7h-9"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/></g>"#,
};

pub(super) fn view(s: &Desktop, labels: &SettingsLabels, narrow: bool, theme: &tok::Theme) -> El {
    let mut card = col()
        .w_full()
        .radius(12.)
        .clip_rounded(12.)
        .border(1., theme.separator);
    for (i, switch) in [
        Switch::ConfirmUnpin,
        Switch::ConfirmArchive,
        Switch::ConfirmDelete,
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            card = card.child(el().w_full().h(1.).fill(theme.separator));
        }
        card = card.child(switch.row(s, labels, narrow, theme));
    }
    col().w_full().child(
        col()
            .w_full()
            .gap(10.)
            .child(label(&labels.general.confirmations, 16., 600., theme.text).pl(16.))
            .child(card),
    )
}
