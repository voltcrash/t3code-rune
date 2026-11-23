use miniaudio::{Backend, Context, Device, DeviceConfig, DeviceType, Error, Format};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    DeviceStopped,
}

#[derive(Default)]
struct Progress {
    cancelled: AtomicBool,
    frames: AtomicUsize,
    level: AtomicUsize,
}

impl Progress {
    fn capture(&self, buffer: &Mutex<Vec<f32>>, input: &[f32]) -> bool {
        if self.cancelled.load(Ordering::Acquire) {
            return false;
        }
        let mut data = buffer.lock().unwrap_or_else(|e| e.into_inner());
        if data.len() == LIMIT {
            return false;
        }
        let input = &input[..input.len().min(LIMIT - data.len())];
        data.extend_from_slice(input);
        let peak = input.iter().fold(0.0_f32, |peak, v| peak.max(v.abs()));
        self.level
            .store((peak.min(1.) * 100.).round() as usize, Ordering::Relaxed);
        self.frames.store(data.len(), Ordering::Relaxed);
        data.len() == LIMIT
    }
}

pub(crate) struct Job {
    stop: mpsc::Sender<Signal>,
    events: mpsc::Receiver<Event>,
    progress: Arc<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.progress.cancelled.store(true, Ordering::Release);
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
) -> Result<Option<Event>, Error> {
    #[cfg(target_os = "linux")]
    let backends = [Backend::PulseAudio, Backend::Alsa, Backend::Jack];
    #[cfg(target_os = "macos")]
    let backends = [Backend::CoreAudio];
    #[cfg(windows)]
    let backends = [Backend::Wasapi, Backend::DSound, Backend::WinMM];

    if progress.cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let context = Context::new(&backends, None)?;
    if progress.cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let state = progress.clone();
    let stopped = signal.clone();
    let (mut config, capture) = match activity {
        Activity::Record => {
            let samples = Arc::new(Mutex::new(Vec::with_capacity(LIMIT)));
            let buffer = samples.clone();
            let mut config = DeviceConfig::new(DeviceType::Capture);
            config.set_sample_rate(RATE as u32);
            config.capture_mut().set_format(Format::F32);
            config.capture_mut().set_channels(1);
            config.set_data_callback(move |_, _, input| {
                if state.capture(&buffer, input.as_samples::<f32>()) {
                    let _ = signal.send(Signal::Drained);
                }
            });
            (config, Some(samples))
        }
        Activity::Play(samples, offset) => {
            let mut cursor = offset.min(samples.len());
            let mut silence = 0;
            let mut config = DeviceConfig::new(DeviceType::Playback);
            config.set_sample_rate(RATE as u32);
            config.playback_mut().set_format(Format::F32);
            config.playback_mut().set_channels(1);
            config.set_period_size_in_milliseconds(10);
            config.set_data_callback(move |_, output, _| {
                let output = output.as_samples_mut::<f32>();
                output.fill(0.);
                if state.cancelled.load(Ordering::Acquire) {
                    return;
                }
                let count = output.len().min(samples.len() - cursor);
                output[..count].copy_from_slice(&samples[cursor..cursor + count]);
                cursor += count;
                state.frames.store(cursor, Ordering::Relaxed);
                // Let queued hardware periods play the final samples before closing the device.
                if cursor == samples.len() && silence < RATE / 10 {
                    silence += output.len() - count;
                    if silence >= RATE / 10 {
                        let _ = signal.send(Signal::Drained);
                    }
                }
            });
            (config, None)
        }
    };
    if progress.cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    config.set_stop_callback(move |_| {
        let _ = stopped.send(Signal::DeviceStopped);
    });
    let device = Device::new(Some(context), &config)?;
    if progress.cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    device.start()?;
    if progress.cancelled.load(Ordering::Acquire) || events.send(Event::Started).is_err() {
        return Ok(None);
    }
    let timeout = Duration::from_secs(if capture.is_some() { 10 } else { 12 });
    let reason = commands.recv_timeout(timeout);
    drop(device);
    if progress.cancelled.load(Ordering::Acquire)
        || matches!(
            reason,
            Ok(Signal::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected)
        )
    {
        return Ok(None);
    }
    if matches!(reason, Ok(Signal::DeviceStopped)) {
        return Ok(Some(Event::Failed(if capture.is_some() {
            Failure::Input
        } else {
            Failure::Playback
        })));
    }
    if let Some(samples) = capture {
        let mut data = samples.lock().unwrap_or_else(|e| e.into_inner());
        return Ok(Some(if data.is_empty() {
            Event::Failed(Failure::Empty)
        } else {
            Event::Recorded(std::mem::take(&mut *data).into())
        }));
    }
    Ok(Some(match reason {
        Ok(Signal::Drained) => Event::Played(0),
        Ok(Signal::Stop) => Event::Played(progress.frames.load(Ordering::Relaxed)),
        _ => Event::Failed(Failure::Playback),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_during_startup_discards_late_input() {
        let progress = Arc::new(Progress::default());
        let (stop, commands) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        let job = Job {
            stop: stop.clone(),
            events: receive,
            progress: progress.clone(),
        };
        drop(job);

        let buffer = Mutex::new(Vec::new());
        assert!(!progress.capture(&buffer, &[0.5; 32]));
        assert!(buffer.into_inner().unwrap().is_empty());
        assert!(
            run(Activity::Record, &progress, stop, commands, &events)
                .unwrap()
                .is_none()
        );
    }
}
