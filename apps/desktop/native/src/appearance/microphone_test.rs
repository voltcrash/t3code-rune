use super::{Desktop, button, label};
use crate::microphone::{Activity, Event, Failure, Job, RATE, Recording};
use rune::{El, Tx, col, el, row, tok};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
pub(super) struct Labels {
    pub(super) title: String,
    description: String,
    record: String,
    again: String,
    stop: String,
    opening: String,
    cancel: String,
    play: String,
    pause: String,
    discard: String,
    level: String,
    input_error: String,
    empty_error: String,
    play_error: String,
}

#[derive(Default)]
pub(super) struct Test {
    job: Option<Job>,
    recording: Option<Recording>,
    opened: bool,
    stopping: bool,
    offset: usize,
    seconds: usize,
    level: usize,
    error: Option<Failure>,
}

impl Test {
    fn record(&mut self) {
        *self = Self {
            job: Some(Job::start(Activity::Record)),
            ..Self::default()
        };
    }

    fn play(&mut self) {
        if let Some(samples) = &self.recording {
            self.error = None;
            self.opened = false;
            self.job = Some(Job::start(Activity::Play(samples.clone(), self.offset)));
        }
    }

    fn stop(&mut self) {
        if let Some(job) = &self.job {
            self.stopping = true;
            job.stop();
        }
    }

    fn poll(&mut self, tx: &mut Tx) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let (frames, level) = job.progress();
        let seconds = frames / RATE;
        let changed = (seconds, level) != (self.seconds, self.level);
        self.seconds = seconds;
        self.level = level;
        let event = job.event();
        if matches!(event, None | Some(Event::Started)) {
            tx.wake_after(Duration::from_millis(100));
            if event.is_some() {
                self.opened = true;
                return true;
            }
            return changed;
        }
        match event {
            Some(Event::Recorded(samples)) => {
                self.recording = Some(samples);
                self.offset = 0;
            }
            Some(Event::Played(offset)) => {
                self.offset = self
                    .recording
                    .as_ref()
                    .filter(|s| offset < s.len())
                    .map_or(0, |_| offset);
            }
            Some(Event::Failed(error)) => self.error = Some(error),
            _ => unreachable!(),
        }
        self.job = None;
        self.stopping = false;
        self.opened = false;
        true
    }
}

impl Desktop {
    pub(crate) fn poll_microphone(&mut self, tx: &mut Tx) -> bool {
        self.voice_settings.microphone.poll(tx)
    }
}

pub(super) fn view(test: &Test, labels: &Labels, theme: &tok::Theme) -> El {
    let running = test.job.is_some();
    let recorded = test.recording.is_some();
    let mut section = col()
        .w_full()
        .gap(6.)
        .child(label(&labels.title, 16., 600., theme.text))
        .child(label(&labels.description, 13., 400., theme.text_secondary).wrap());
    let mut controls = row().w_full().flex_wrap().center_y().gap(6.);
    let action = |key: &str, caption: &str, enabled| button(key, caption, caption, enabled, theme);
    if running {
        let caption = if recorded {
            &labels.pause
        } else {
            &labels.stop
        };
        let mut stop = action("stop-test-audio", caption, test.opened && !test.stopping);
        if test.opened && !test.stopping {
            stop = stop.on_press(|s: &mut Desktop| s.voice_settings.microphone.stop());
        }
        controls = controls.child(stop);
    } else {
        let caption = if recorded {
            &labels.again
        } else {
            &labels.record
        };
        controls = controls.child(
            action("record-test-audio", caption, true)
                .on_press(|s: &mut Desktop| s.voice_settings.microphone.record()),
        );
        if recorded {
            controls = controls.child(
                action("play-test-audio", &labels.play, true)
                    .on_press(|s: &mut Desktop| s.voice_settings.microphone.play()),
            );
        }
    }
    if running || recorded {
        let caption = if recorded {
            &labels.discard
        } else {
            &labels.cancel
        };
        controls = controls.child(
            action("discard-test-audio", caption, true)
                .on_press(|s: &mut Desktop| s.voice_settings.microphone = Test::default()),
        );
    }
    section = section.child(controls);
    if running && !recorded && !test.opened {
        section = section.child(label(&labels.opening, 12., 400., theme.text_secondary));
    } else if running || recorded {
        let elapsed = if running {
            test.seconds
        } else {
            test.offset / RATE
        };
        let total = test
            .recording
            .as_ref()
            .map_or(10, |s| s.len().div_ceil(RATE));
        let mut status = row().gap(8.).center_y().child(label(
            &format!("0:{elapsed:02} / 0:{total:02}"),
            12.,
            400.,
            theme.text_secondary,
        ));
        if !recorded {
            status = status.child(
                el().role("meter")
                    .label(&labels.level)
                    .range_value(test.level as f64, 0., 100.)
                    .w(64.)
                    .h(4.)
                    .radius(2.)
                    .fill(theme.separator)
                    .child(
                        el().w_frac(test.level as f32 / 100.)
                            .h_full()
                            .radius(2.)
                            .fill(theme.text),
                    ),
            );
        }
        section = section.child(status);
    }
    if let Some(error) = test.error {
        let message = match error {
            Failure::Input => &labels.input_error,
            Failure::Empty => &labels.empty_error,
            Failure::Playback => &labels.play_error,
        };
        section = section.child(label(message, 13., 400., theme.text).wrap());
    }
    section
}
