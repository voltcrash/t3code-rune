use cubeb::{
    ChannelLayout, Context, MonoFrame, SampleFormat, State, StreamBuilder, StreamParamsBuilder,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

pub(crate) const RATE: usize = 16_000;
const LIMIT: usize = RATE * 10;
pub(crate) type Recording = Arc<[f32]>;

#[derive(Clone, Copy)]
pub(crate) enum Failure {
    Input,
    Empty,
    Playback,
}

pub(crate) enum Activity {
    Record,
    Play(Recording, usize),
}

pub(crate) enum Event {
    Started,
    Recorded(Recording),
    Played(usize),
    Failed(Failure),
}

enum Signal {
    Stop,
    Cancel,
    Drained,
    Error,
}

#[derive(Default)]
struct Progress {
    frames: AtomicUsize,
    level: AtomicUsize,
}

pub(crate) struct Job {
    stop: mpsc::Sender<Signal>,
    events: mpsc::Receiver<Event>,
    progress: Arc<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = self.stop.send(Signal::Cancel);
    }
}

impl Job {
    pub(crate) fn start(activity: Activity) -> Self {
        let (stop, commands) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        let state = progress.clone();
        let signal = stop.clone();
        std::thread::spawn(move || {
            let failure = match activity {
                Activity::Record => Failure::Input,
                Activity::Play(..) => Failure::Playback,
            };
            let result = run(activity, &state, signal, commands, &events);
            if let Some(event) = result.unwrap_or(Some(Event::Failed(failure))) {
                let _ = events.send(event);
            }
        });
        Self {
            stop,
            events: receive,
            progress,
        }
    }

    pub(crate) fn stop(&self) {
        let _ = self.stop.send(Signal::Stop);
    }
    pub(crate) fn event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    pub(crate) fn progress(&self) -> (usize, usize) {
        (
            self.progress.frames.load(Ordering::Relaxed),
            self.progress.level.load(Ordering::Relaxed),
        )
    }
}

fn run(
    activity: Activity,
    progress: &Arc<Progress>,
    signal: mpsc::Sender<Signal>,
    commands: mpsc::Receiver<Signal>,
    events: &mpsc::Sender<Event>,
) -> cubeb::Result<Option<Event>> {
    let ctx = Context::init(Some(c"T3 Code"), None)?;
    let params = StreamParamsBuilder::new()
        .format(SampleFormat::Float32NE)
        .rate(RATE as u32)
        .channels(1)
        .layout(ChannelLayout::MONO)
        .take();
    let mut stream = StreamBuilder::<MonoFrame<f32>>::new();
    stream
        .name("T3 Code microphone test")
        .latency(ctx.min_latency(&params).unwrap_or(512).max(256))
        .state_callback(move |state| {
            let reason = match state {
                State::Error => Signal::Error,
                State::Drained => Signal::Drained,
                _ => return,
            };
            let _ = signal.send(reason);
        });
    let counter = progress.clone();
    let capture = match &activity {
        Activity::Record => {
            let samples = Arc::new(Mutex::new(Vec::with_capacity(LIMIT)));
            let buffer = samples.clone();
            stream
                .default_input(&params)
                .data_callback(move |input, _| {
                    let Ok(mut dest) = buffer.try_lock() else {
                        return -1;
                    };
                    let count = input.len().min(LIMIT - dest.len());
                    let mut peak = 0.0_f32;
                    for frame in &input[..count] {
                        dest.push(frame.m);
                        peak = peak.max(frame.m.abs());
                    }
                    counter.frames.store(dest.len(), Ordering::Relaxed);
                    counter
                        .level
                        .store((peak.min(1.) * 100.).round() as usize, Ordering::Relaxed);
                    count as isize
                });
            Some(samples)
        }
        Activity::Play(samples, offset) => {
            let samples = samples.clone();
            let mut pos = *offset;
            stream
                .default_output(&params)
                .data_callback(move |_, output| {
                    let count = output.len().min(samples.len() - pos);
                    for (frame, sample) in
                        output[..count].iter_mut().zip(&samples[pos..pos + count])
                    {
                        frame.m = *sample;
                    }
                    pos += count;
                    counter.frames.store(pos, Ordering::Relaxed);
                    count as isize
                });
            None
        }
    };
    let stream = stream.init(&ctx)?;
    if commands.try_recv().is_ok() {
        return Ok(None);
    }
    stream.start()?;
    if events.send(Event::Started).is_err() {
        return Ok(None);
    }
    let timeout = Duration::from_secs(if capture.is_some() { 10 } else { 12 });
    let reason = commands.recv_timeout(timeout);
    let played = match reason {
        Ok(Signal::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
        Ok(Signal::Error) => {
            return Ok(Some(Event::Failed(if capture.is_some() {
                Failure::Input
            } else {
                Failure::Playback
            })));
        }
        Ok(Signal::Drained) => 0,
        Err(mpsc::RecvTimeoutError::Timeout) if capture.is_none() => {
            return Ok(Some(Event::Failed(Failure::Playback)));
        }
        _ => stream.position().unwrap_or(0) as usize,
    };
    drop(stream);
    if let Some(samples) = capture {
        let mut data = samples.lock().unwrap_or_else(|e| e.into_inner());
        return Ok(Some(if data.is_empty() {
            Event::Failed(Failure::Empty)
        } else {
            Event::Recorded(std::mem::take(&mut *data).into())
        }));
    }
    let Activity::Play(samples, offset) = activity else {
        unreachable!()
    };
    Ok(Some(Event::Played(
        if matches!(reason, Ok(Signal::Drained)) {
            0
        } else {
            (offset + played).min(samples.len())
        },
    )))
}
