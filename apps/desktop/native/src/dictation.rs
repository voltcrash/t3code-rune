use super::{Desktop, Request, appearance, attachments::Target};
use rune::{El, KeyEvent, Tx, el, icon, row, tok};
use rune_kit::glyphs;
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize)]
pub(crate) struct Choice {
    pub available: bool,
    pub enabled: bool,
    pub destination: String,
}

#[derive(Deserialize)]
pub(crate) struct Labels {
    pub destination: String,
    pub off: String,
    pub start: String,
    pub finish: String,
    pub cancel: String,
    pub transcribing: String,
    pub starting: String,
}

#[derive(Serialize)]
pub(crate) struct Snapshot<'a> {
    text: &'a str,
    selection: Utf16Range,
}
#[derive(Serialize)]
struct Utf16Range {
    start: usize,
    end: usize,
}

#[derive(Deserialize)]
pub(crate) struct Edit {
    start: usize,
    end: usize,
    text: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Phase {
    Preparing,
    Recording,
    Transcribing,
    Transcript,
    Error,
}

struct Take {
    target: Target,
    watch: u64,
    operation: u64,
    original: String,
    submit: bool,
    seconds: usize,
    levels: [u8; 24],
    phase: Phase,
}

#[derive(Default)]
pub(crate) struct Dictation {
    pub choice: Choice,
    take: Option<Take>,
}

impl Desktop {
    fn dictation_target(&self) -> Option<Target> {
        if self.settings_open || self.pending || !self.can_operate {
            return None;
        }
        let (draft, id) = self.details_target()?;
        let target = if draft {
            self.new_threads.prompt(id)?;
            Target::Project(id.to_owned())
        } else {
            Target::Thread(id.to_owned())
        };
        target.active(self).then_some(target)
    }

    pub(crate) fn dictating(&self, target: &Target) -> bool {
        self.dictation
            .take
            .as_ref()
            .is_some_and(|take| &take.target == target)
    }

    pub(crate) fn reconcile_dictation(&mut self) -> bool {
        let stale = self.dictation.take.as_ref().is_some_and(|take| {
            Some(&take.target) != self.dictation_target().as_ref()
                || self.workspace.feed != Some(take.watch)
                || !self.dictation.choice.enabled
        });
        if stale {
            self.cancel_dictation();
        }
        stale
    }

    fn cancel_dictation(&mut self) {
        if let Some(take) = self.dictation.take.take() {
            self.write_request(
                None,
                Request::Dictate {
                    watch: take.watch,
                    operation: take.operation,
                    action: "cancel",
                    draft: None,
                },
            );
        }
    }
    pub(crate) fn finish_dictation(&mut self, submit: bool) -> bool {
        let Some(take) = self.dictation.take.as_mut() else {
            return false;
        };
        take.submit |= submit;
        if take.phase == Phase::Transcribing {
            return true;
        }
        let (watch, operation) = (take.watch, take.operation);
        self.write_request(
            None,
            Request::Dictate {
                watch,
                operation,
                action: "stop",
                draft: None,
            },
        );
        true
    }

    fn toggle_dictation(&mut self, tx: &mut Tx) {
        if self.finish_dictation(false) {
            return;
        }
        let Some(target) = self.dictation_target() else {
            return;
        };
        let Some(watch) = self.workspace.feed else {
            return;
        };
        if !self.dictation.choice.available {
            return;
        }
        if !self.dictation.choice.enabled {
            self.settings_page = appearance::SettingsPage::Voice;
            self.open_settings(tx);
            tx.focus_when_shown("dictation-choice", true);
            return;
        }
        let operation = self.next_request_id();
        let edit = match &target {
            Target::Project(id) => self.new_threads.prompt(id),
            Target::Thread(id) => self.workspace.drafts.get(id).map(|draft| &draft.editor),
        };
        let Some(edit) = edit else {
            return;
        };
        if edit.is_composing() {
            return;
        }
        let original = edit.text.clone();
        let range = edit.range();
        let draft = Snapshot {
            text: &original,
            selection: Utf16Range {
                start: original[..range.0].encode_utf16().count(),
                end: original[..range.1].encode_utf16().count(),
            },
        };
        if self.write_request(
            None,
            Request::Dictate {
                watch,
                operation,
                action: "start",
                draft: Some(draft),
            },
        ) {
            tx.focus_when_shown(target.field_key(), false);
            self.dictation.take = Some(Take {
                target,
                operation,
                watch,
                original,
                submit: false,
                seconds: 0,
                levels: [0; 24],
                phase: Phase::Preparing,
            });
        }
    }

    pub(crate) fn dictation_reply(
        &mut self,
        watch: u64,
        operation: u64,
        phase: Phase,
        meter: (usize, usize),
        message: Option<String>,
        edit: Option<Edit>,
    ) {
        if self.reconcile_dictation() {
            return;
        }
        let Some(take) = self
            .dictation
            .take
            .as_mut()
            .filter(|t| t.watch == watch && t.operation == operation)
        else {
            return;
        };
        if phase == Phase::Error {
            self.error = message;
            self.dictation.take = None;
        } else if phase == Phase::Transcript {
            let mut take = self.dictation.take.take().unwrap();
            let Some(patch) = edit else {
                return;
            };
            let mut changed = None;
            let mut inserted = false;
            take.target.edit(self, |_, edit| {
                if edit.text != take.original {
                    changed = Some(edit.text.clone());
                } else if patch.start <= patch.end
                    && edit.text.is_char_boundary(patch.start)
                    && edit.text.is_char_boundary(patch.end)
                {
                    edit.undo_group(|e| {
                        e.set_selection(patch.start, patch.end);
                        e.paste(&patch.text);
                    });
                    inserted = true;
                }
            });
            if let Some(current) = changed {
                take.original = current;
                let draft = Snapshot {
                    text: &take.original,
                    selection: Utf16Range { start: 0, end: 0 },
                };
                if self.write_request(
                    None,
                    Request::Dictate {
                        watch,
                        operation,
                        action: "insert",
                        draft: Some(draft),
                    },
                ) {
                    self.dictation.take = Some(take);
                }
                return;
            }
            self.write_request(
                None,
                Request::Dictate {
                    watch,
                    operation,
                    action: "cancel",
                    draft: None,
                },
            );
            if take.submit && inserted {
                match take.target {
                    Target::Thread(_) => self.send_message(),
                    Target::Project(_) => self.launch(),
                }
            }
        } else {
            take.phase = phase;
            take.seconds = meter.0;
            if take.levels.last().copied() != Some(meter.1.min(100) as u8) {
                take.levels.rotate_left(1);
                take.levels[23] = meter.1.min(100) as u8;
            }
        }
    }

    pub(crate) fn dictation_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if let Some(take) = &self.dictation.take
            && tx.focused() == Some(take.target.field_key())
            && event.key == rune::Key::Enter
            && event.mods == rune::Mods::default()
        {
            return self.finish_dictation(true);
        }
        if self.dictation.take.is_some() && event.key == rune::Key::Escape {
            self.cancel_dictation();
            return true;
        }
        if event.mods.shift
            && !event.mods.alt
            && event.is_command('d')
            && self.dictation.choice.available
            && tx.focused() != Some("thread-terminal-grid")
            && self.dictation_target().is_some()
        {
            if !tx.key_repeat() {
                self.toggle_dictation(tx);
            }
            return true;
        }
        false
    }

    pub(crate) fn dictation_button(&self, target: &Target, theme: &tok::Theme) -> Option<El> {
        let labels = &self.settings_labels.as_ref()?.voice.dictation;
        if !self.dictation.choice.available || self.pending {
            return None;
        }
        let take = self
            .dictation
            .take
            .as_ref()
            .filter(|take| &take.target == target);
        let button = |key: &str, name: &str, glyph: &'static rune::Icon| {
            el().key(key)
                .role("button")
                .label(name)
                .help(name)
                .keeps_text_selection()
                .size(28., 28.)
                .center()
                .radius(14.)
                .hover(theme.separator)
                .focusable(true)
                .child(icon(glyph, 16.).color(theme.text_secondary))
        };
        let Some(take) = take else {
            return Some(
                button("dictation-start", &labels.start, &appearance::voice::MIC)
                    .on_press_tx(|s: &mut Desktop, tx| s.toggle_dictation(tx)),
            );
        };
        let mut pill = row()
            .center_y()
            .shrink()
            .min_w(0.)
            .radius(15.)
            .gap(6.)
            .h(32.)
            .px(2.)
            .border(1., theme.card_border)
            .fill(theme.separator.alpha(0.48))
            .child(
                button("dictation-cancel", &labels.cancel, &glyphs::CLOSE)
                    .on_press(|s: &mut Desktop| s.cancel_dictation()),
            );
        if take.phase == Phase::Recording {
            let seconds = take.seconds;
            let bars = take.levels.iter().map(|level| {
                let scale = f32::from(*level) / 100.;
                el().w(2.)
                    .h(16. * scale.max(0.2))
                    .radius(1.)
                    .fill(theme.text.alpha(0.3 + scale * 0.55))
            });
            pill = pill
                .child(
                    el().size(6., 6.)
                        .radius(3.)
                        .fill(rune::color::hex("#ef4444").unwrap()),
                )
                .child(super::workspace::label(
                    &format!("{}:{:02} / 5:00", seconds / 60, seconds % 60),
                    12.,
                    400.,
                    theme.text_secondary,
                ))
                .child(
                    row()
                        .center_y()
                        .h(16.)
                        .w(64.)
                        .shrink()
                        .justify_end()
                        .clip_rounded(0.)
                        .gap(2.)
                        .children(bars),
                )
                .child(
                    button("dictation-finish", &labels.finish, &glyphs::CHECK).on_press(
                        |s: &mut Desktop| {
                            s.finish_dictation(false);
                        },
                    ),
                );
        } else {
            let status = if take.phase == Phase::Preparing {
                &labels.starting
            } else {
                &labels.transcribing
            };
            pill = pill.child(
                super::workspace::label(status, 12., 400., theme.text_secondary)
                    .truncate()
                    .pr(8.)
                    .role("status"),
            );
        }
        Some(pill)
    }
}

pub(super) fn recording_field(field: El) -> El {
    field
        .label("Message")
        .focusable(true)
        .read_only(true)
        .on_submit(|s: &mut Desktop, _| {
            s.finish_dictation(true);
        })
}
