use super::{Desktop, MessageRow, label};
use rune::{El, col, row, tok};
use rune_kit::{Kit, Markdown, MarkdownStyle};
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
pub(super) struct Plan {
    start: usize,
    preview_end: Option<usize>,
    collapsible: bool,
}

// ECMAScript trimEnd includes the BOM and preserves NEXT LINE in Markdown.
fn trim_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

impl Plan {
    fn markdown<'a>(&self, source: &'a str, closed: bool, title: Option<&str>) -> Cow<'a, str> {
        let body = trim_end(source.get(self.start..).unwrap_or(source));
        if closed {
            let preview = trim_end(
                self.preview_end
                    .and_then(|end| body.get(..end))
                    .unwrap_or(body),
            );
            if preview.is_empty() {
                return Cow::Owned(title.unwrap_or("Plan preview unavailable.").into());
            }
            if self.preview_end.is_some()
                || preview
                    .lines()
                    .any(|line| line.len() != trim_end(line).len())
            {
                let mut lines = preview.lines().map(trim_end).collect::<Vec<_>>().join("\n");
                if self.preview_end.is_some() {
                    lines.push_str("\n\n...");
                }
                return Cow::Owned(lines);
            }
        }
        Cow::Borrowed(body)
    }

    pub(super) fn card(
        &self,
        s: &Desktop,
        item: &MessageRow,
        theme: &tok::Theme,
        kit: &Kit,
        style: &MarkdownStyle,
    ) -> El {
        let pane = s.workspace.conversation.as_ref().unwrap();
        let closed = self.collapsible && !pane.expanded_activities.contains(&item.key);
        let title = item.caption.as_deref().unwrap_or("Proposed plan");
        let header = row()
            .w_full()
            .center_y()
            .gap(8.)
            .child(
                label("Plan", 12., 500., theme.text)
                    .px(8.)
                    .py(2.)
                    .radius(6.)
                    .fill(theme.text.alpha(0.08)),
            )
            .child(
                label(title, 14., 500., theme.text)
                    .truncate()
                    .min_w(0.)
                    .grow(1.),
            );
        let body = Markdown::new(
            self.markdown(&item.body, closed, item.caption.as_deref())
                .as_ref(),
        )
        .selectable(true)
        .style(style.clone())
        .build(kit);
        let mut card = col()
            .w_full()
            .px(20.)
            .py(20.)
            .radius(16.)
            .border(1., theme.separator)
            .fill(theme.text.alpha(0.035))
            .gap(16.)
            .child(header)
            .child(if closed {
                col()
                    .w_full()
                    .max_h(416.)
                    .clip_fade_linear(0., 0., 0., 24.)
                    .child(body)
            } else {
                body
            });
        if self.collapsible {
            let key = item.key.clone();
            let watch = pane.watch;
            let name = if closed {
                "Expand plan"
            } else {
                "Collapse plan"
            };
            card = card.child(
                row()
                    .self_center()
                    .label(name)
                    .role("button")
                    .focusable(true)
                    .key(format!("toggle-plan-{key}"))
                    .expanded(!closed)
                    .center()
                    .h(28.)
                    .px(10.)
                    .border(1., theme.separator)
                    .radius(6.)
                    .hover(theme.hover)
                    .child(label(name, 12., 500., theme.text))
                    .on_press(move |s: &mut Desktop| {
                        if let Some(pane) = &mut s.workspace.conversation
                            && pane.watch == watch
                        {
                            if !pane.expanded_activities.remove(&key) {
                                pane.expanded_activities.insert(key.clone());
                            }
                            pane.revision += 1;
                        }
                    }),
            );
        }
        col().w_full().py(10.).child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanding_restores_the_original_markdown_hard_breaks() {
        let header = "# 計画\r\n\r\n";
        let first_lines = "Line one  \r\n第二步  \u{85}\r\nThird.\u{feff}\r\n\r\n";
        let source = format!("{header}{first_lines}Last step.");
        let plan = Plan {
            start: header.len(),
            preview_end: Some(first_lines.len()),
            collapsible: true,
        };
        assert_eq!(
            plan.markdown(&source, true, None),
            "Line one\n第二步  \u{85}\nThird.\n\n..."
        );
        assert_eq!(
            plan.markdown(&source, false, None),
            format!("{first_lines}Last step.")
        );
    }
}
