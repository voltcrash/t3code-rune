use super::{Desktop, Request, workspace::LiveStatus};
use rune::{El, ScrollSpec, TextEdit, TextStyle, col, el, row, spacer, text, text_input, tok};
use rune_kit::{Kit, Markdown, MarkdownStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Prompt {
    request_id: String,
    pub version: String,
    response_capability: String,
    dismissible: bool,
    questions: Vec<Question>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Question {
    id: String,
    header: String,
    question: String,
    options: Vec<Choice>,
    multi_select: bool,
    allow_custom_answer: Option<bool>,
    initial_answer: Option<String>,
    required: Option<bool>,
}

#[derive(Deserialize, PartialEq)]
struct Choice {
    label: String,
    description: String,
    value: Option<String>,
    preview: Option<String>,
}

impl Choice {
    fn value(&self) -> &str {
        self.value.as_deref().unwrap_or(&self.label)
    }
}

struct Answer {
    edited: bool,
    editor: TextEdit,
    selected: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Submission {
    selected_option_values: Vec<String>,
    custom_answer: String,
}

pub(super) struct Form {
    pub prompt: Prompt,
    answers: Vec<Answer>,
    step: usize,
    collapsed: bool,
}

impl Form {
    fn new(prompt: Prompt) -> Self {
        let answers = prompt
            .questions
            .iter()
            .map(|q| Answer {
                edited: false,
                editor: TextEdit::new(q.initial_answer.as_deref().unwrap_or("")),
                selected: vec![],
            })
            .collect();
        Self {
            prompt,
            answers,
            step: 0,
            collapsed: false,
        }
    }
    fn answered(&self, step: usize) -> bool {
        let q = &self.prompt.questions[step];
        let a = &self.answers[step];
        !a.selected.is_empty()
            || (q.allow_custom_answer != Some(false)
                && (q.initial_answer.is_some() || !a.editor.text.trim().is_empty()))
            || (q.required == Some(false) && q.options.is_empty())
    }
    fn last(&self) -> bool {
        self.step + 1 == self.answers.len()
    }
    fn can_advance(&self) -> bool {
        if self.last() {
            (0..self.answers.len()).all(|i| self.answered(i))
        } else {
            self.answered(self.step)
        }
    }
    fn request(&self, thread: &str, dismiss: bool) -> Request<'static> {
        let drafts = (!dismiss).then(|| {
            self.prompt
                .questions
                .iter()
                .zip(&self.answers)
                .map(|(q, a)| {
                    (
                        q.id.clone(),
                        Submission {
                            selected_option_values: a.selected.clone(),
                            custom_answer: a.editor.text.clone(),
                        },
                    )
                })
                .collect()
        });
        Request::Question {
            thread: thread.into(),
            request: self.prompt.request_id.clone(),
            version: self.prompt.version.clone(),
            operation: Uuid::new_v4().to_string(),
            drafts,
        }
    }
}

pub(super) type Forms = HashMap<String, Form>;

pub(super) fn replace(forms: &mut Forms, thread: &str, prompts: Vec<Prompt>) {
    let Some(prompt) = prompts.into_iter().find(|p| !p.questions.is_empty()) else {
        forms.remove(thread);
        return;
    };
    if let Some(form) = forms.get_mut(thread) {
        if form.prompt.version == prompt.version {
            return;
        }
        if form.prompt.request_id == prompt.request_id && form.prompt.questions == prompt.questions
        {
            form.prompt = prompt;
            return;
        }
    }
    forms.insert(thread.into(), Form::new(prompt));
}

pub(super) struct Advance {
    thread: String,
    version: String,
    step: usize,
    deadline: Instant,
}

impl Desktop {
    pub(super) fn poll_answer(&mut self, tx: &mut rune::Tx) -> bool {
        let Some(next) = &self.answer_advance else {
            return false;
        };
        if self.palette.is_some() || !self.question_enabled(&next.thread, &next.version, next.step)
        {
            self.answer_advance = None;
            return false;
        }
        if let Some(wait) = next.deadline.checked_duration_since(Instant::now()) {
            tx.wake_after(wait);
            return false;
        }
        if tx.input_waiting() {
            tx.wake_after(Duration::ZERO);
            return false;
        }
        let next = self.answer_advance.take().unwrap();
        self.advance_answer(&next.thread, &next.version, next.step, false);
        true
    }
    pub(super) fn question_shortcut(&mut self, event: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
        let rune::Key::Text(key) = &event.key else {
            return false;
        };
        if event.mods.cmd
            || event.mods.ctrl
            || event.mods.alt
            || event.mods.shift
            || key.len() != 1
            || tx
                .focused()
                .is_some_and(|focus| focus == "thread-search" || focus.starts_with("answer-"))
        {
            return false;
        }
        let Some(index) = key.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else {
            return false;
        };
        let Some(p) = &self.workspace.conversation else {
            return false;
        };
        let Some(f) = self.workspace.questions.get(&p.id) else {
            return false;
        };
        if f.collapsed
            || p.approval_menu.is_some()
            || !self.question_enabled(&p.id, &f.prompt.version, f.step)
        {
            return false;
        }
        let Some(choice) = f.prompt.questions[f.step].options.get(index) else {
            return false;
        };
        let (id, version, step, value) = (
            p.id.clone(),
            f.prompt.version.clone(),
            f.step,
            choice.value().to_owned(),
        );
        if !tx.key_repeat() {
            self.choose_answer(&id, &version, step, &value, tx);
        }
        true
    }
    fn question_enabled(&self, thread: &str, version: &str, step: usize) -> bool {
        self.can_operate
            && !self.pending
            && !self.workspace.show_projects
            && self.new_threads.active.is_none()
            && self
                .workspace
                .conversation
                .as_ref()
                .is_some_and(|p| p.id == thread && p.status == LiveStatus::Live)
            && self
                .workspace
                .questions
                .get(thread)
                .is_some_and(|f| f.prompt.version == version && f.step == step)
    }
    fn advance_answer(&mut self, thread: &str, version: &str, step: usize, dismiss: bool) {
        if !self.question_enabled(thread, version, step) {
            return;
        }
        self.answer_advance = None;
        let f = self.workspace.questions.get_mut(thread).unwrap();
        if dismiss {
            if !f.prompt.dismissible {
                return;
            }
        } else {
            if f.prompt.response_capability == "not_resumable" || !f.can_advance() {
                return;
            }
            if !f.last() {
                f.step += 1;
                f.collapsed = false;
                return;
            }
        }
        let request = f.request(thread, dismiss);
        self.send(request);
    }
    fn choose_answer(
        &mut self,
        thread: &str,
        version: &str,
        step: usize,
        value: &str,
        tx: &mut rune::Tx,
    ) {
        if !self.question_enabled(thread, version, step) {
            return;
        }
        let f = self.workspace.questions.get_mut(thread).unwrap();
        let q = &f.prompt.questions[step];
        if f.collapsed
            || f.prompt.response_capability == "not_resumable"
            || !q.options.iter().any(|c| c.value() == value)
        {
            return;
        }
        let a = &mut f.answers[step];
        let old = std::mem::replace(&mut a.editor, TextEdit::new(""));
        let displaced = &old.text;
        if q.multi_select {
            if let Some(i) = a.selected.iter().position(|v| v == value) {
                a.selected.remove(i);
            } else {
                a.selected.push(value.into());
            }
        } else {
            a.selected = vec![value.into()];
        }
        let advance = !q.multi_select;
        if std::mem::take(&mut a.edited) && !displaced.trim().is_empty() {
            self.workspace
                .drafts
                .entry(thread.into())
                .or_default()
                .preserve_answer(displaced);
        }
        if advance {
            let delay = Duration::from_millis(200);
            self.answer_advance = Some(Advance {
                thread: thread.into(),
                version: version.into(),
                step,
                deadline: Instant::now() + delay,
            });
            tx.wake_after(delay);
        }
    }
}

fn label(value: &str, size: f32, color: rune::Color) -> El {
    text(value)
        .font(TextStyle::new(size, 400.))
        .color(color)
        .wrap_anywhere()
}

pub(super) fn banner(s: &Desktop, thread: &str, theme: &tok::Theme, dark: bool) -> El {
    let form = &s.workspace.questions[thread];
    let step = form.step;
    let q = &form.prompt.questions[step];
    let version = &form.prompt.version;
    let enabled = s.question_enabled(thread, version, step);
    let can_answer = enabled && form.prompt.response_capability != "not_resumable";
    let kit = Kit::neutral(dark);
    let mut header = row().w_full().gap(6.).center_y().min_h(24.).child(label(
        &q.header,
        12.,
        theme.text_secondary,
    ));
    if form.collapsed {
        header = header.child(text("·").color(theme.text_secondary)).child(
            text(&q.question)
                .font(TextStyle::new(12., 400.))
                .color(theme.text_secondary)
                .grow(1.)
                .min_w(0.)
                .truncate(),
        );
    } else {
        header = header.child(spacer());
    }
    if form.answers.len() > 1 {
        header = header.child(label(
            &format!("{}/{}", step + 1, form.answers.len()),
            11.,
            theme.text_secondary,
        ));
    }
    header = header.child(
        rune::icon(
            if form.collapsed {
                &rune_kit::glyphs::CHEVRON_UP
            } else {
                &rune_kit::glyphs::CHEVRON_DOWN
            },
            14.,
        )
        .color(theme.text_secondary),
    );
    let id = thread.to_owned();
    let target = version.clone();
    let caption = if form.collapsed {
        "Show question"
    } else {
        "Hide question"
    };
    let toggle = el()
        .role("button")
        .label(caption)
        .key(format!("question-toggle-{version}"))
        .focusable(true)
        .grow(1.)
        .min_w(0.)
        .child(header)
        .on_press(move |s: &mut Desktop| {
            if let Some(f) = s.workspace.questions.get_mut(&id)
                && f.prompt.version == target
            {
                s.answer_advance = None;
                f.collapsed = !f.collapsed;
            }
        });
    let mut bar = row().w_full().gap(4.).center_y().child(toggle);
    if form.prompt.dismissible {
        let id = thread.to_owned();
        let target = version.clone();
        bar = bar.child(
            super::workspace::action("Dismiss", theme, enabled, move |s| {
                s.advance_answer(&id, &target, step, true)
            })
            .key(format!("{version}-dismiss")),
        );
    }
    let mut card = col()
        .key(format!("question-{version}-{step}"))
        .w_full()
        .max_w(700.)
        .self_center()
        .px(16.)
        .pt(8.)
        .pb(6.)
        .gap(4.)
        .corners([16., 16., 0., 0.])
        .fill(theme.text.alpha(0.06))
        .border(1., theme.separator)
        .child(bar);
    if !form.collapsed {
        let style = MarkdownStyle::from_kit(&kit)
            .body_font(TextStyle::new(14., 400.).lh(20.))
            .text(theme.text);
        let mut body = col().w_full().gap(3.).child(
            Markdown::new(&q.question)
                .style(style.clone())
                .selectable(true)
                .build(&kit),
        );
        for (index, choice) in q.options.iter().enumerate() {
            let selected = form.answers[step]
                .selected
                .iter()
                .any(|v| v == choice.value());
            let ink = if selected {
                theme.text
            } else {
                theme.text_secondary
            };
            let mut captions = col().grow(1.).min_w(0.).child(
                text(&choice.label)
                    .font(TextStyle::new(14., 500.))
                    .color(ink)
                    .wrap_anywhere(),
            );
            if !choice.description.is_empty() && choice.description != choice.label {
                captions = captions.child(label(&choice.description, 11., theme.text_secondary));
            }
            let mut mark = el()
                .size(16., 16.)
                .radius(if q.multi_select { 3. } else { 8. })
                .border(1., ink)
                .center();
            if selected {
                mark = mark
                    .fill(theme.text)
                    .child(el().size(6., 6.).radius(3.).fill(theme.bg));
            }
            let id = thread.to_owned();
            let target = version.clone();
            let value = choice.value().to_owned();
            let mut item = row()
                .key(format!("{version}-{step}-option-{index}"))
                .role(if q.multi_select { "checkbox" } else { "radio" })
                .label(&choice.label)
                .checked(selected)
                .w_full()
                .px(4.)
                .py(5.)
                .gap(9.)
                .radius(5.)
                .child(mark)
                .child(captions);
            if index < 9 {
                item = item.child(
                    label(&(index + 1).to_string(), 11., theme.text_secondary)
                        .px(5.)
                        .py(2.)
                        .radius(4.)
                        .fill(theme.separator),
                );
            }
            body = body.child(if can_answer {
                item.focusable(true).hover(theme.separator).on_press_tx(
                    move |s: &mut Desktop, tx| s.choose_answer(&id, &target, step, &value, tx),
                )
            } else {
                item.disabled(true).opacity(0.6)
            });
            if let Some(preview) = &choice.preview {
                body = body.child(
                    Markdown::new(preview)
                        .style(style.clone())
                        .selectable(true)
                        .build(&kit)
                        .pl(24.),
                );
            }
        }
        if form.prompt.response_capability == "not_resumable" {
            body = body.child(label(
                "Provider process is gone. Interrupt or restart the run to respond.",
                12.,
                kit.c.warning.fg,
            ));
        }
        card = card.child(
            col()
                .key(format!("question-scroll-{version}-{step}"))
                .max_h(200.)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(body),
        );
    }
    col().w_full().px(40.).mb(-8.).child(card)
}

pub(super) fn input(s: &Desktop, thread: &str, theme: &tok::Theme) -> El {
    let f = &s.workspace.questions[thread];
    let step = f.step;
    let q = &f.prompt.questions[step];
    let id = thread.to_owned();
    let typing = id.clone();
    let submit = id.clone();
    let version = f.prompt.version.clone();
    let editable = q.allow_custom_answer != Some(false);
    let can_edit = editable
        && s.question_enabled(thread, &version, step)
        && f.prompt.response_capability != "not_resumable";
    text_input(&f.answers[step].editor, TextStyle::new(14., 400.).lh(21.))
        .key(format!("answer-{}-{step}", f.prompt.version))
        .label("Question answer")
        .color(theme.text)
        .w_full()
        .placeholder(
            if editable {
                "Type your answer…"
            } else {
                "Choose an option above"
            },
            theme.text_secondary,
        )
        .disabled(!can_edit)
        .on_edit(move |s: &mut Desktop| {
            &mut s.workspace.questions.get_mut(&id).unwrap().answers[step].editor
        })
        .on_input_change(move |s: &mut Desktop, _| {
            s.answer_advance = None;
            if let Some(f) = s.workspace.questions.get_mut(&typing) {
                f.answers[step].edited = true;
                f.answers[step].selected.clear();
            }
        })
        .on_submit(move |s: &mut Desktop, _| s.advance_answer(&submit, &version, step, false))
}

pub(super) fn controls(s: &Desktop, thread: &str, theme: &tok::Theme) -> El {
    let f = &s.workspace.questions[thread];
    let step = f.step;
    let version = &f.prompt.version;
    let enabled = s.question_enabled(thread, version, step)
        && f.prompt.response_capability != "not_resumable";
    let mut buttons = row().center_y().gap(6.);
    if step > 0 {
        let id = thread.to_string();
        let v = version.clone();
        buttons = buttons.child(
            super::workspace::action("Previous", theme, enabled, move |s| {
                if s.question_enabled(&id, &v, step) {
                    s.answer_advance = None;
                    let f = s.workspace.questions.get_mut(&id).unwrap();
                    f.step -= 1;
                    f.collapsed = false;
                }
            })
            .key(format!("{version}-{step}-previous")),
        );
    }
    let id = thread.to_owned();
    let v = version.clone();
    let caption = if f.last() { "Submit" } else { "Next" };
    let button = el()
        .key(format!("{version}-{step}-advance"))
        .label(caption)
        .role("button")
        .px(12.)
        .h(28.)
        .radius(14.)
        .center()
        .fill(rune::oklch(0.56, 0.19, 259.))
        .child(
            text(caption)
                .font(TextStyle::new(13., 500.))
                .color(rune::Color::WHITE),
        );
    buttons.child(if enabled && f.can_advance() {
        button
            .focusable(true)
            .on_press(move |s: &mut Desktop| s.advance_answer(&id, &v, step, false))
    } else {
        button.disabled(true).opacity(0.5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repeated_snapshots_keep_answers_until_the_presented_question_changes() {
        let prompt = |version: &str| {
            serde_json::from_value(json!({
            "requestId":"request","version":version,"responseCapability":"live","dismissible":false,
            "questions":[{"id":"answer","header":"Question","question":"Choose languages","options":[],"multiSelect":true}]
        })).unwrap()
        };
        let mut forms = Forms::new();
        replace(&mut forms, "thread", vec![prompt("v1")]);
        forms.get_mut("thread").unwrap().answers[0].editor = TextEdit::new("a draft");
        replace(&mut forms, "thread", vec![prompt("v1")]);
        assert_eq!(forms["thread"].answers[0].editor.text, "a draft");
        let mut recovered: Prompt = prompt("v2");
        recovered.response_capability = "message".into();
        recovered.dismissible = true;
        replace(&mut forms, "thread", vec![recovered]);
        assert_eq!(forms["thread"].answers[0].editor.text, "a draft");
        assert!(forms["thread"].prompt.dismissible);
        let mut revised: Prompt = prompt("v3");
        revised.questions[0].question = "Choose a deployment target".into();
        replace(&mut forms, "thread", vec![revised]);
        assert!(forms["thread"].answers[0].editor.text.is_empty());
        replace(&mut forms, "thread", vec![]);
        assert!(!forms.contains_key("thread"));
    }
}
