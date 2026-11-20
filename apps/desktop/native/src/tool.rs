use super::{Desktop, MessageRow, Request, activity::disclosure, label};
use rune::{El, ScrollSpec, TextStyle, col, rich, text, tok};
use rune_kit::{Kit, glyphs, syntax};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Tool {
    action: String,
}

#[derive(Deserialize)]
struct Block {
    title: Option<String>,
    text: String,
    language: Option<String>,
    href: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct Detail {
    #[serde(default)]
    blocks: Vec<Block>,
    #[serde(default)]
    images: Vec<String>,
    message: Option<String>,
}

pub(super) struct Open {
    operation: String,
    detail: Detail,
}

impl Desktop {
    pub(crate) fn update_tool(&mut self, key: &str, operation: &str, detail: Detail) {
        if let Some(pane) = &mut self.workspace.conversation
            && let Some(open) = pane.tools.get_mut(key)
            && open.operation == operation
        {
            open.detail = detail;
            pane.revision += 1;
        }
    }

    pub(crate) fn close_conversation_inspectors(&mut self) {
        self.close_image();
        self.close_command();
        self.close_diff();
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        let Some(watch) = pane.watch else { return };
        let tools = std::mem::take(&mut pane.tools);
        if !tools.is_empty() {
            pane.revision += 1;
        }
        for (key, open) in tools {
            self.write_request(
                None,
                Request::InspectTool {
                    watch,
                    key,
                    operation: open.operation,
                    open: false,
                },
            );
        }
    }

    fn toggle_tool(&mut self, watch: u64, key: &str) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        if self.pending || pane.watch != Some(watch) {
            return;
        }
        let prior = pane.tools.remove(key);
        let opening = prior.is_none();
        let operation = match prior {
            Some(prior) => prior.operation,
            None => {
                let operation = Uuid::new_v4().to_string();
                pane.tools.insert(
                    key.into(),
                    Open {
                        operation: operation.clone(),
                        detail: Detail {
                            message: Some("Loading output…".into()),
                            ..Default::default()
                        },
                    },
                );
                operation
            }
        };
        pane.revision += 1;
        self.write_request(
            None,
            Request::InspectTool {
                watch,
                key: key.into(),
                operation,
                open: opening,
            },
        );
    }
}

pub(super) fn row_view(
    app: &Desktop,
    item: &MessageRow,
    tool: &Tool,
    caption: &str,
    theme: &tok::Theme,
    kit: &Kit,
) -> El {
    let pane = app.workspace.conversation.as_ref().unwrap();
    let open = pane.tools.get(&item.key);
    let glyph = match tool.action.as_str() {
        "read" => &glyphs::FILE_TEXT,
        "search" | "code-search" => &glyphs::SEARCH,
        _ => &glyphs::FILE_CODE,
    };
    let mut button = disclosure(
        &item.key,
        caption,
        glyph,
        if item.failed {
            kit.c.danger.icon
        } else {
            theme.text_secondary
        },
        open.is_some(),
        theme,
    );
    if let Some(watch) = pane.watch
        && !app.pending
    {
        let key = item.key.clone();
        button = button
            .focusable(true)
            .on_press(move |app: &mut Desktop| app.toggle_tool(watch, &key));
    } else {
        button = button.disabled(true);
    }
    let mut row = col().w_full().pb(8.).child(button);
    if let Some(open) = open {
        let mut content = col()
            .key(format!("tool-body-{}", item.key))
            .w_full()
            .py(4.)
            .gap(4.)
            .max_h(320.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        for key in &open.detail.images {
            content = content.child(super::images::thumbnail(
                app,
                key,
                "Tool output image",
                theme,
                256.,
            ));
        }
        for block in &open.detail.blocks {
            if let Some(title) = &block.title {
                let mut heading = label(title, 12., 500., theme.text_secondary).wrap();
                if let Some(href) = block.href.clone() {
                    heading = heading
                        .role("link")
                        .label(title)
                        .focusable(true)
                        .on_press_tx(move |_: &mut Desktop, tx| {
                            let _ = tx.open_url(&href);
                        });
                }
                content = content.child(heading);
            }
            if block.text.is_empty() {
                continue;
            }
            let value = if let Some(language) = &block.language {
                let tokens = syntax::highlight(Some(language), &block.text);
                rich(
                    block.text.clone(),
                    syntax::SyntaxTheme::from_kit(kit).spans(&tokens, 0),
                )
            } else {
                text(&block.text)
            };
            content = content.child(
                value
                    .font(TextStyle::new(12., 400.).mono().lh(18.))
                    .wrap_anywhere()
                    .selectable(true)
                    .color(theme.text),
            );
        }
        if let Some(msg) = &open.detail.message {
            content = content.child(label(msg, 12., 400., theme.text_secondary).wrap());
        }
        row = row.child(content);
    }
    row
}
