mod device;

use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

pub(crate) const RATE: usize = 16_000;
const LIMIT: usize = RATE * 10;
pub(crate) type Recording = Arc<[f32]>;

#[derive(Clone, Copy, Serialize, Deserialize)]
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

#[derive(Default)]
struct Progress {
    frames: AtomicUsize,
    level: AtomicUsize,
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Started,
    Progress(usize, usize),
    Recorded(usize),
    Played(usize),
    Failed(Failure),
}

fn write_samples(mut output: impl Write, samples: &[f32]) -> io::Result<()> {
    let mut bytes = BufWriter::new(&mut output);
    for sample in samples {
        bytes.write_all(&sample.to_le_bytes())?;
    }
    bytes.flush()
}

fn read_samples(input: &mut impl Read, count: usize) -> io::Result<Recording> {
    if count > LIMIT {
        return Err(io::Error::other(
            "Recording exceeds the microphone test limit",
        ));
    }
    let mut bytes = vec![0; count * 4];
    input.read_exact(&mut bytes)?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes)
        .collect())
}

pub(crate) struct Job {
    child: Option<Child>,
    stop: mpsc::Sender<()>,
    events: mpsc::Receiver<Event>,
    progress: Arc<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

impl Job {
    pub(crate) fn start(activity: Activity) -> Self {
        let (events, receive) = mpsc::channel();
        let (stop, stopping) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        let failure = match &activity {
            Activity::Record => Failure::Input,
            Activity::Play(..) => Failure::Playback,
        };
        let result = (|| -> io::Result<Child> {
            let mut command = Command::new(std::env::current_exe()?);
            command.arg("--microphone-test");
            if let Activity::Play(samples, offset) = &activity {
                command
                    .arg(samples.len().to_string())
                    .arg(offset.to_string());
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let mut input = child.stdin.take().unwrap();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            std::thread::spawn(move || {
                if let Activity::Play(samples, _) = activity
                    && write_samples(&mut input, &samples).is_err()
                {
                    return;
                }
                if stopping.recv().is_ok() {
                    let _ = input.write_all(&[1]);
                    // Keep stdin open until the job is dropped. EOF ends the helper.
                    let _ = stopping.recv();
                }
            });
            let events = events.clone();
            let state = progress.clone();
            std::thread::spawn(move || {
                let event =
                    read_events(&mut output, &events, &state).unwrap_or(Event::Failed(failure));
                let _ = events.send(event);
            });
            Ok(child)
        })();
        let child = match result {
            Ok(child) => Some(child),
            Err(_) => {
                let _ = events.send(Event::Failed(failure));
                None
            }
        };
        Self {
            child,
            stop,
            events: receive,
            progress,
        }
    }
    pub(crate) fn stop(&self) {
        let _ = self.stop.send(());
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

fn read_events(
    output: &mut impl BufRead,
    events: &mpsc::Sender<Event>,
    progress: &Progress,
) -> io::Result<Event> {
    let mut line = String::new();
    loop {
        line.clear();
        output.read_line(&mut line)?;
        match serde_json::from_str(&line)? {
            Reply::Started => {
                events
                    .send(Event::Started)
                    .map_err(|_| io::Error::other("Microphone test closed"))?;
            }
            Reply::Progress(frames, level) => {
                progress.frames.store(frames, Ordering::Relaxed);
                progress.level.store(level, Ordering::Relaxed);
            }
            Reply::Recorded(count) => return Ok(Event::Recorded(read_samples(output, count)?)),
            Reply::Played(offset) => return Ok(Event::Played(offset)),
            Reply::Failed(reason) => return Ok(Event::Failed(reason)),
        }
    }
}

pub(crate) use device::helper;
