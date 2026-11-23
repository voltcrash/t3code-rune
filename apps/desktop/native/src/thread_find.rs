use super::{
    Conversation, Desktop, El, MessageRow, Request, TextEdit, Tx, icon, label, row, text_input, tok,
};
use rune::{Key, TextStyle};
use rune_kit::glyphs;
#[path = "thread_find/highlight.rs"]
mod highlight;
pub(super) use highlight::{markdown, plain};
use serde::Deserialize;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub(super) title: String,
    next: String,
    previous: String,
    close: String,
    searching: String,
    failed: String,
    retry: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultPage {
    #[serde(default)]
    navigation: Vec<Neighbor>,
    active_index: usize,
    total_matches: usize,
    complete: Option<bool>,
    #[serde(rename = "match")]
    selected: Option<Match>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Neighbor {
    entry_id: String,
    start_index: usize,
    count: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Match {
    entry_id: String,
    occurrence: usize,
}

#[derive(Default)]
pub(super) struct Search {
    input: TextEdit,
    dark: Cell<bool>,
    due: Option<Instant>,
    requested: Option<(u64, u64)>,
    requested_sequence: Option<u64>,
    requested_dark: bool,
    index: usize,
    result: Option<ResultPage>,
    failed: bool,
    reveal: bool,
    history_cursor: Option<String>,
    return_focus: Option<String>,
}

impl Search {
    fn query(&self) -> &str {
        self.input.text.trim()
    }

    fn can_step(&self) -> bool {
        self.result
            .as_ref()
            .is_some_and(|p| p.complete != Some(false) && p.total_matches > 0)
    }

    fn occurrence(&self, row: &MessageRow) -> Option<usize> {
        let m = self.result.as_ref()?.selected.as_ref()?;
        (row.entry_id.as_deref() == Some(&m.entry_id)).then_some(m.occurrence)
    }
}

impl Desktop {
    pub(crate) fn can_find_thread(&self) -> bool {
        self.workspace.find_labels.is_some()
            && !self.history.on_issues()
            && matches!(self.details_target(), Some((false, _)))
    }

    pub(crate) fn open_thread_find(&mut self, tx: &mut Tx) {
        if !self.can_find_thread() {
            return;
        }
        let pane = self.workspace.conversation.as_mut().unwrap();
        let find = pane.find.get_or_insert_with(|| Search {
            return_focus: tx.focused().map(str::to_owned),
            ..Default::default()
        });
        find.input.set_selection(0, find.input.text.len());
        tx.focus_when_shown("thread-find-query", true);
    }

    fn cancel_thread_search(&mut self, request: Option<(u64, u64)>) {
        if let Some((watch, operation)) = request {
            self.write_request(
                None,
                Request::FindThread {
                    watch,
                    operation,
                    index: 0,
                    query: String::new(),
                    dark: false,
                    start: None,
                },
            );
        }
    }

    pub(crate) fn close_thread_find(&mut self, tx: &mut Tx) -> bool {
        let Some(pane) = &mut self.workspace.conversation else {
            return false;
        };
        let Some(search) = pane.find.take() else {
            return false;
        };
        pane.revision += 1;
        self.cancel_thread_search(search.requested);
        if self.can_find_thread() {
            tx.focus(
                search.return_focus.as_deref().unwrap_or("message-input"),
                true,
            );
        }
        true
    }

    fn schedule_find(&mut self, index: usize, delay: Duration, tx: &mut Tx) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        let Some(search) = &mut pane.find else { return };
        let mut units = 0;
        if let Some((limit, _)) = search.input.text.char_indices().find(|(_, ch)| {
            units += ch.len_utf16();
            units > 200
        }) {
            search.input.text.truncate(limit);
        }
        let cancel = search.requested.take();
        search.index = index;
        search.failed = false;
        search.result = None;
        search.reveal = false;
        search.history_cursor = None;
        search.due = (!search.query().is_empty()).then(|| Instant::now() + delay);
        pane.revision += 1;
        self.cancel_thread_search(cancel);
        tx.wake_after(delay);
    }

    fn step_find(&mut self, previous: bool, tx: &mut Tx) {
        let Some(search) = self
            .workspace
            .conversation
            .as_mut()
            .and_then(|p| p.find.as_mut())
        else {
            return;
        };
        if !search.can_step() {
            return;
        }
        let result = search.result.as_mut().unwrap();
        let total = result.total_matches;
        let index = (result.active_index + if previous { total - 1 } else { 1 }) % total;
        if let Some(neighbor) = result
            .navigation
            .iter()
            .find(|n| n.start_index <= index && index - n.start_index < n.count)
        {
            result.selected = Some(Match {
                entry_id: neighbor.entry_id.clone(),
                occurrence: index - neighbor.start_index,
            });
            result.active_index = index;
            search.index = index;
            search.reveal = true;
            search.history_cursor = None;
            return;
        }
        self.schedule_find(index, Duration::ZERO, tx);
    }

    pub(crate) fn receive_thread_find(
        &mut self,
        watch: u64,
        operation: u64,
        result: Option<ResultPage>,
    ) {
        let Some(pane) = &mut self.workspace.conversation else {
            return;
        };
        let Some(search) = &mut pane.find else { return };
        if pane.watch != Some(watch) || search.requested != Some((watch, operation)) {
            return;
        }
        search.failed = result.is_none();
        let previous = search.result.as_ref().and_then(|p| p.selected.as_ref());
        let next = result.as_ref().and_then(|p| p.selected.as_ref());
        search.reveal |= next.is_some() && previous != next;
        search.result = result;
        pane.revision += 1;
    }

    pub(crate) fn sync_thread_find(&mut self, tx: &mut Tx) -> bool {
        if self
            .workspace
            .conversation
            .as_ref()
            .is_none_or(|p| p.find.is_none())
        {
            return false;
        }
        if !self.can_find_thread() {
            return self.close_thread_find(tx);
        }
        let pane = self.workspace.conversation.as_mut().unwrap();
        let find = pane.find.as_mut().unwrap();
        if self.pending || find.query().is_empty() {
            return false;
        }
        let Some(watch) = pane.watch else {
            return false;
        };
        if find.reveal
            && let Some(m) = find.result.as_ref().and_then(|r| r.selected.as_ref())
        {
            if let Some(index) = pane
                .rows
                .iter()
                .position(|row| row.entry_id.as_deref() == Some(&m.entry_id))
            {
                pane.folds.reveal(index, pane.rows.len());
                let row = &pane.rows[index];
                if row.plan.is_some() {
                    pane.expanded_activities.insert(row.key.clone());
                }
                tx.scroll_center_in("message-rows", &row.key);
                tx.scroll_center_in("message-rows", format!("{}/thread-find-current", row.key));
                find.reveal = false;
                pane.revision += 1;
                return true;
            }
            if pane.cursor.is_some() && pane.cursor != find.history_cursor {
                find.history_cursor.clone_from(&pane.cursor);
                self.older();
                return true;
            }
            find.reveal = false;
        }
        if find.due.is_none()
            && (find.requested.is_none_or(|(id, _)| id != watch)
                || (!find.failed
                    && find
                        .result
                        .as_ref()
                        .is_some_and(|r| r.complete != Some(false))
                    && (find.requested_sequence != pane.sequence
                        || find.requested_dark != find.dark.get())))
        {
            let delay = Duration::from_millis(450);
            find.due = Some(Instant::now() + delay);
            tx.wake_after(delay);
        }
        let Some(due) = find.due else { return false };
        if let Some(remaining) = due.checked_duration_since(Instant::now()) {
            tx.wake_after(remaining);
            return false;
        }
        let index = find.index;
        let dark = find.dark.get();
        let query = find.query().to_owned();
        let start = find.result.as_ref().and_then(|r| r.selected.clone());
        let sequence = pane.sequence;
        let operation = self.next_request_id();
        let search = self
            .workspace
            .conversation
            .as_mut()
            .unwrap()
            .find
            .as_mut()
            .unwrap();
        search.requested = Some((watch, operation));
        search.requested_sequence = sequence;
        search.requested_dark = dark;
        search.due = None;
        self.write_request(
            None,
            Request::FindThread {
                watch,
                operation,
                query,
                index,
                start,
                dark,
            },
        );
        true
    }
}

pub(super) const BAR_HEIGHT: f32 = 48.;

pub(super) fn bar(s: &Desktop, pane: &Conversation, theme: &tok::Theme, dark: bool) -> Option<El> {
    let search = pane.find.as_ref()?;
    let labels = s.workspace.find_labels.as_ref()?;
    search.dark.set(dark);
    let field = text_input(&search.input, TextStyle::new(13., 400.))
        .key("thread-find-query")
        .label(&labels.title)
        .placeholder(&labels.title, theme.text_secondary)
        .single_line()
        .min_w(0.)
        .grow(1.)
        .color(theme.text)
        .on_edit(|s: &mut Desktop| {
            &mut s
                .workspace
                .conversation
                .as_mut()
                .unwrap()
                .find
                .as_mut()
                .unwrap()
                .input
        })
        .on_input_change(|s: &mut Desktop, tx| s.schedule_find(0, Duration::from_millis(150), tx))
        .on_input_key(|s: &mut Desktop, e, tx| {
            if e.key == Key::Escape {
                return s.close_thread_find(tx);
            }
            if e.key == Key::Enter {
                s.step_find(e.mods.shift, tx);
                true
            } else {
                false
            }
        });
    let count = if search.query().is_empty() {
        String::new()
    } else if search.failed {
        labels.failed.clone()
    } else if let Some(r) = &search.result {
        let active = if r.total_matches == 0 {
            0
        } else {
            r.active_index + 1
        };
        if r.complete == Some(false) {
            format!("{active}/…")
        } else {
            format!("{active}/{}", r.total_matches)
        }
    } else {
        labels.searching.clone()
    };
    let mut bar = row()
        .key("thread-find")
        .role("search")
        .label(&labels.title)
        .grow(1.)
        .max_w(super::thread_details::WIDTH)
        .min_w(0.)
        .h(36.)
        .px(8.)
        .gap(4.)
        .center_y()
        .radius(8.)
        .fill(theme.card)
        .border(1., theme.card_border)
        .child(field)
        .child(label(&count, 12., 400., theme.text_secondary).role("status"));
    if search.failed {
        bar = bar.child(super::action_with_tx(
            &labels.retry,
            theme,
            true,
            |s, tx| s.schedule_find(0, Duration::ZERO, tx),
        ));
    }
    let button =
        |name: &str, glyph: &'static rune::Icon, enabled: bool, run: fn(&mut Desktop, &mut Tx)| {
            let button = rune::el()
                .key(name)
                .label(name)
                .help(name)
                .role("button")
                .size(24., 24.)
                .center()
                .radius(4.)
                .child(icon(glyph, 14.).color(theme.text_secondary));
            if enabled {
                button.focusable(true).hover(theme.hover).on_press_tx(run)
            } else {
                button.disabled(true).opacity(0.5)
            }
        };
    bar = bar
        .child(button(
            &labels.previous,
            &glyphs::CHEVRON_UP,
            search.can_step(),
            |s, tx| s.step_find(true, tx),
        ))
        .child(button(
            &labels.next,
            &glyphs::CHEVRON_DOWN,
            search.can_step(),
            |s, tx| s.step_find(false, tx),
        ))
        .child(button(&labels.close, &glyphs::CLOSE, true, |s, tx| {
            s.close_thread_find(tx);
        }));
    Some(
        row()
            .w_full()
            .px(12.)
            .pt(BAR_HEIGHT - 36.)
            .justify_end()
            .child(bar),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_old_search_cannot_replace_the_result_after_typing_or_reconnect() {
        let mut app = Desktop::new();
        app.open_thread("thread");
        let pane = app.workspace.conversation.as_mut().unwrap();
        pane.begin_watch(8);
        pane.find = Some(Search {
            requested: Some((8, 22)),
            ..Default::default()
        });
        let result = || {
            serde_json::from_value(serde_json::json!({
                "activeIndex": 1, "totalMatches": 2, "match": {"entryId":"earlier", "occurrence":1},
                "navigation":[{"entryId":"earlier","startIndex":0,"count":2}]
            }))
            .unwrap()
        };
        app.receive_thread_find(8, 22, Some(result()));
        app.receive_thread_find(7, 22, None);
        app.receive_thread_find(8, 21, None);
        let search = app
            .workspace
            .conversation
            .as_ref()
            .unwrap()
            .find
            .as_ref()
            .unwrap();
        assert!(!search.failed && search.reveal && search.can_step());
        assert_eq!(search.result.as_ref().unwrap().active_index, 1);
    }
}
