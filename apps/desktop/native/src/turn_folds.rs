use super::{Desktop, label};
use rune::{El, col, el, row, tok};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub(crate) struct Fold {
    id: String,
    label: String,
    at: usize,
    ranges: Vec<[usize; 2]>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum VisibleRow {
    Message(usize),
    Header(usize),
}

#[derive(Default)]
pub(super) struct Layout {
    folds: Vec<Fold>,
    open: HashSet<String>,
    visible: Vec<VisibleRow>,
}

impl Layout {
    pub(super) fn update(&mut self, count: usize, folds: Option<Vec<Fold>>) {
        if let Some(folds) = folds {
            self.folds = folds;
            if !self.open.is_empty() {
                let available: HashSet<_> = self.folds.iter().map(|fold| &fold.id).collect();
                self.open.retain(|id| available.contains(id));
            }
        }
        self.visible.clear();
        if self.folds.is_empty() {
            return;
        }
        let mut hidden = vec![false; count];
        let mut headers = Vec::with_capacity(self.folds.len());
        for (group, fold) in self.folds.iter().enumerate() {
            headers.push((fold.at.min(count), group));
            if !self.open.contains(&fold.id) {
                for &[first, end] in &fold.ranges {
                    if let Some(slice) = hidden.get_mut(first..end) {
                        slice.fill(true);
                    }
                }
            }
        }
        headers.sort_unstable();
        let mut headers = headers.into_iter().peekable();
        for (index, hidden) in hidden.into_iter().enumerate() {
            while let Some((_, group)) = headers.next_if(|(at, _)| *at == index) {
                self.visible.push(VisibleRow::Header(group));
            }
            if !hidden {
                self.visible.push(VisibleRow::Message(index));
            }
        }
        self.visible
            .extend(headers.map(|(_, group)| VisibleRow::Header(group)));
    }

    pub(super) fn resize(&mut self, old: usize, new: usize, folds: Option<Vec<Fold>>) {
        if folds.is_some() {
            self.update(new, folds);
        } else if !self.folds.is_empty() {
            if new < old {
                self.visible
                    .truncate(self.visible.len().saturating_sub(old - new));
            } else {
                self.visible.extend((old..new).map(VisibleRow::Message));
            }
        }
    }

    pub(super) fn len(&self, count: usize) -> usize {
        if self.folds.is_empty() {
            count
        } else {
            self.visible.len()
        }
    }

    pub(super) fn row(&self, index: usize) -> VisibleRow {
        if self.folds.is_empty() {
            VisibleRow::Message(index)
        } else {
            self.visible[index]
        }
    }
    pub(super) fn reveal(&mut self, index: usize, count: usize) {
        let mut changed = false;
        for fold in &self.folds {
            if fold
                .ranges
                .iter()
                .any(|&[first, end]| (first..end).contains(&index))
            {
                changed |= self.open.insert(fold.id.clone());
            }
        }
        if changed {
            self.update(count, None);
        }
    }

    pub(super) fn key(&self, group: usize) -> &str {
        &self.folds[group].id
    }

    pub(super) fn header(&self, group: usize, thread: &str, theme: &tok::Theme) -> El {
        let fold = &self.folds[group];
        let open = self.open.contains(&fold.id);
        let key = fold.id.clone();
        let thread = thread.to_owned();
        let button = row()
            .key(format!("turn-fold-{key}"))
            .label(&fold.label)
            .role("button")
            .expanded(open)
            .focusable(true)
            .center_y()
            .px(4.)
            .gap(4.)
            .min_h(24.)
            .radius(6.)
            .hover(theme.hover)
            .child(label(&fold.label, 14., 400., theme.text_secondary))
            .child(crate::disclosure_chevron(open, 14.).color(theme.text_secondary))
            .on_press(move |s: &mut Desktop| {
                if let Some(pane) = &mut s.workspace.conversation
                    && pane.id == thread
                {
                    if !pane.folds.open.remove(&key) {
                        pane.folds.open.insert(key.clone());
                    }
                    pane.folds.update(pane.rows.len(), None);
                    pane.revision += 1;
                }
            });
        col()
            .w_full()
            .pt(4.)
            .gap(8.)
            .pb(4.)
            .child(row().child(button))
            .child(
                el().w_full()
                    .h(1.)
                    .fill(theme.separator.alpha(theme.separator.a * 0.6)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Fold, Layout,
        VisibleRow::{Header, Message},
    };

    #[test]
    fn preserved_cards_keep_their_positions_as_a_fold_opens_or_history_grows() {
        let mut layout = Layout::default();
        layout.update(
            6,
            Some(vec![Fold {
                id: "run".into(),
                label: "Worked".into(),
                at: 0,
                ranges: vec![[1, 3], [4, 5]],
            }]),
        );
        assert_eq!(
            layout.visible,
            [Header(0), Message(0), Message(3), Message(5)]
        );
        layout.resize(6, 8, None);
        assert_eq!(layout.row(5), Message(7));
        layout.resize(8, 6, None);
        layout.open.insert("run".into());
        layout.update(6, None);
        assert_eq!(
            layout.visible,
            [
                Header(0),
                Message(0),
                Message(1),
                Message(2),
                Message(3),
                Message(4),
                Message(5)
            ]
        );
        layout.update(6, Some(vec![]));
        assert_eq!(layout.len(6), 6);
        assert_eq!(layout.row(1), Message(1));
    }
}
