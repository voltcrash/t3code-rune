use super::*;
use miniaudio::{Backend, Context, Device, DeviceConfig, DeviceType, Error, Format};
use std::sync::Mutex;
use std::time::Duration;

enum Signal {
    Stop,
    Drained,
    DeviceStopped,
}

impl Progress {
    fn capture(&self, buffer: &Mutex<Vec<f32>>, input: &[f32], limit: usize) -> bool {
        let mut data = buffer.lock().unwrap_or_else(|e| e.into_inner());
        if data.len() == limit {
            return false;
        }
        let input = &input[..input.len().min(limit - data.len())];
        data.extend_from_slice(input);
        let peak = input.iter().fold(0.0_f32, |peak, v| peak.max(v.abs()));
        self.level
            .store((peak.min(1.) * 100.).round() as usize, Ordering::Relaxed);
        self.frames.store(data.len(), Ordering::Relaxed);
        data.len() == limit
    }
}

fn run(
    activity: Activity,
    record_seconds: usize,
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

    let context = Context::new(&backends, None)?;
    let state = progress.clone();
    let stopped = signal.clone();
    let (mut config, capture) = match activity {
        Activity::Record => {
            let samples = Arc::new(Mutex::new(Vec::with_capacity(RATE * 10)));
            let buffer = samples.clone();
            let mut config = DeviceConfig::new(DeviceType::Capture);
            config.set_sample_rate(RATE as u32);
            config.capture_mut().set_format(Format::F32);
            config.capture_mut().set_channels(1);
            config.set_data_callback(move |_, _, input| {
                if state.capture(&buffer, input.as_samples::<f32>(), RATE * record_seconds) {
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
    config.set_stop_callback(move |_| {
        let _ = stopped.send(Signal::DeviceStopped);
    });
    let device = Device::new(Some(context), &config)?;
    device.start()?;
    if events.send(Event::Started).is_err() {
        return Ok(None);
    }
    let timeout = Duration::from_secs(if capture.is_some() {
        record_seconds as u64
    } else {
        12
    });
    let reason = commands.recv_timeout(timeout);
    drop(device);
    if matches!(reason, Err(mpsc::RecvTimeoutError::Disconnected)) {
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

pub(crate) fn helper(dictating: bool) -> io::Result<()> {
    let mut args = std::env::args().skip(2);
    let mut input = BufReader::new(io::stdin());
    let number = |s: &str| s.parse::<usize>().map_err(io::Error::other);
    let activity = if let Some(count) = args.next() {
        let offset = number(
            &args
                .next()
                .ok_or_else(|| io::Error::other("Missing playback offset"))?,
        )?;
        Activity::Play(read_samples(&mut input, number(&count)?)?, offset)
    } else {
        Activity::Record
    };
    let failure = match activity {
        Activity::Record => Failure::Input,
        Activity::Play(..) => Failure::Playback,
    };
    let (stop, commands) = mpsc::channel();
    let signal = stop.clone();
    std::thread::spawn(move || {
        let mut byte = [0];
        while input.read_exact(&mut byte).is_ok() {
            let _ = stop.send(Signal::Stop);
        }
        // The parent can disappear while a platform audio API is blocked in initialization.
        std::process::exit(0);
    });
    let progress = Arc::new(Progress::default());
    let state = progress.clone();
    let (events, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let event = run(
            activity,
            if dictating { 300 } else { 10 },
            &state,
            signal,
            commands,
            &events,
        )
        .ok()
        .flatten()
        .unwrap_or(Event::Failed(failure));
        let _ = events.send(event);
    });
    let mut output = BufWriter::new(io::stdout().lock());
    loop {
        let (reply, samples, done) = match receive.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::Recorded(samples)) => (Reply::Recorded(samples.len()), Some(samples), true),
            Ok(Event::Played(offset)) => (Reply::Played(offset), None, true),
            Ok(Event::Failed(reason)) => (Reply::Failed(reason), None, true),
            Ok(Event::Started) => (Reply::Started, None, false),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("Audio device closed"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (
                Reply::Progress(
                    progress.frames.load(Ordering::Relaxed),
                    progress.level.load(Ordering::Relaxed),
                ),
                None,
                false,
            ),
        };
        serde_json::to_writer(&mut output, &reply)?;
        output.write_all(b"\n")?;
        if let Some(samples) = samples {
            write_samples(&mut output, &samples)?;
        }
        output.flush()?;
        if done {
            return Ok(());
        }
    }
}
