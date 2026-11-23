use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use std::collections::BTreeSet;

fn fits(text: &str) -> bool {
    text.len() <= 8192 && text.lines().take(129).count() <= 128
}

fn source_rows(mut source: &str, push: &mut impl FnMut(String, bool)) {
    while !source.is_empty() {
        let bound = source.floor_char_boundary(source.len().min(2048));
        let head = &source[..bound];
        let end = head
            .match_indices('\n')
            .nth(63)
            .map(|(n, _)| n + 1)
            .unwrap_or_else(|| {
                if bound < source.len() {
                    head.rfind('\n').map_or(bound, |n| n + 1)
                } else {
                    bound
                }
            });
        let (part, rest) = source.split_at(end);
        push(part.strip_suffix('\n').unwrap_or(part).into(), false);
        source = rest;
    }
}

pub(super) fn partition(text: String, mut push: impl FnMut(String, bool)) {
    if fits(&text) {
        push(text, true);
        return;
    }
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut events = Parser::new_ext(&text, options).into_offset_iter();
    let mut depth = 0;
    let mut start = 0;
    let mut definitions = BTreeSet::new();
    while let Some((event, span)) = events.next() {
        if depth == 0 {
            start = if matches!(event, Event::Start(Tag::CodeBlock(CodeBlockKind::Indented))) {
                text[..span.start]
                    .rfind(['\r', '\n'])
                    .map_or(0, |end| end + 1)
            } else {
                span.start
            };
        }
        match event {
            Event::Start(tag) => {
                depth += 1;
                if let Tag::Link { id, .. } | Tag::Image { id, .. } = tag
                    && !id.is_empty()
                    && let Some(def) = events.reference_definitions().get(&id)
                {
                    definitions.insert((def.span.start, def.span.end));
                }
            }
            Event::End(_) => depth -= 1,
            _ => {}
        }
        if depth != 0 {
            continue;
        }
        let source = &text[start..span.end];
        if fits(source) {
            let mut block = source.to_owned();
            for &(from, to) in &definitions {
                if from < start || to > span.end {
                    if block.len() + to - from + 2 > 8192 {
                        break;
                    }
                    block.push_str("\n\n");
                    block.push_str(&text[from..to]);
                }
            }
            push(block, true);
        } else {
            source_rows(source, &mut push);
        }
        definitions.clear();
    }
}
