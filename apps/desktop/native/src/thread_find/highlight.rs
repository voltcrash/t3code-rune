use super::{Desktop, El, MessageRow};
use rune_kit::Markdown;

fn fold(c: char) -> char {
    match c {
        '‘' | '’' => '\'',
        'ς' => 'σ',
        _ => c.to_lowercase().next().unwrap_or(c),
    }
}

// Fold the same characters as the server while keeping offsets into the original UTF-8 text.
struct Folded {
    text: String,
    offsets: Vec<(usize, usize)>,
}

impl Folded {
    fn new(text: &str) -> Self {
        let mut folded = String::with_capacity(text.len());
        let mut offsets = Vec::new();
        for (at, c) in text.char_indices() {
            let lower = fold(c);
            folded.push(lower);
            if c.len_utf8() != lower.len_utf8() {
                offsets.push((folded.len(), at + c.len_utf8()));
            }
        }
        Self {
            text: folded,
            offsets,
        }
    }

    fn original(&self, offset: usize) -> usize {
        let n = self
            .offsets
            .partition_point(|&(folded, _)| folded <= offset);
        match n.checked_sub(1).map(|i| self.offsets[i]) {
            Some((folded, original)) => original + (offset - folded),
            None => offset,
        }
    }

    fn matches<'a>(&'a self, needle: &'a str) -> impl Iterator<Item = std::ops::Range<usize>> + 'a {
        self.text
            .match_indices(needle)
            .map(|(start, value)| self.original(start)..self.original(start + value.len()))
    }
}

fn paint(s: &Desktop, row: &MessageRow) -> Option<(String, Option<usize>)> {
    let search = s.workspace.conversation.as_ref()?.find.as_ref()?;
    if search.query().is_empty() || !search.result.as_ref().is_some_and(|r| r.total_matches > 0) {
        return None;
    }
    Some((
        search.query().chars().map(fold).collect(),
        search.occurrence(row),
    ))
}

// A repeated character in a large message must not create thousands of paint spans.
const MARKS_PER_ROW: usize = 128;

pub(in super::super) fn plain(s: &Desktop, row: &MessageRow, source: &str, element: El) -> El {
    let Some((needle, active)) = paint(s, row) else {
        return element;
    };
    let folded = Folded::new(source);
    let through = active.map_or(MARKS_PER_ROW, |n| MARKS_PER_ROW.max(n.saturating_add(1)));
    element.text_highlights(
        folded
            .matches(&needle)
            .take(through)
            .enumerate()
            .filter_map(|(i, range)| {
                if Some(i) == active {
                    Some(
                        rune::TextHighlight::new(range, rune::oklch(0.78, 0.15, 80.).alpha(0.65))
                            .key("thread-find-current"),
                    )
                } else if i < MARKS_PER_ROW {
                    Some(rune::TextHighlight::new(
                        range,
                        rune::oklch(0.85, 0.14, 95.).alpha(0.35),
                    ))
                } else {
                    None
                }
            }),
    )
}

pub(in super::super) fn markdown(s: &Desktop, row: &MessageRow, src: &str) -> Markdown {
    let md = Markdown::new(src);
    let Some((needle, active)) = paint(s, row) else {
        return md;
    };
    let mut index = if row.plan.is_some() {
        Folded::new(row.caption.as_deref().unwrap_or("Proposed plan"))
            .matches(&needle)
            .count()
    } else {
        0
    };
    let through = active.map_or(MARKS_PER_ROW, |n| MARKS_PER_ROW.max(n.saturating_add(1)));
    let mut highlights = Vec::new();
    let rendered = rune_kit::markdown::Rendered::new(src);
    for (block, text) in rendered.texts().enumerate() {
        if index >= through {
            break;
        }
        for range in Folded::new(text).matches(&needle).take(through - index) {
            let mark = if Some(index) == active {
                Some(rune_kit::markdown::Highlight::current().key("thread-find-current"))
            } else if index < MARKS_PER_ROW {
                Some(rune_kit::markdown::Highlight::new())
            } else {
                None
            };
            if let Some(mark) = mark {
                highlights.push((rendered.source_ranges(src, block, range), mark));
            }
            index += 1;
        }
    }
    md.highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_keeps_original_byte_offsets_for_curly_quotes_and_dotted_i() {
        let source = "İt’s Σς. IT'S";
        let matches: Vec<_> = Folded::new(source)
            .matches("it's")
            .map(|r| &source[r])
            .collect();
        assert_eq!(matches, ["İt’s", "IT'S"]);
        assert_eq!(
            &source[Folded::new(source).matches("σσ").next().unwrap()],
            "Σς"
        );
    }
}
