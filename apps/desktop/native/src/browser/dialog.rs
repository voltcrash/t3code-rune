use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Dialog {
    pub id: u64,
    #[serde(rename = "type")]
    kind: String,
    message: String,
    pub default_value: String,
}

impl Dialog {
    pub(super) fn prompt(&self) -> bool {
        self.kind == "prompt"
    }
}

impl Desktop {
    fn answer_browser_dialog(&mut self, id: u64, accept: bool) {
        let Some(p) = &self.workspace.browser.panel else {
            return;
        };
        if p.page.controlled && p.page.dialog.as_ref().is_some_and(|d| d.id == id) {
            self.browser_request(
                json!({"action":"dialog", "tab":p.page.selected, "dialogId":id,
                "accept":accept, "promptText":p.answer.text}),
            );
        }
    }
}

pub(super) fn view(p: &Panel, d: &Dialog, labels: &Labels, theme: &tok::Theme) -> El {
    let id = d.id;
    let label = |name: &str| super::super::workspace::label(name, 13., 400., theme.text);
    let mut card = col()
        .role("dialog")
        .label(&labels.dialog)
        .w_full()
        .px(12.)
        .py(12.)
        .gap(8.)
        .radius(8.)
        .border(1., theme.separator)
        .fill(theme.bg)
        .child(
            col()
                .w_full()
                .max_h(96.)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(label(&d.message).wrap()),
        );
    if p.page.controlled {
        if d.prompt() {
            card = card.child(
                text_input(&p.answer, TextStyle::new(13., 400.))
                    .key("browser-response")
                    .label(&labels.response)
                    .single_line()
                    .w_full()
                    .h(28.)
                    .px(6.)
                    .radius(5.)
                    .color(theme.text)
                    .border(1., theme.separator)
                    .on_edit(|s: &mut Desktop| {
                        &mut s.workspace.browser.panel.as_mut().unwrap().answer
                    })
                    .on_submit(move |s: &mut Desktop, _| s.answer_browser_dialog(id, true)),
            );
        }
        card = card.child(
            row()
                .w_full()
                .gap(6.)
                .child(rune::spacer())
                .child(
                    button(
                        &labels.dismiss,
                        label(&labels.dismiss),
                        true,
                        theme,
                        move |s, _| s.answer_browser_dialog(id, false),
                    )
                    .key("browser-dismiss"),
                )
                .child(
                    button(
                        &labels.accept,
                        label(&labels.accept),
                        true,
                        theme,
                        move |s, _| s.answer_browser_dialog(id, true),
                    )
                    .fill(theme.hover),
                ),
        );
    } else {
        card = card.child(label(&labels.take_to_respond).wrap());
    }
    col().absolute(0., 8.).w_full().px(8.).child(card)
}
