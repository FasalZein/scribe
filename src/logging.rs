//! Configure native engine and ggml logging once, before any model loads.
struct EngineLogger;
static LOGGER: EngineLogger = EngineLogger;

impl log::Log for EngineLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "transcribe_cpp"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            // Logging must not panic across the native callback if stderr is closed.
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "{}", record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init(verbose: bool) {
    if verbose {
        log::set_logger(&LOGGER).expect("engine logger is installed only once at startup");
        log::set_max_level(log::LevelFilter::Trace);
        transcribe_cpp::init_logging();
    } else {
        transcribe_cpp::disable_logging();
    }
}
