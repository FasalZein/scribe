//! Per-stage wall times for `--timings`, printed as one line on stderr.
use std::time::Instant;

pub struct Timings {
    enabled: bool,
    start: Instant,
    stages: Vec<(&'static str, f64)>,
}
impl Timings {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            start: Instant::now(),
            stages: Vec::new(),
        }
    }
    pub fn add(&mut self, stage: &'static str, secs: f64) {
        self.stages.push((stage, secs));
    }
    /// Run `f` and record its wall time under `stage`.
    pub fn time<T>(&mut self, stage: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let value = f();
        self.add(stage, start.elapsed().as_secs_f64());
        value
    }
    pub fn report(&self) {
        if !self.enabled {
            return;
        }
        let stages: Vec<String> = self
            .stages
            .iter()
            .map(|(stage, secs)| format!("{stage} {secs:.2}s"))
            .collect();
        eprintln!(
            "timings: {}; total {:.2}s",
            stages.join(", "),
            self.start.elapsed().as_secs_f64()
        );
    }
}
