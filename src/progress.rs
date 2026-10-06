//! Terminal-only progress. All scribe diagnostics share the same lock so a refresh
//! cannot erase a warning, a failure, or a message from the model loader thread.
use std::{
    io::{self, IsTerminal, Write},
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Fetch,
    Download,
    Decode,
    Transcribe,
    Write,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Download => "download",
            Self::Decode => "decode",
            Self::Transcribe => "transcribe",
            Self::Write => "write",
        }
    }
}

struct Frame {
    source: String,
    stage: Stage,
    processed: f64,
    total: Option<f64>,
    started: Instant,
}
static ACTIVE: Mutex<Option<Frame>> = Mutex::new(None);
const REFRESH: Duration = Duration::from_millis(250);
const CLEAR: &str = "\r\x1b[2K";

pub struct Progress(Option<(mpsc::Sender<()>, JoinHandle<()>)>);
impl Progress {
    pub fn new(source: &str) -> Self {
        if !io::stderr().is_terminal() {
            return Self(None);
        }
        // Keep the end of long paths and URLs, where the source name or ID lives.
        let source: Vec<_> = source.chars().filter(|c| !c.is_control()).collect();
        let label = if source.len() > 32 {
            format!(
                "…{}",
                source[source.len() - 31..].iter().collect::<String>()
            )
        } else {
            source.iter().collect()
        };
        let (stop, receiver) = mpsc::channel();
        let mut active = ACTIVE.lock().unwrap();
        *active = Some(Frame {
            source: label,
            stage: Stage::Fetch,
            processed: 0.0,
            total: None,
            started: Instant::now(),
        });
        render(&active);
        drop(active);
        Self(Some((
            stop,
            thread::spawn(move || {
                while receiver.recv_timeout(REFRESH) == Err(mpsc::RecvTimeoutError::Timeout) {
                    render(&ACTIVE.lock().unwrap());
                }
            }),
        )))
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        if let Some((stop, ticker)) = self.0.take() {
            let _ = stop.send(());
            ticker.join().expect("progress ticker panicked");
            let mut active = ACTIVE.lock().unwrap();
            *active = None;
            let mut stderr = io::stderr().lock();
            let _ = write!(stderr, "{CLEAR}");
            let _ = stderr.flush();
        }
    }
}

/// Stages cannot go backwards: decode continues in parallel after transcription starts.
pub fn update(stage: Stage, processed: f64, total: Option<f64>) {
    let mut active = ACTIVE.lock().unwrap();
    if let Some(frame) = active.as_mut() {
        if stage < frame.stage {
            return;
        }
        if stage != frame.stage {
            frame.stage = stage;
            frame.started = Instant::now();
        }
        frame.processed = processed;
        frame.total = total.filter(|total| total.is_finite() && *total > 0.0);
        render(&active);
    }
}

pub fn message(args: std::fmt::Arguments<'_>) {
    let active = ACTIVE.lock().unwrap();
    let mut stderr = io::stderr().lock();
    if active.is_some() {
        let _ = write!(stderr, "{CLEAR}");
    }
    let _ = writeln!(stderr, "{args}");
    // The next refresh restores progress below the permanent diagnostic line.
}
macro_rules! line {
    ($($args:tt)*) => { $crate::progress::message(format_args!($($args)*)) };
}
pub(crate) use line;

fn render(active: &Option<Frame>) {
    if let Some(frame) = active {
        let line = format_line(
            &frame.source,
            frame.stage.name(),
            frame.processed,
            frame.total,
            frame.started.elapsed(),
        );
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "{CLEAR}{line}");
        let _ = stderr.flush();
    }
}

fn format_line(
    source: &str,
    stage: &str,
    processed: f64,
    total: Option<f64>,
    elapsed: Duration,
) -> String {
    let Some(total) = total else {
        return format!("{source}: {stage} | ETA unknown");
    };
    let processed = processed.clamp(0.0, total);
    let percent = (processed / total * 100.0).floor() as u32;
    let estimate = if processed == total {
        "0s".to_owned()
    } else if processed > 0.0 && !elapsed.is_zero() {
        let seconds = (elapsed.as_secs_f64() * (total - processed) / processed).ceil() as u64;
        match seconds {
            0..60 => format!("{seconds}s"),
            60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
            _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
        }
    } else {
        "unknown".to_owned()
    };
    format!("{source}: {stage} {percent}% | ETA {estimate}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_total_or_no_processed_audio_has_no_estimate() {
        assert_eq!(
            format_line("talk", "fetch", 0.0, None, Duration::from_secs(10)),
            "talk: fetch | ETA unknown"
        );
        assert_eq!(
            format_line(
                "talk",
                "transcribe",
                0.0,
                Some(120.0),
                Duration::from_secs(10)
            ),
            "talk: transcribe 0% | ETA unknown"
        );
    }

    #[test]
    fn estimates_round_up_and_format_minutes_and_hours() {
        assert_eq!(
            format_line(
                "talk",
                "transcribe",
                1.0,
                Some(4.0),
                Duration::from_millis(500)
            ),
            "talk: transcribe 25% | ETA 2s"
        );
        assert_eq!(
            format_line(
                "talk",
                "transcribe",
                30.0,
                Some(120.0),
                Duration::from_secs(30)
            ),
            "talk: transcribe 25% | ETA 1m 30s"
        );
        assert_eq!(
            format_line(
                "talk",
                "transcribe",
                30.0,
                Some(120.0),
                Duration::from_secs(1200)
            ),
            "talk: transcribe 25% | ETA 1h 00m"
        );
    }

    #[test]
    fn completed_audio_is_capped_at_one_hundred_percent() {
        assert_eq!(
            format_line(
                "talk",
                "decode",
                121.0,
                Some(120.0),
                Duration::from_secs(10)
            ),
            "talk: decode 100% | ETA 0s"
        );
    }

    #[test]
    fn percentage_and_estimate_use_processed_audio() {
        // 30 of 120 audio seconds took 10 wall seconds: 25%, with 30 seconds left.
        assert_eq!(
            format_line(
                "talk",
                "transcribe",
                30.0,
                Some(120.0),
                Duration::from_secs(10)
            ),
            "talk: transcribe 25% | ETA 30s"
        );
    }
}
