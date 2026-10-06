//! Per-stage wall times for publication and the optional `--timings` stderr line.
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
    /// The first recorded time of `stage`, or zero.
    pub fn get(&self, stage: &str) -> f64 {
        self.stages
            .iter()
            .find(|(name, _)| *name == stage)
            .map_or(0.0, |(_, secs)| *secs)
    }
    /// Run `f` and record its wall time under `stage`.
    pub fn time<T>(&mut self, stage: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let value = f();
        self.add(stage, start.elapsed().as_secs_f64());
        value
    }
    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
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
        crate::progress::line!(
            "timings: {}; total {:.2}s",
            stages.join(", "),
            self.elapsed()
        );
    }
}
