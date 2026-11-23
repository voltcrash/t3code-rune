use super::{Desktop, Request, label, reset_button};
use rune::{El, Tx, col, row, tok};
use rune_kit::{Placement, Select, SelectAppearance};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChatWidth {
    #[default]
    Comfortable,
    Wide,
    Full,
}

impl ChatWidth {
    pub(crate) fn limit(self) -> Option<f32> {
        match self {
            Self::Comfortable => Some(736.),
            Self::Wide => Some(1152.),
            Self::Full => None,
        }
    }

    pub(crate) fn lane(self, element: El) -> El {
        let element = element.w_full().self_center();
        match self.limit() {
            Some(width) => element.max_w(width),
            None => element,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Wide => "Wide",
            Self::Full => "Full",
        }
    }

    fn choose(self, s: &mut Desktop, tx: &mut Tx) {
        s.chat_width_menu = false;
        tx.focus_when_shown("chat-width-trigger", true);
        if !s.saving_client_settings && self != s.chat_width {
            s.saving_client_settings = s.write_request(None, Request::SetChatWidth { width: self });
        }
    }
}

pub(super) fn setting(
    s: &Desktop,
    narrow: bool,
    theme: &tok::Theme,
    dark: bool,
    popups: &mut Vec<El>,
) -> El {
    let disabled = s.font_smoothing.is_none() || s.saving_client_settings;
    let options = ["Comfortable (default)", "Wide", "Full"];
    let current = if s.chat_width_menu && s.chat_width == ChatWidth::Comfortable {
        options[0]
    } else {
        s.chat_width.label()
    };
    let picker = Select::new("chat-width", &options)
        .label("Chat width")
        .current(Some(current))
        .appearance(SelectAppearance::Field)
        .placement(Placement::BottomEnd)
        .width(160.)
        .open(s.chat_width_menu)
        .disabled(disabled)
        .on_toggle(|s: &mut Desktop| s.chat_width_menu = !s.chat_width_menu)
        .on_close(|s: &mut Desktop| s.chat_width_menu = false)
        .on_choose(|s: &mut Desktop, choice, tx| match choice {
            "Comfortable (default)" => ChatWidth::Comfortable.choose(s, tx),
            "Wide" => ChatWidth::Wide.choose(s, tx),
            "Full" => ChatWidth::Full.choose(s, tx),
            _ => {}
        })
        .build(&s.contrast.kit(dark), popups);
    let picker = if narrow { picker.w_full() } else { picker };

    let mut title = row()
        .center_y()
        .gap(4.)
        .child(label("Chat width", 14., 500., theme.text));
    if s.chat_width != ChatWidth::Comfortable {
        title = title.child(
            reset_button("reset-chat-width", "Reset chat width to default", theme)
                .disabled(disabled)
                .on_press_tx(|s: &mut Desktop, tx| {
                    ChatWidth::Comfortable.choose(s, tx);
                }),
        );
    }
    let copy = col().min_w(0.).grow(1.).gap(3.).child(title).child(
        label(
            "Set how wide messages and the composer can grow on large screens.",
            12.,
            400.,
            theme.text_secondary,
        )
        .wrap(),
    );
    let layout = if narrow { col() } else { row().center_y() };
    layout
        .w_full()
        .padding(if narrow { 12. } else { 16. })
        .gap(12.)
        .child(copy)
        .child(picker)
}
