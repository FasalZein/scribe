# scribe

Rust CLI (`src/`) plus the agent skill (`SKILL.md`, `reference/`). Domain terms live in `GLOSSARY.md`; decisions live in `docs/adr/`.

- Delegated work on this repository (workers, reviewers, any helper that edits or reviews the CLI or the skill) runs on Opus: `anthropic/claude-opus-5-5`. The user asked for this.
- Verify with `cargo build --release && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
- The accuracy regression test needs the model and a media file, so plain `cargo test` skips it. Run it with `SCRIBE_REGRESSION_MODEL=... SCRIBE_REGRESSION_MEDIA=... cargo test --release -- --ignored` after any change to chunking or the engine (see ADR 0003).
- Word count alone does not prove accuracy. Compare against a reference transcript, as in ADR 0003.
